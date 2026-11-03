//! The game loop: script lifecycle, fixed-step physics and deferred operations.

use crate::bindings::{self, entity_value, Timers, WorldRef};
use crate::physics;
use crate::world::{CollisionKind, LogLevel, World};
use grodot_core::config::Project;
use grodot_core::scene::{Component, EntityId, Scene};
use groscript::ast::StmtKind;
use groscript::{Class, GroError, Interpreter, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;

/// Per-callback execution budget (statements), so a runaway loop cannot freeze the editor.
pub const SCRIPT_BUDGET: u64 = 5_000_000;
pub const MAX_FIXED_STEPS: u32 = 5;

struct ScriptInstance {
    entity: EntityId,
    object: Value,
    path: String,
    ready: bool,
    disabled: bool,
}

pub struct Runtime {
    pub world: WorldRef,
    pub interp: Interpreter,
    scripts: Vec<ScriptInstance>,
    timers: Rc<RefCell<Timers>>,
    classes: HashMap<String, Result<Rc<Class>, String>>,
    accumulator: f32,
    started: bool,
}

impl Runtime {
    /// Build a runtime for a scene. `project_root` is used to resolve scripts and assets.
    pub fn new(scene: Scene, project_root: Option<PathBuf>, actions: BTreeMap<String, Vec<String>>) -> Runtime {
        let world = Rc::new(RefCell::new(World::new(scene, project_root.clone(), actions)));
        let timers = Rc::new(RefCell::new(Timers::default()));
        let mut interp = Interpreter::new();
        interp.step_limit = SCRIPT_BUDGET;
        interp.import_root = project_root.map(|r| r.join("scripts"));
        bindings::install(&mut interp, &world, &timers);
        Runtime {
            world,
            interp,
            scripts: Vec::new(),
            timers,
            classes: HashMap::new(),
            accumulator: 0.0,
            started: false,
        }
    }

    /// Load a project's main scene.
    pub fn from_project(project: &Project) -> Result<Runtime, String> {
        let scene = load_scene_file(project, &project.config.main_scene)?;
        let rt = Runtime::new(scene, Some(project.root.clone()), project.config.input.clone());
        rt.world.borrow_mut().scene_path = Some(project.config.main_scene.clone());
        Ok(rt)
    }

    pub fn log(&self, level: LogLevel, msg: impl Into<String>) {
        self.world.borrow_mut().log.push(level, msg);
    }

    /// Attach scripts, call `initialize()` and `ready()`, start autoplay audio.
    pub fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let ids: Vec<EntityId> = self.world.borrow().scene.entities.iter().map(|e| e.id).collect();
        self.attach(&ids);
        self.call_ready();
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    fn attach(&mut self, ids: &[EntityId]) {
        for &id in ids {
            let (script, autoplay) = {
                let w = self.world.borrow();
                let Some(e) = w.scene.entity(id) else { continue };
                let script = e.script().filter(|s| s.enabled && !s.path.is_empty()).map(|s| s.path.clone());
                let autoplay = match e.get("AudioSource") {
                    Some(Component::AudioSource(a)) if a.autoplay && !a.clip.is_empty() => {
                        Some((a.clip.clone(), a.volume, a.looping))
                    }
                    _ => None,
                };
                (script, autoplay)
            };
            if let Some((clip, volume, looping)) = autoplay {
                let mut w = self.world.borrow_mut();
                match w.resolve(&clip) {
                    Ok(path) if path.is_file() => {
                        let cmd = if looping {
                            crate::world::AudioCmd::PlayMusic { path, volume }
                        } else {
                            crate::world::AudioCmd::Play { path, volume }
                        };
                        w.audio_cmds.push(cmd);
                    }
                    _ => w.log.warn(format!("AudioSource clip '{}' not found", clip)),
                }
            }
            let Some(path) = script else { continue };
            let class = match self.load_class(&path) {
                Ok(c) => c,
                Err(e) => {
                    let name = self.world.borrow().scene.entity(id).map(|e| e.name.clone()).unwrap_or_default();
                    self.log(LogLevel::Error, format!("script for '{}' not attached: {}", name, e));
                    continue;
                }
            };
            let handle = Rc::new(bindings::EntityHandle { id, world: self.world.clone() });
            let object = self.interp.new_object(&class, Some(handle));
            self.world.borrow_mut().script_objects.insert(id, object.clone());
            self.scripts.push(ScriptInstance { entity: id, object, path, ready: false, disabled: false });
            let idx = self.scripts.len() - 1;
            self.call(idx, "initialize", vec![]);
        }
    }

    fn call_ready(&mut self) {
        for i in 0..self.scripts.len() {
            if !self.scripts[i].ready {
                self.scripts[i].ready = true;
                self.call(i, "ready", vec![]);
            }
        }
    }

    /// Load (once) and return the Entity subclass defined by a script file.
    fn load_class(&mut self, path: &str) -> Result<Rc<Class>, String> {
        if let Some(r) = self.classes.get(path) {
            return r.clone();
        }
        let result = (|| {
            let full = self.world.borrow().resolve(path)?;
            let src = std::fs::read_to_string(&full).map_err(|e| format!("cannot read '{}': {}", path, e))?;
            let stmts = groscript::parser::parse(&src).map_err(|errs| {
                let mut e = errs[0].clone();
                e.file = Some(path.into());
                e.source = Some(src.as_str().into());
                self.log_script_error(&e);
                format!("{} has syntax errors", path)
            })?;
            let class_names: Vec<String> = stmts
                .iter()
                .filter_map(|s| match &s.kind {
                    StmtKind::Class { name, .. } => Some(name.to_string()),
                    _ => None,
                })
                .collect();
            if let Err(e) = self.interp.run_source(path, &src) {
                self.log_script_error(&e);
                return Err(format!("{} failed to load", path));
            }
            for n in &class_names {
                if let Some(Value::Class(c)) = self.interp.get_global(n) {
                    if c.is_subclass_of("Entity") {
                        return Ok(c);
                    }
                }
            }
            Err(format!("{} must define a class that extends Entity (for example: class Player < Entity)", path))
        })();
        self.classes.insert(path.to_string(), result.clone());
        result
    }

    fn log_script_error(&self, e: &GroError) {
        let loc = match (&e.file, e.span) {
            (Some(f), Some(sp)) => Some((f.to_string(), sp.line)),
            _ => None,
        };
        self.world.borrow_mut().log.push_at(LogLevel::Error, e.report().trim_end().to_string(), loc);
    }

    /// Call a lifecycle method on a script instance; errors disable the instance.
    fn call(&mut self, idx: usize, method: &str, args: Vec<Value>) {
        if self.scripts[idx].disabled {
            return;
        }
        let entity = self.scripts[idx].entity;
        if !self.world.borrow().is_alive(entity) && method != "on_destroy" {
            return;
        }
        let obj = self.scripts[idx].object.clone();
        self.interp.reset_budget();
        if let Err(mut e) = self.interp.call_method_if_exists(&obj, method, args) {
            e.stack.push(format!("Runtime.{}", method));
            self.log_script_error(&e);
            let name = self.world.borrow().scene.entity(entity).map(|e| e.name.clone()).unwrap_or_default();
            self.log(
                LogLevel::Warning,
                format!("script {} on '{}' was disabled after an error", self.scripts[idx].path, name),
            );
            self.scripts[idx].disabled = true;
        }
    }

    fn index_of(&self, entity: EntityId) -> Option<usize> {
        self.scripts.iter().position(|s| s.entity == entity)
    }

    /// Advance the game by `dt` seconds (one rendered frame).
    pub fn frame(&mut self, dt: f32) {
        if !self.started {
            self.start();
        }
        let dt = dt.clamp(0.0, 0.1);
        let fixed = {
            let mut w = self.world.borrow_mut();
            w.time.delta = dt;
            w.time.elapsed += dt as f64;
            w.time.frame += 1;
            if dt > 0.0 {
                let inst = 1.0 / dt;
                w.time.fps = if w.time.fps == 0.0 { inst } else { w.time.fps * 0.9 + inst * 0.1 };
            }
            w.hud = Default::default();
            w.debug_lines.clear();
            w.time.fixed_delta
        };
        self.attach_spawned();

        // Fixed-step physics and fixed_update.
        self.accumulator += dt;
        let mut steps = 0;
        while self.accumulator >= fixed && steps < MAX_FIXED_STEPS {
            physics::step(&mut self.world.borrow_mut(), fixed);
            self.dispatch_collisions();
            for i in 0..self.scripts.len() {
                self.call(i, "fixed_update", vec![Value::Number(fixed as f64)]);
            }
            self.accumulator -= fixed;
            steps += 1;
        }
        if steps == MAX_FIXED_STEPS {
            self.accumulator = 0.0; // drop time instead of spiralling
        }

        for i in 0..self.scripts.len() {
            self.call(i, "update", vec![Value::Number(dt as f64)]);
        }
        self.run_timers();
        self.process_destroys();
        self.process_scene_load();
        self.world.borrow_mut().input.end_frame();
    }

    fn attach_spawned(&mut self) {
        let spawned: Vec<EntityId> = std::mem::take(&mut self.world.borrow_mut().spawned);
        if !spawned.is_empty() {
            self.attach(&spawned);
            self.call_ready();
        }
    }

    fn dispatch_collisions(&mut self) {
        let events = std::mem::take(&mut self.world.borrow_mut().collision_events);
        for (a, b, kind) in events {
            let method = match kind {
                CollisionKind::Enter => "on_collision",
                CollisionKind::Exit => "on_collision_exit",
            };
            for (me, other) in [(a, b), (b, a)] {
                if !self.world.borrow().is_alive(me) || !self.world.borrow().is_alive(other) {
                    continue;
                }
                if let Some(idx) = self.index_of(me) {
                    let ov = self.world.borrow().script_objects.get(&other).cloned();
                    let arg = ov.unwrap_or_else(|| entity_value(&self.world, other));
                    self.call(idx, method, vec![arg]);
                }
            }
        }
    }

    fn run_timers(&mut self) {
        let now = self.world.borrow().time.elapsed;
        let due: Vec<(u64, Value, Value, Option<EntityId>)> = {
            let mut t = self.timers.borrow_mut();
            let mut out = Vec::new();
            for timer in t.list.iter_mut() {
                if timer.due <= now {
                    out.push((timer.id, timer.callback.clone(), timer.receiver.clone(), timer.entity));
                    match timer.interval {
                        Some(iv) => timer.due += iv.max(0.001),
                        None => timer.due = f64::INFINITY,
                    }
                }
            }
            t.list.retain(|x| x.due.is_finite());
            out
        };
        for (_id, cb, receiver, entity) in due {
            if let Some(e) = entity {
                if !self.world.borrow().is_alive(e) {
                    continue;
                }
                if let Some(idx) = self.index_of(e) {
                    if self.scripts[idx].disabled {
                        continue;
                    }
                }
            }
            self.interp.reset_budget();
            let r = match &cb {
                Value::Symbol(m) => self.interp.call_method(&receiver, m, vec![]),
                f => self.interp.call_value(f, vec![]),
            };
            if let Err(mut e) = r {
                e.stack.push("Runtime.timer".into());
                self.log_script_error(&e);
            }
        }
    }

    fn process_destroys(&mut self) {
        loop {
            let pending: Vec<EntityId> = std::mem::take(&mut self.world.borrow_mut().pending_destroy);
            if pending.is_empty() {
                break;
            }
            for id in pending {
                let ids = self.world.borrow().scene.subtree(id);
                for &eid in &ids {
                    if let Some(idx) = self.index_of(eid) {
                        self.call(idx, "on_destroy", vec![]);
                    }
                }
                let mut w = self.world.borrow_mut();
                w.scene.remove_entity(id);
                for eid in &ids {
                    w.script_objects.remove(eid);
                    w.grounded.remove(eid);
                }
                w.contacts.retain(|(a, b)| !ids.contains(a) && !ids.contains(b));
                drop(w);
                self.scripts.retain(|s| !ids.contains(&s.entity));
                self.timers.borrow_mut().list.retain(|t| t.entity.map(|e| !ids.contains(&e)).unwrap_or(true));
            }
        }
    }

    fn process_scene_load(&mut self) {
        let Some(path) = self.world.borrow_mut().pending_scene_load.take() else { return };
        let root = self.world.borrow().project_root.clone();
        let full = match self.world.borrow().resolve(&path) {
            Ok(p) => p,
            Err(e) => return self.log(LogLevel::Error, e),
        };
        let scene =
            match std::fs::read_to_string(&full).map_err(|e| e.to_string()).and_then(|s| Scene::from_json_str(&s)) {
                Ok(s) => s,
                Err(e) => return self.log(LogLevel::Error, format!("cannot load scene '{}': {}", path, e)),
            };
        self.shutdown_scripts();
        {
            let mut w = self.world.borrow_mut();
            w.scene = scene;
            w.scene_path = Some(path);
            w.project_root = root;
            w.contacts.clear();
            w.grounded.clear();
            w.collision_events.clear();
            w.forces.clear();
            w.spawned.clear();
            w.pending_destroy.clear();
        }
        self.timers.borrow_mut().list.clear();
        self.accumulator = 0.0;
        let ids: Vec<EntityId> = self.world.borrow().scene.entities.iter().map(|e| e.id).collect();
        self.attach(&ids);
        self.call_ready();
    }

    fn shutdown_scripts(&mut self) {
        for i in 0..self.scripts.len() {
            self.call(i, "on_destroy", vec![]);
        }
        self.scripts.clear();
        self.world.borrow_mut().script_objects.clear();
    }

    /// Stop the game: call `on_destroy()` everywhere and break reference cycles.
    pub fn shutdown(&mut self) {
        self.shutdown_scripts();
        self.timers.borrow_mut().list.clear();
    }

    /// Number of attached (not necessarily enabled) script instances.
    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }

    pub fn disabled_scripts(&self) -> usize {
        self.scripts.iter().filter(|s| s.disabled).count()
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        // Break Rc cycles: script objects -> entity handles -> world -> script objects.
        if let Ok(mut w) = self.world.try_borrow_mut() {
            w.script_objects.clear();
        }
        if let Ok(mut t) = self.timers.try_borrow_mut() {
            t.list.clear();
        }
    }
}

pub fn load_scene_file(project: &Project, rel: &str) -> Result<Scene, String> {
    let full = project.resolve(rel)?;
    let src = std::fs::read_to_string(&full).map_err(|e| format!("cannot read scene '{}': {}", rel, e))?;
    Scene::from_json_str(&src).map_err(|e| format!("{}: {}", rel, e))
}
