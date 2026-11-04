//! Hot reload for play mode. The watcher compares modification times of the
//! scene file, every script and every texture or model the scene uses, and
//! applies changes without restarting play:
//!
//! * A changed script is compiled again and its class replaced. Every running
//!   instance of it gets a fresh object with the old object's fields copied
//!   over, so counters, timers and other state survive. An instance that was
//!   disabled by an error restarts from `ready()`. A script that fails to
//!   compile keeps the last working version and logs the error.
//! * A changed helper module (anything in `scripts/` that is not an entity
//!   script) reloads every entity script, because scripts import helpers.
//! * A changed scene file is reloaded at the end of the frame. That resets
//!   entities to their saved state, since the scene itself changed.
//! * A changed texture or model is reported in [`Reloaded::assets`] so the
//!   host can drop its cached copy. Animated entities that use a changed model
//!   restart their clip.
//!
//! Checks are throttled to [`CHECK_INTERVAL`] seconds of game time. The first
//! call only records the current times.

use super::{Runtime, ScriptInstance};
use crate::bindings::EntityHandle;
use crate::world::LogLevel;
use grodot_core::scene::Component;
use groscript::Value;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;
use std::time::SystemTime;

/// Seconds of game time between file checks.
pub const CHECK_INTERVAL: f64 = 0.5;

/// What one check reloaded. Empty when nothing changed or no check was due.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reloaded {
    /// Scripts that were compiled again (project-relative paths).
    pub scripts: Vec<String>,
    /// Scripts that failed to compile and kept their last working version.
    pub failed: Vec<String>,
    /// Textures and models that changed. The host must drop cached copies.
    pub assets: Vec<String>,
    /// The scene file changed and will be reloaded at the end of this frame.
    pub scene: bool,
}

impl Reloaded {
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty() && self.failed.is_empty() && self.assets.is_empty() && !self.scene
    }
}

#[derive(Default)]
pub struct Watcher {
    stamps: HashMap<String, SystemTime>,
    next_check: f64,
}

impl Runtime {
    /// Check the watched files and apply any changes. Call once per frame in
    /// play mode, before [`Runtime::frame`]. Cheap between checks.
    pub fn hot_reload(&mut self) -> Reloaded {
        let now = self.world.borrow().time.elapsed;
        if now < self.watcher.next_check {
            return Reloaded::default();
        }
        self.watcher.next_check = now + CHECK_INTERVAL;
        let Some(root) = self.world.borrow().project_root.clone() else {
            return Reloaded::default();
        };
        let (scene_path, asset_refs) = {
            let w = self.world.borrow();
            (w.scene_path.clone(), scene_assets(&w.scene))
        };

        // Everything that can change: the scene, the entry scripts, the helper
        // modules in scripts/, and the scene's textures and models.
        let mut watched: BTreeSet<String> = BTreeSet::new();
        if let Some(p) = &scene_path {
            watched.insert(p.clone());
        }
        watched.extend(self.classes.keys().cloned());
        watched.extend(self.scripts.iter().map(|s| s.path.clone()));
        if let Ok(dir) = std::fs::read_dir(root.join("scripts")) {
            for entry in dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".gro") {
                    watched.insert(format!("scripts/{}", name));
                }
            }
        }
        watched.extend(asset_refs.iter().cloned());

        let mut changed: Vec<String> = Vec::new();
        for rel in &watched {
            let Ok(mtime) = std::fs::metadata(root.join(rel)).and_then(|m| m.modified()) else { continue };
            match self.watcher.stamps.insert(rel.clone(), mtime) {
                Some(old) if old != mtime => changed.push(rel.clone()),
                _ => {}
            }
        }

        let mut out = Reloaded::default();
        let mut script_changes: Vec<String> = Vec::new();
        for rel in changed {
            if Some(&rel) == scene_path.as_ref() {
                out.scene = true;
            } else if rel.ends_with(".gro") {
                script_changes.push(rel);
            } else if asset_refs.contains(&rel) {
                self.drop_model(&rel);
                out.assets.push(rel);
            }
        }

        // Entity scripts that changed reload themselves. A changed helper module is
        // run again first (the interpreter keeps imported modules), then every
        // entity script reloads, since any of them may import it.
        let entry_scripts: Vec<String> = self.classes.keys().cloned().collect();
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for rel in script_changes {
            if self.classes.contains_key(&rel) {
                targets.insert(rel);
            } else if self.run_module(&rel) {
                targets.extend(entry_scripts.iter().cloned());
            } else {
                out.failed.push(rel);
            }
        }
        for t in targets {
            if self.reload_script(&t) {
                out.scripts.push(t);
            } else {
                out.failed.push(t);
            }
        }

        if out.scene {
            self.log(LogLevel::Info, format!("Reloading scene {}", scene_path.clone().unwrap_or_default()));
            self.world.borrow_mut().pending_scene_load = scene_path;
        }
        for s in &out.scripts {
            self.log(LogLevel::Info, format!("Reloaded {}", s));
        }
        for f in &out.failed {
            self.log(LogLevel::Warning, format!("{} has errors; the previous version keeps running", f));
        }
        out
    }

    /// Run a helper module's file again so its `module` definitions are replaced.
    /// Returns false (after logging the error) if the file did not compile.
    fn run_module(&mut self, path: &str) -> bool {
        let full = match self.world.borrow().resolve(path) {
            Ok(p) => p,
            Err(e) => {
                self.log(LogLevel::Error, e);
                return false;
            }
        };
        let src = match std::fs::read_to_string(&full) {
            Ok(s) => s,
            Err(e) => {
                self.log(LogLevel::Error, format!("cannot read '{}': {}", path, e));
                return false;
            }
        };
        match self.interp.run_source(path, &src) {
            Ok(_) => true,
            Err(e) => {
                self.log_script_error(&e);
                false
            }
        }
    }

    /// Compile a script again and swap it into every running instance.
    /// Returns false (after logging the error) if the file did not compile.
    fn reload_script(&mut self, path: &str) -> bool {
        let full = match self.world.borrow().resolve(path) {
            Ok(p) => p,
            Err(e) => {
                self.log(LogLevel::Error, e);
                return false;
            }
        };
        let src = match std::fs::read_to_string(&full) {
            Ok(s) => s,
            Err(e) => {
                self.log(LogLevel::Error, format!("cannot read '{}': {}", path, e));
                return false;
            }
        };
        let stmts = match groscript::parser::parse(&src) {
            Ok(s) => s,
            Err(errs) => {
                let mut e = errs[0].clone();
                e.file = Some(path.into());
                e.source = Some(src.as_str().into());
                self.log_script_error(&e);
                return false;
            }
        };
        let class_names: Vec<String> = stmts
            .iter()
            .filter_map(|s| match &s.kind {
                groscript::ast::StmtKind::Class { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect();
        if let Err(e) = self.interp.run_source(path, &src) {
            self.log_script_error(&e);
            return false;
        }
        let class = class_names.iter().find_map(|n| match self.interp.get_global(n) {
            Some(Value::Class(c)) if c.is_subclass_of("Entity") => Some(c),
            _ => None,
        });
        let Some(class) = class else {
            self.log(LogLevel::Error, format!("{} must define a class that extends Entity", path));
            return false;
        };
        self.classes.insert(path.to_string(), Ok(class.clone()));

        for s in self.scripts.iter_mut().filter(|s| s.path == path) {
            swap_instance(&mut self.interp, &self.world, s, &class);
        }
        true
    }

    /// Forget a cached model so the next use loads the new file, and restart
    /// animated entities that use it.
    fn drop_model(&mut self, path: &str) {
        let ids: Vec<_> = {
            let mut w = self.world.borrow_mut();
            w.models.remove(path);
            w.animations.iter().filter(|(_, st)| st.model == path).map(|(id, _)| *id).collect()
        };
        for id in ids {
            let mut w = self.world.borrow_mut();
            w.animations.remove(&id);
            crate::animation::attach(&mut w, id);
        }
    }
}

/// Replace one script instance's object with a fresh one of `class`, keeping its fields.
fn swap_instance(
    interp: &mut groscript::Interpreter,
    world: &crate::bindings::WorldRef,
    s: &mut ScriptInstance,
    class: &Rc<groscript::Class>,
) {
    let old_fields = match &s.object {
        Value::Object(o) => o.fields.borrow().clone(),
        _ => Default::default(),
    };
    let handle = Rc::new(EntityHandle { id: s.entity, world: world.clone() });
    let fresh = interp.new_object(class, Some(handle));
    let was_disabled = s.disabled;
    if !was_disabled {
        if let Value::Object(o) = &fresh {
            o.fields.borrow_mut().extend(old_fields);
        }
    }
    world.borrow_mut().script_objects.insert(s.entity, fresh.clone());
    s.object = fresh;
    if was_disabled {
        // The old object failed part way through; let ready() set it up again.
        s.disabled = false;
        s.ready = false;
    }
}

/// Textures and models the scene refers to (project-relative paths).
fn scene_assets(scene: &grodot_core::scene::Scene) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in &scene.entities {
        if let Some(Component::MeshRenderer(m)) = e.get("MeshRenderer") {
            if grodot_core::gltf::is_model_path(&m.mesh) || m.mesh.to_lowercase().ends_with(".obj") {
                out.insert(m.mesh.clone());
            }
            for t in [&m.material.texture, &m.material.emissive_texture].into_iter().flatten() {
                out.insert(t.clone());
            }
        }
    }
    out
}
