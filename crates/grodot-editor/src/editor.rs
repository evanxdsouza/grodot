//! The Grodot editor: viewport, hierarchy, inspector, assets, console, Play/Stop.

use crate::hud;
use crate::ui::{fmt_f32, id_of, theme::*, InputFrame, PopupResult, Ui};
use grodot_core::config::Project;
use grodot_core::math::{vec3, Color, Mat4, Quat, Vec3};
use grodot_core::scene::{Component, EntityId, FieldValue, Scene, COMPONENT_TYPES, PRIMITIVE_MESHES};
use grodot_platform::Audio;
use grodot_render::canvas::Rect;
use grodot_render::font::FontId;
use grodot_render::raster::{self, View};
use grodot_render::scene_render::{view_from_matrix, RenderOptions, SceneRenderer};
use grodot_runtime::{AudioCmd, Log, LogEntry, LogLevel, Runtime};
use std::path::Path;

pub const TOOLBAR_H: i32 = 44;
pub const STATUS_H: i32 = 24;
pub const LEFT_W: i32 = 250;
pub const RIGHT_W: i32 = 340;
pub const BOTTOM_H: i32 = 220;
const ROW_H: i32 = 24;

#[derive(Debug, Clone, Copy)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        OrbitCamera { target: vec3(0.0, 0.5, 0.0), yaw: -30.0, pitch: -28.0, distance: 22.0 }
    }
}

impl OrbitCamera {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler_deg(vec3(self.pitch, self.yaw, 0.0))
    }
    pub fn view(&self, viewport: Rect) -> View {
        let q = self.rotation();
        let back = q.rotate(Vec3::Z);
        let eye = self.target + back * self.distance;
        let world = Mat4::from_trs(eye, q, Vec3::ONE);
        view_from_matrix(&world, 60.0, 0.1, 1000.0, viewport)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    Console,
    Assets,
}

#[derive(Debug, Clone, PartialEq)]
enum Popup {
    None,
    AddEntity,
    AddComponent,
    AssetPicker { component: String, field: String, options: Vec<String> },
}

#[derive(Debug, Clone)]
pub enum Action {
    Save,
    TogglePlay,
    Undo,
    Redo,
    CreateEntity(String),
    Delete,
    Duplicate,
    Focus,
    OpenScene(String),
    OpenExternal(String, u32),
    NewScript(EntityId),
    PlayClip(String),
    /// Create an entity showing a model (or mesh) at a viewport pixel.
    PlaceModel(String, (f32, f32)),
    SpawnPrefab(String),
}

struct Gizmo {
    axis: usize,
    start_local: Vec3,
    start_mouse: (f32, f32),
}

pub struct Editor {
    pub project: Project,
    pub scene: Scene,
    pub scene_path: String,
    pub dirty: bool,
    pub selection: Option<EntityId>,
    pub camera: OrbitCamera,
    renderer: SceneRenderer,
    pub ui: Ui,
    pub play: Option<Runtime>,
    pub log: Log,
    pub audio: Option<Audio>,
    undo: Vec<Scene>,
    redo: Vec<Scene>,
    last_stable: Scene,
    assets: Vec<String>,
    asset_scan_timer: f32,
    pub bottom_tab: BottomTab,
    scroll_hierarchy: i32,
    scroll_inspector: i32,
    scroll_console: i32,
    scroll_assets: i32,
    console_follow: bool,
    rename: Option<EntityId>,
    popup: Popup,
    popup_pos: (i32, i32),
    status: Option<(String, f32)>,
    pub show_grid: bool,
    pub show_colliders: bool,
    gizmo: Option<Gizmo>,
    drag_entity: Option<(EntityId, (f32, f32))>,
    drag_asset: Option<(String, (f32, f32))>,
    asset_selected: Option<String>,
    fps: f32,
    close_requested_at: Option<f32>,
    time: f32,
    pub quit: bool,
}

impl Editor {
    pub fn open(project: Project) -> Result<Editor, String> {
        let scene_path = project.config.main_scene.clone();
        let scene = match grodot_runtime::load_scene_file(&project, &scene_path) {
            Ok(s) => s,
            Err(e) => {
                if project.resolve(&scene_path).map(|p| p.exists()).unwrap_or(false) {
                    return Err(e);
                }
                Scene::new("Main")
            }
        };
        let mut log = Log::default();
        log.info(format!("Opened project '{}'", project.config.name));
        let mut ed = Editor {
            renderer: SceneRenderer::new(Some(project.root.clone())),
            last_stable: scene.clone(),
            scene,
            scene_path,
            dirty: false,
            selection: None,
            camera: OrbitCamera::default(),
            ui: Ui::new(1440, 900),
            play: None,
            log,
            audio: None,
            undo: Vec::new(),
            redo: Vec::new(),
            assets: Vec::new(),
            asset_scan_timer: 0.0,
            bottom_tab: BottomTab::Console,
            scroll_hierarchy: 0,
            scroll_inspector: 0,
            scroll_console: 0,
            scroll_assets: 0,
            console_follow: true,
            rename: None,
            popup: Popup::None,
            popup_pos: (0, 0),
            status: None,
            show_grid: true,
            show_colliders: false,
            gizmo: None,
            drag_entity: None,
            drag_asset: None,
            asset_selected: None,
            fps: 60.0,
            close_requested_at: None,
            time: 0.0,
            project,
            quit: false,
        };
        ed.scan_assets();
        ed.validate_scene();
        Ok(ed)
    }

    pub fn title(&self) -> String {
        format!(
            "{}{} - {} - Grodot Editor{}",
            self.scene_path,
            if self.dirty { " *" } else { "" },
            self.project.config.name,
            if self.play.is_some() { " [Playing]" } else { "" }
        )
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        self.ui.canvas.resize(w as usize, h as usize);
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), 4.0));
    }

    /// Called when the window close button is pressed. Returns true if the editor may close.
    pub fn request_close(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        if let Some(t) = self.close_requested_at {
            if self.time - t < 4.0 {
                return true;
            }
        }
        self.close_requested_at = Some(self.time);
        self.set_status("Unsaved changes! Press Ctrl+S to save, or close again to discard them.");
        false
    }

    fn validate_scene(&mut self) {
        // Report missing scripts and assets up front so they are visible, not silent.
        for e in &self.scene.entities {
            if let Some(s) = e.script() {
                if !s.path.is_empty() && !self.project.resolve(&s.path).map(|p| p.is_file()).unwrap_or(false) {
                    self.log.warn(format!("'{}': script '{}' does not exist", e.name, s.path));
                }
            }
            if let Some(t) = e.mesh().and_then(|m| m.material.texture.as_ref()) {
                if !self.project.resolve(t).map(|p| p.is_file()).unwrap_or(false) {
                    self.log.warn(format!("'{}': texture '{}' does not exist", e.name, t));
                }
            }
            if let Some(Component::AudioSource(a)) = e.get("AudioSource") {
                if !a.clip.is_empty() && !self.project.resolve(&a.clip).map(|p| p.is_file()).unwrap_or(false) {
                    self.log.warn(format!("'{}': audio clip '{}' does not exist", e.name, a.clip));
                }
            }
        }
    }

    pub fn scan_assets(&mut self) {
        let mut out = Vec::new();
        fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, depth: usize) {
            if depth > 6 {
                return;
            }
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name == "builds" || name == "target" {
                    continue;
                }
                if p.is_dir() {
                    walk(&p, root, out, depth + 1);
                } else if let Ok(rel) = p.strip_prefix(root) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        walk(&self.project.root, &self.project.root, &mut out, 0);
        self.assets = out;
    }

    // ------------------------------------------------------------------ frame

    pub fn frame(&mut self, input: InputFrame, dt: f32) {
        self.time += dt;
        if dt > 0.0 {
            self.fps = self.fps * 0.95 + (1.0 / dt) * 0.05;
        }
        self.asset_scan_timer += dt;
        if self.asset_scan_timer > 2.0 {
            self.asset_scan_timer = 0.0;
            self.scan_assets();
        }
        if let Some((_, t)) = &mut self.status {
            *t -= dt;
            if *t <= 0.0 {
                self.status = None;
            }
        }
        self.ui.begin_frame(input, dt);
        self.ui.modal = None;
        let vp_now = self.layout().3;
        self.ui.hovered_raw_viewport = vp_now.contains(self.ui.input.mouse.0 as i32, self.ui.input.mouse.1 as i32);
        let mut actions: Vec<Action> = Vec::new();

        // Keyboard shortcuts
        if !self.ui.has_text_focus() {
            let ctrl = self.ui.input.ctrl;
            if ctrl && self.ui.key("S") {
                actions.push(Action::Save);
            }
            if ctrl && self.ui.key("Z") {
                actions.push(if self.ui.input.shift { Action::Redo } else { Action::Undo });
            }
            if ctrl && self.ui.key("Y") {
                actions.push(Action::Redo);
            }
            if ctrl && self.ui.key("D") {
                actions.push(Action::Duplicate);
            }
            if self.ui.key("F5") || (ctrl && self.ui.key("P")) {
                actions.push(Action::TogglePlay);
            }
            if self.play.is_some() && self.ui.key("Escape") && self.popup == Popup::None {
                actions.push(Action::TogglePlay);
            }
            if self.play.is_none() {
                if self.ui.key("Delete") {
                    actions.push(Action::Delete);
                }
                if self.ui.key("F") && !ctrl {
                    actions.push(Action::Focus);
                }
            }
        }

        // Game input and simulation
        if let Some(rt) = &mut self.play {
            {
                let mut w = rt.world.borrow_mut();
                if !self.ui.has_text_focus() {
                    for k in &self.ui.input.keys {
                        w.input.press(k);
                    }
                }
                if self.ui.hovered_raw_viewport {
                    for (down, key) in
                        [(self.ui.input.left_pressed, "MouseLeft"), (self.ui.input.right_pressed, "MouseRight")]
                    {
                        if down {
                            w.input.press(key);
                        }
                    }
                }
                w.input.mouse_position = self.ui.input.mouse;
                w.input.mouse_delta = self.ui.input.mouse_delta;
            }
            rt.frame(dt);
            let cmds: Vec<AudioCmd> = std::mem::take(&mut rt.world.borrow_mut().audio_cmds);
            run_audio(&mut self.audio, cmds, &mut self.log);
            let entries: Vec<LogEntry> = std::mem::take(&mut rt.world.borrow_mut().log.entries);
            for e in entries {
                push_entry(&mut self.log, e);
            }
            if rt.world.borrow().quit_requested {
                actions.push(Action::TogglePlay);
            }
        }

        let (w, h) = (self.ui.canvas.width as i32, self.ui.canvas.height as i32);
        self.ui.canvas.clear(BG);
        let (left, right, bottom, viewport) = self.layout();

        // Popups are drawn last but must block input underneath: register their area first.
        if self.popup != Popup::None {
            self.ui.modal = Some(Rect::new(self.popup_pos.0, self.popup_pos.1, 300, 400));
        }

        self.draw_viewport(viewport, &mut actions);
        self.draw_hierarchy(left, &mut actions);
        self.draw_inspector(right, &mut actions);
        self.draw_bottom(bottom, &mut actions);
        self.draw_toolbar(Rect::new(0, 0, w, TOOLBAR_H), &mut actions);
        self.draw_status(Rect::new(0, h - STATUS_H, w, STATUS_H));
        self.draw_popup(&mut actions);
        // Dragging a model from the asset browser into the viewport places it there.
        if let Some((path, start)) = self.drag_asset.clone() {
            let (mx, my) = self.ui.input.mouse;
            let moved = (mx - start.0).abs() + (my - start.1).abs() > 6.0;
            if moved && self.ui.input.left_down {
                let label = format!("place {}", path.rsplit('/').next().unwrap_or(&path));
                self.ui.canvas.text_shadow(mx as i32 + 12, my as i32 + 4, &label, 0xffffff, FontId::Ui);
            }
            if !self.ui.input.left_down {
                if moved && viewport.contains(mx as i32, my as i32) && self.play.is_none() {
                    actions.push(Action::PlaceModel(path, (mx, my)));
                }
                self.drag_asset = None;
            }
        }

        // Release game keys after the frame so just_pressed/just_released work.
        if let Some(rt) = &self.play {
            let mut wld = rt.world.borrow_mut();
            for k in &self.ui.input.keys_up {
                wld.input.release(k);
            }
            if self.ui.input.left_released {
                wld.input.release("MouseLeft");
            }
            if !self.ui.input.right_down {
                wld.input.release("MouseRight");
            }
            if self.ui.input.focus_lost {
                wld.input.release_all();
            }
        }
        self.ui.end_frame();

        for a in actions {
            self.apply(a);
        }

        // Undo snapshots: when nothing is being dragged and the scene changed.
        if self.play.is_none() && self.ui.active.is_none() && self.gizmo.is_none() && self.scene != self.last_stable {
            self.undo.push(std::mem::replace(&mut self.last_stable, self.scene.clone()));
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.dirty = true;
        }
    }

    /// Panel rectangles: (hierarchy, inspector, bottom dock, viewport).
    pub fn layout(&self) -> (Rect, Rect, Rect, Rect) {
        let (w, h) = (self.ui.canvas.width as i32, self.ui.canvas.height as i32);
        let body_y = TOOLBAR_H;
        let body_h = h - TOOLBAR_H - STATUS_H;
        let left = Rect::new(0, body_y, LEFT_W, body_h);
        let right = Rect::new(w - RIGHT_W, body_y, RIGHT_W, body_h);
        let bottom = Rect::new(LEFT_W, body_y + body_h - BOTTOM_H, w - LEFT_W - RIGHT_W, BOTTOM_H);
        let viewport = Rect::new(LEFT_W, body_y, w - LEFT_W - RIGHT_W, body_h - BOTTOM_H);
        (left, right, bottom, viewport)
    }

    pub fn apply(&mut self, a: Action) {
        match a {
            Action::Save => self.save(),
            Action::TogglePlay => self.toggle_play(),
            Action::Undo => {
                if self.play.is_none() {
                    if let Some(prev) = self.undo.pop() {
                        self.redo.push(std::mem::replace(&mut self.scene, prev));
                        self.last_stable = self.scene.clone();
                        self.dirty = true;
                        self.fix_selection();
                        self.set_status("Undo");
                    }
                }
            }
            Action::Redo => {
                if self.play.is_none() {
                    if let Some(next) = self.redo.pop() {
                        self.undo.push(std::mem::replace(&mut self.scene, next));
                        self.last_stable = self.scene.clone();
                        self.dirty = true;
                        self.fix_selection();
                        self.set_status("Redo");
                    }
                }
            }
            Action::CreateEntity(kind) => {
                if self.play.is_some() {
                    return self.set_status("Stop the game to edit the scene");
                }
                let id = create_entity(&mut self.scene, &kind, self.camera.target);
                self.selection = Some(id);
            }
            Action::Delete => {
                if let (Some(sel), None) = (self.selection, &self.play) {
                    let name = self.scene.entity(sel).map(|e| e.name.clone()).unwrap_or_default();
                    self.scene.remove_entity(sel);
                    self.selection = None;
                    self.set_status(format!("Deleted '{}'", name));
                }
            }
            Action::Duplicate => {
                if let (Some(sel), None) = (self.selection, &self.play) {
                    let src = self.scene.clone();
                    let parent = src.entity(sel).and_then(|e| e.parent);
                    if let Some(id) = self.scene.instantiate(&src, sel, parent) {
                        let e = self.scene.entity_mut(id).unwrap();
                        e.name = format!("{} copy", e.name);
                        e.transform_mut().position += vec3(1.0, 0.0, 0.0);
                        self.selection = Some(id);
                    }
                }
            }
            Action::Focus => {
                if let Some(sel) = self.selection {
                    let m = self.scene.world_matrix(sel);
                    self.camera.target = m.get_translation();
                    let s = m.transform_vector(Vec3::ONE).length();
                    self.camera.distance = (s * 3.0).clamp(3.0, 60.0);
                }
            }
            Action::OpenScene(path) => {
                if self.play.is_some() {
                    return self.set_status("Stop the game before opening another scene");
                }
                if self.dirty && self.close_requested_at.map(|t| self.time - t > 4.0).unwrap_or(true) {
                    self.close_requested_at = Some(self.time);
                    return self.set_status("Unsaved changes! Save first, or click Open again to discard them.");
                }
                match grodot_runtime::load_scene_file(&self.project, &path) {
                    Ok(s) => {
                        self.scene = s;
                        self.scene_path = path.clone();
                        self.last_stable = self.scene.clone();
                        self.undo.clear();
                        self.redo.clear();
                        self.dirty = false;
                        self.selection = None;
                        self.close_requested_at = None;
                        self.log.info(format!("Opened scene {}", path));
                        self.validate_scene();
                    }
                    Err(e) => self.log.error(e),
                }
            }
            Action::OpenExternal(path, line) => open_external(&self.project, &path, line, &mut self.log),
            Action::NewScript(id) => self.new_script(id),
            Action::PlaceModel(path, (px, py)) => {
                if self.play.is_some() {
                    return self.set_status("Stop the game to edit the scene");
                }
                let vp = self.layout().3;
                let view = self.camera.view(vp);
                let (o, d) = raster::screen_ray(&view, vp, px, py);
                // Drop onto the ground plane (y = 0), or in front of the camera when looking up.
                let at = if d.y < -1e-4 { o + d * (-o.y / d.y) } else { self.camera.target };
                let name: String = path
                    .rsplit('/')
                    .next()
                    .unwrap_or("Model")
                    .split('.')
                    .next()
                    .unwrap_or("Model")
                    .split(['_', '-'])
                    .map(|w| {
                        let mut c = w.chars();
                        c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let id = create_entity(&mut self.scene, "Empty", at);
                let e = self.scene.entity_mut(id).unwrap();
                e.name = name;
                e.transform_mut().position =
                    Vec3 { x: (at.x * 4.0).round() / 4.0, y: 0.0, z: (at.z * 4.0).round() / 4.0 };
                let mut mr = Component::new_default("MeshRenderer").unwrap();
                let _ = mr.set_field("mesh", FieldValue::Text(path.clone()));
                let _ = mr.set_field("base_color", FieldValue::Color(Color::WHITE));
                e.add(mr);
                self.selection = Some(id);
                self.set_status(format!("Placed {}", path));
            }
            Action::PlayClip(path) => {
                if let Ok(full) = self.project.resolve(&path) {
                    run_audio(&mut self.audio, vec![AudioCmd::Play { path: full, volume: 1.0 }], &mut self.log);
                    if self.audio.is_none() {
                        self.set_status("Audio output is not available on this system");
                    }
                }
            }
            Action::SpawnPrefab(path) => {
                if self.play.is_some() {
                    return;
                }
                let r = self
                    .project
                    .resolve(&path)
                    .and_then(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()))
                    .and_then(|s| Scene::from_json_str(&s));
                match r {
                    Ok(prefab) => {
                        let roots: Vec<EntityId> =
                            prefab.entities.iter().filter(|e| e.parent.is_none()).map(|e| e.id).collect();
                        for root in roots {
                            if let Some(id) = self.scene.instantiate(&prefab, root, None) {
                                self.selection = Some(id);
                            }
                        }
                    }
                    Err(e) => self.log.error(format!("cannot spawn {}: {}", path, e)),
                }
            }
        }
    }

    fn fix_selection(&mut self) {
        if let Some(s) = self.selection {
            if self.scene.entity(s).is_none() {
                self.selection = None;
            }
        }
    }

    pub fn save(&mut self) {
        if self.play.is_some() {
            return self.set_status("Stop the game before saving (Play mode changes are not saved)");
        }
        let path = match self.project.resolve(&self.scene_path) {
            Ok(p) => p,
            Err(e) => return self.log.error(e),
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&path, self.scene.to_string_pretty()) {
            Ok(()) => {
                self.dirty = false;
                self.close_requested_at = None;
                self.log.info(format!("Saved {}", self.scene_path));
                self.set_status(format!("Saved {}", self.scene_path));
            }
            Err(e) => self.log.error(format!("could not save {}: {}", self.scene_path, e)),
        }
    }

    pub fn toggle_play(&mut self) {
        if let Some(mut rt) = self.play.take() {
            rt.shutdown();
            let entries: Vec<LogEntry> = std::mem::take(&mut rt.world.borrow_mut().log.entries);
            for e in entries {
                push_entry(&mut self.log, e);
            }
            if let Some(a) = &mut self.audio {
                a.stop_music();
            }
            self.log.info("Stopped. The editor scene is unchanged.");
            self.fix_selection();
        } else {
            self.ui.focus = None;
            let mut rt =
                Runtime::new(self.scene.clone(), Some(self.project.root.clone()), self.project.config.input.clone());
            rt.world.borrow_mut().scene_path = Some(self.scene_path.clone());
            self.log.info("Playing... (press Esc or Stop to return to the editor)");
            rt.start();
            self.play = Some(rt);
            self.bottom_tab = BottomTab::Console;
        }
    }

    fn new_script(&mut self, id: EntityId) {
        let Some(e) = self.scene.entity(id) else { return };
        let class: String = e
            .name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut c = s.chars();
                c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
            })
            .collect();
        let class = if class.is_empty() || class.chars().next().unwrap().is_ascii_digit() {
            format!("Entity{}", id)
        } else {
            class
        };
        let file: String = to_snake(&class);
        let mut rel = format!("scripts/{}.gro", file);
        let mut n = 2;
        while self.project.resolve(&rel).map(|p| p.exists()).unwrap_or(false) {
            rel = format!("scripts/{}_{}.gro", file, n);
            n += 1;
        }
        let src = format!(
            "# Script for '{name}'.\nclass {class} < Entity\n  function ready()\n    print(\"{name} is ready\")\n  end\n\n  function update(delta)\n  end\nend\n",
            name = e.name,
            class = class
        );
        let full = match self.project.resolve(&rel) {
            Ok(p) => p,
            Err(e) => return self.log.error(e),
        };
        let _ = std::fs::create_dir_all(full.parent().unwrap());
        if let Err(err) = std::fs::write(&full, src) {
            return self.log.error(format!("could not create {}: {}", rel, err));
        }
        let ent = self.scene.entity_mut(id).unwrap();
        if !ent.has("Script") {
            ent.add(Component::new_default("Script").unwrap());
        }
        if let Some(c) = ent.get_mut("Script") {
            let _ = c.set_field("path", FieldValue::Text(rel.clone()));
        }
        self.log.info(format!("Created {}", rel));
        self.scan_assets();
    }

    // ------------------------------------------------------------------ panels

    fn draw_toolbar(&mut self, r: Rect, actions: &mut Vec<Action>) {
        self.ui.canvas.vgradient(r, 0x2c2e33, 0x24262a);
        self.ui.canvas.fill_rect(Rect::new(0, r.bottom() - 1, r.w, 1), BORDER);
        // logo
        let lx = 12;
        self.ui.canvas.fill_circle(lx as f32 + 11.0, 22.0, 11.0, ACCENT, 255);
        self.ui.canvas.fill_circle(lx as f32 + 11.0, 22.0, 5.0, 0x1b1c1f, 255);
        self.ui.canvas.fill_rect(Rect::new(lx + 11, 20, 11, 4), 0x1b1c1f);
        self.ui.canvas.text(lx + 30, 13, "Grodot", TEXT, FontId::UiBold);
        let mut x = 120;
        let by = 8;
        let bh = 28;
        let btn = |ui: &mut Ui, x: &mut i32, w: i32, label: &str| -> bool {
            let rr = Rect::new(*x, by, w, bh);
            *x += w + 6;
            ui.button(rr, label)
        };
        if btn(&mut self.ui, &mut x, 64, "Save") {
            actions.push(Action::Save);
        }
        if btn(&mut self.ui, &mut x, 56, "Undo") {
            actions.push(Action::Undo);
        }
        if btn(&mut self.ui, &mut x, 56, "Redo") {
            actions.push(Action::Redo);
        }
        x += 10;
        if btn(&mut self.ui, &mut x, 92, "+ Entity") {
            self.popup = Popup::AddEntity;
            self.popup_pos = (x - 98, TOOLBAR_H);
        }
        if btn(&mut self.ui, &mut x, 84, "Duplicate") {
            actions.push(Action::Duplicate);
        }
        if btn(&mut self.ui, &mut x, 70, "Delete") {
            actions.push(Action::Delete);
        }
        // Play / Stop centred
        let pw = 110;
        let px = (r.w - pw) / 2;
        let play_rect = Rect::new(px.max(x + 10), by, pw, bh);
        if self.play.is_some() {
            if self.ui.button_colored(play_rect, "Stop  (Esc)", RED) {
                actions.push(Action::TogglePlay);
            }
        } else if self.ui.button_colored(play_rect, "Play  (F5)", 0x2f8f4e) {
            actions.push(Action::TogglePlay);
        }
        // Toggles and title on the right
        let mut rx = r.w - 12;
        let mut rbtn = |ui: &mut Ui, w: i32, label: &str, on: bool| -> bool {
            rx -= w;
            let rr = Rect::new(rx, by, w, bh);
            rx -= 6;
            ui.toggle(rr, label, on)
        };
        if rbtn(&mut self.ui, 82, "Colliders", self.show_colliders) {
            self.show_colliders = !self.show_colliders;
        }
        if rbtn(&mut self.ui, 50, "Grid", self.show_grid) {
            self.show_grid = !self.show_grid;
        }
        let title =
            format!("{}  /  {}{}", self.project.config.name, self.scene_path, if self.dirty { " *" } else { "" });
        let tw = self.ui.canvas.measure(&title, FontId::Ui);
        let tx = rx - tw - 16;
        if tx > play_rect.right() + 10 {
            self.ui.canvas.text(tx, 14, &title, TEXT_DIM, FontId::Ui);
        }
    }

    fn draw_status(&mut self, r: Rect) {
        self.ui.canvas.fill_rect(r, 0x1f2023);
        self.ui.canvas.fill_rect(Rect::new(0, r.y, r.w, 1), BORDER);
        let msg = match &self.status {
            Some((s, _)) => s.clone(),
            None if self.play.is_some() => "Playing - keyboard input goes to the game. Esc stops.".into(),
            None => "Right-drag: orbit   Middle-drag / Shift+right-drag: pan   Wheel: zoom   F: focus   Ctrl+S: save   F5: play".into(),
        };
        let color = if self.status.is_some() { YELLOW } else { TEXT_DIM };
        self.ui.canvas.text(r.x + 10, r.y + 4, &msg, color, FontId::Ui);
        let count = match &self.play {
            Some(rt) => rt.world.borrow().scene.entities.len(),
            None => self.scene.entities.len(),
        };
        let right = format!("{} entities   {} fps   Grodot {}", count, self.fps.round(), grodot_core::VERSION);
        let w = self.ui.canvas.measure(&right, FontId::Ui);
        self.ui.canvas.text(r.right() - w - 10, r.y + 4, &right, TEXT_DIM, FontId::Ui);
    }

    fn draw_viewport(&mut self, vp: Rect, actions: &mut Vec<Action>) {
        if vp.w < 10 || vp.h < 10 {
            return;
        }
        let hovered = self.ui.hovered(vp) && self.ui.active.is_none() || (self.ui.active == Some(id_of(&["viewport"])));
        let mouse = self.ui.input.mouse;
        if let Some(rt) = &self.play {
            let w = rt.world.borrow();
            let view = SceneRenderer::camera_view(&w.scene, vp).unwrap_or_else(|| self.camera.view(vp));
            let opts = RenderOptions { show_colliders: self.show_colliders, ..Default::default() };
            self.renderer.render(&mut self.ui.canvas, vp, &w.scene, &view, &opts);
            hud::draw_debug_lines(&mut self.ui.canvas, vp, &view, &w);
            hud::draw_hud(&mut self.ui.canvas, vp, &w);
            if SceneRenderer::camera_view(&w.scene, vp).is_none() {
                self.ui.canvas.text_shadow(
                    vp.x + 12,
                    vp.bottom() - 52,
                    "No active Camera in the scene - showing the editor view.",
                    YELLOW,
                    FontId::Ui,
                );
            }
            // playing border
            self.ui.canvas.stroke_rect(vp, 0x2f8f4e);
            self.ui.canvas.stroke_rect(vp.shrink(1), 0x2f8f4e);
        } else {
            // Camera controls
            if hovered {
                if self.ui.input.wheel != 0.0 {
                    self.camera.distance =
                        (self.camera.distance * (1.0 - self.ui.input.wheel * 0.12)).clamp(0.5, 500.0);
                }
                let (dx, dy) = self.ui.input.mouse_delta;
                let pan = self.ui.input.middle_down || (self.ui.input.right_down && self.ui.input.shift);
                if pan {
                    let q = self.camera.rotation();
                    let s = self.camera.distance * 0.0016;
                    self.camera.target += q.rotate(Vec3::X) * (-dx * s) + q.rotate(Vec3::Y) * (dy * s);
                } else if self.ui.input.right_down {
                    self.camera.yaw -= dx * 0.3;
                    self.camera.pitch = (self.camera.pitch - dy * 0.3).clamp(-89.0, 89.0);
                }
            }
            let view = self.camera.view(vp);
            let opts = RenderOptions {
                grid: self.show_grid,
                selection: self.selection,
                show_colliders: self.show_colliders,
                show_camera_icons: true,
            };
            self.renderer.render(&mut self.ui.canvas, vp, &self.scene, &view, &opts);
            let gizmo_used = self.gizmo_ui(vp, &view);
            if !gizmo_used && self.ui.clicked(vp) && self.popup == Popup::None {
                self.selection = self.renderer.pick(&self.scene, &view, vp, mouse.0, mouse.1);
                self.rename = None;
            }
            if let Some(sel) = self.selection {
                if let Some(e) = self.scene.entity(sel) {
                    let label = e.name.clone();
                    self.ui.canvas.text_shadow(vp.x + 12, vp.y + 10, &label, 0xffffff, FontId::UiBold);
                }
            }
            if self.scene.entities.is_empty() {
                let r = Rect::new(vp.x, vp.y + vp.h / 2 - 30, vp.w, 60);
                self.ui.label_centered(
                    r,
                    "Empty scene - click \"+ Entity\" to add a cube, camera or light.",
                    TEXT,
                    FontId::UiBold,
                );
            }
        }
        for w in self.renderer.warnings.drain(..) {
            self.log.warn(w);
        }
        let _ = actions;
    }

    /// Translate gizmo. Returns true if it consumed the mouse this frame.
    fn gizmo_ui(&mut self, vp: Rect, view: &View) -> bool {
        let Some(sel) = self.selection else {
            self.gizmo = None;
            return false;
        };
        let Some(ent) = self.scene.entity(sel) else { return false };
        let world = self.scene.world_matrix(sel);
        let origin = world.get_translation();
        let len = ((origin - view.eye).length() * 0.12).max(0.2);
        let Some(o2) = raster::project(view, vp, origin) else { return false };
        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
        let colors = [AXIS_X, AXIS_Y, AXIS_Z];
        let mouse = self.ui.input.mouse;
        let mut hover_axis = None;
        let mut ends = [(0.0, 0.0); 3];
        for i in 0..3 {
            let Some(e2) = raster::project(view, vp, origin + axes[i] * len) else { continue };
            ends[i] = e2;
            let d = dist_to_segment(mouse, o2, e2);
            if d < 7.0 && hover_axis.is_none() {
                hover_axis = Some(i);
            }
        }
        // Begin drag
        if self.gizmo.is_none() && self.ui.input.left_pressed && self.ui.hovered(vp) {
            if let Some(axis) = hover_axis {
                self.gizmo = Some(Gizmo { axis, start_local: ent.transform().position, start_mouse: mouse });
                self.ui.active = Some(id_of(&["gizmo"]));
            }
        }
        let mut used = false;
        if let Some(g) = &self.gizmo {
            used = true;
            let axis = g.axis;
            let s = (ends[axis].0 - o2.0, ends[axis].1 - o2.1);
            let slen = (s.0 * s.0 + s.1 * s.1).sqrt().max(1e-3);
            let dir = (s.0 / slen, s.1 / slen);
            let md = (mouse.0 - g.start_mouse.0, mouse.1 - g.start_mouse.1);
            let mut units = (md.0 * dir.0 + md.1 * dir.1) / slen * len;
            if self.ui.input.ctrl {
                units = (units * 4.0).round() / 4.0;
            }
            let parent_inv = ent.parent.map(|p| self.scene.world_matrix(p).inverse());
            let world_delta = axes[axis] * units;
            let local_delta = match parent_inv {
                Some(inv) => inv.transform_vector(world_delta),
                None => world_delta,
            };
            let new_pos = g.start_local + local_delta;
            if let Some(e) = self.scene.entity_mut(sel) {
                e.transform_mut().position = new_pos;
            }
            if !self.ui.input.left_down {
                self.gizmo = None;
                self.ui.active = None;
            }
        }
        // Draw
        for i in 0..3 {
            let active = self.gizmo.as_ref().map(|g| g.axis == i).unwrap_or(false) || hover_axis == Some(i);
            let c = if active { 0xffffff } else { colors[i] };
            let (a, b) = (o2, ends[i]);
            for off in [-1.0, 0.0, 1.0] {
                self.ui.canvas.line(a.0 + off, a.1, b.0 + off, b.1, c);
                self.ui.canvas.line(a.0, a.1 + off, b.0, b.1 + off, c);
            }
            self.ui.canvas.fill_circle(b.0, b.1, 5.0, c, 255);
        }
        self.ui.canvas.fill_circle(o2.0, o2.1, 3.5, 0xffffff, 255);
        used || hover_axis.is_some() && self.ui.input.left_pressed
    }

    fn draw_hierarchy(&mut self, r: Rect, actions: &mut Vec<Action>) {
        self.ui.panel(r, PANEL);
        self.ui.canvas.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), BORDER);
        let hr = Rect::new(r.x, r.y, r.w - 1, 30);
        self.ui.header(hr, if self.play.is_some() { "Hierarchy (live)" } else { "Hierarchy" });
        let add = Rect::new(hr.right() - 30, hr.y + 4, 24, 22);
        if self.ui.button(add, "+") {
            self.popup = Popup::AddEntity;
            self.popup_pos = (add.x, add.bottom());
        }
        let list = Rect::new(r.x, r.y + 30, r.w - 1, r.h - 30);
        if self.ui.hovered(list) && self.ui.input.wheel != 0.0 {
            self.scroll_hierarchy = (self.scroll_hierarchy - (self.ui.input.wheel * 48.0) as i32).max(0);
        }
        // Build rows (tree order)
        let playing = self.play.is_some();
        let world_ref = self.play.as_ref().map(|rt| rt.world.clone());
        let mut wb = world_ref.as_ref().map(|w| w.borrow_mut());
        let scene: &mut Scene = match wb.as_mut() {
            Some(w) => &mut w.scene,
            None => &mut self.scene,
        };
        let mut rows: Vec<(EntityId, i32)> = Vec::new();
        fn visit(scene: &Scene, id: EntityId, depth: i32, rows: &mut Vec<(EntityId, i32)>) {
            rows.push((id, depth));
            for c in scene.children(id) {
                visit(scene, c, depth + 1, rows);
            }
        }
        for e in scene.entities.iter().filter(|e| e.parent.is_none()) {
            visit(scene, e.id, 0, &mut rows);
        }
        let max_scroll = (rows.len() as i32 * ROW_H - list.h + 8).max(0);
        self.scroll_hierarchy = self.scroll_hierarchy.min(max_scroll);
        let old = self.ui.canvas.clip;
        self.ui.canvas.set_clip(list);
        let mut y = list.y + 4 - self.scroll_hierarchy;
        let mut drop_target: Option<Option<EntityId>> = None;
        for (id, depth) in rows {
            let rr = Rect::new(list.x + 2, y, list.w - 4, ROW_H);
            y += ROW_H;
            if rr.bottom() < list.y || rr.y > list.bottom() {
                continue;
            }
            let e = scene.entity(id).unwrap();
            let icon = if e.has("Camera") || e.has("Camera2D") {
                (0xd0d4dc, "C")
            } else if e.has("Light") {
                (YELLOW, "L")
            } else if e.has("Script") {
                (GREEN, "S")
            } else if e.has("MeshRenderer") {
                (0x6ea0ff, "M")
            } else {
                (0x80848c, "E")
            };
            let selected = self.selection == Some(id);
            if self.rename == Some(id) {
                let mut name = e.name.clone();
                let fid = id_of(&["rename", &id.to_string()]);
                if self.ui.focus != Some(fid) {
                    self.ui.focus = Some(fid);
                    self.ui.edit_buffer = name.clone();
                    self.ui.edit_cursor = name.chars().count();
                    self.ui.edit_select_all = true;
                }
                let fr = Rect::new(rr.x + 8 + depth * 14, rr.y + 1, rr.w - 12 - depth * 14, rr.h - 2);
                if self.ui.text_field(fid, fr, &mut name) {
                    if !name.trim().is_empty() {
                        scene.entity_mut(id).unwrap().name = name.trim().to_string();
                    }
                    self.rename = None;
                }
                if self.ui.focus != Some(fid) {
                    self.rename = None;
                }
                continue;
            }
            let label = if e.active { e.name.clone() } else { format!("{} (inactive)", e.name) };
            if self.ui.list_row(rr, &label, selected, depth * 14, Some(icon)) {
                if self.ui.input.double_click && selected && !playing {
                    self.rename = Some(id);
                } else {
                    self.selection = Some(id);
                    self.drag_entity = Some((id, self.ui.input.mouse));
                }
            }
            if let Some((dragged, start)) = self.drag_entity {
                let moved = (self.ui.input.mouse.0 - start.0).abs() + (self.ui.input.mouse.1 - start.1).abs() > 6.0;
                if moved && dragged != id && self.ui.hovered(rr) {
                    self.ui.canvas.stroke_rect(rr, ACCENT);
                    if self.ui.input.left_released {
                        drop_target = Some(Some(id));
                    }
                }
            }
        }
        self.ui.canvas.clip = old;
        if let Some((dragged, start)) = self.drag_entity {
            let moved = (self.ui.input.mouse.0 - start.0).abs() + (self.ui.input.mouse.1 - start.1).abs() > 6.0;
            if moved {
                let name = scene.entity(dragged).map(|e| e.name.clone()).unwrap_or_default();
                let (mx, my) = self.ui.input.mouse;
                self.ui.canvas.text_shadow(
                    mx as i32 + 12,
                    my as i32 + 4,
                    &format!("move '{}'", name),
                    0xffffff,
                    FontId::Ui,
                );
                if self.ui.input.left_released
                    && drop_target.is_none()
                    && self.ui.hovered(list)
                    && y < self.ui.input.mouse.1 as i32
                {
                    drop_target = Some(None); // dropped on empty space: unparent
                }
            }
            if self.ui.input.left_released || !self.ui.input.left_down {
                if let Some(target) = drop_target {
                    if playing {
                        self.status = Some(("Stop the game to change the hierarchy".into(), 3.0));
                    } else {
                        // Keep the world transform when re-parenting.
                        let world_before = scene.world_matrix(dragged);
                        match scene.set_parent(dragged, target) {
                            Ok(()) => {
                                let parent_inv =
                                    target.map(|p| scene.world_matrix(p).inverse()).unwrap_or(Mat4::IDENTITY);
                                let local = parent_inv * world_before;
                                let t = scene.entity_mut(dragged).unwrap().transform_mut();
                                t.position = local.get_translation();
                            }
                            Err(e) => self.status = Some((e, 3.0)),
                        }
                    }
                }
                self.drag_entity = None;
            }
        }
        if !playing && self.ui.clicked(list) && y < self.ui.input.mouse.1 as i32 && self.drag_entity.is_none() {
            self.selection = None;
        }
        let _ = actions;
    }

    fn draw_inspector(&mut self, r: Rect, actions: &mut Vec<Action>) {
        self.ui.panel(r, PANEL);
        self.ui.canvas.fill_rect(Rect::new(r.x, r.y, 1, r.h), BORDER);
        self.ui.header(Rect::new(r.x + 1, r.y, r.w - 1, 30), "Inspector");
        let area = Rect::new(r.x + 1, r.y + 30, r.w - 1, r.h - 30);
        if self.ui.hovered(area) && self.ui.input.wheel != 0.0 && self.ui.active.is_none() {
            self.scroll_inspector = (self.scroll_inspector - (self.ui.input.wheel * 48.0) as i32).max(0);
        }
        let world_ref = self.play.as_ref().map(|rt| rt.world.clone());
        let mut wb = world_ref.as_ref().map(|w| w.borrow_mut());
        let playing = wb.is_some();
        let scene: &mut Scene = match wb.as_mut() {
            Some(w) => &mut w.scene,
            None => &mut self.scene,
        };
        let Some(sel) = self.selection.filter(|s| scene.entity(*s).is_some()) else {
            self.ui.label_in(
                Rect::new(area.x + 14, area.y + 10, area.w - 20, 24),
                "Select an entity to inspect it.",
                TEXT_DIM,
            );
            self.ui.label_in(
                Rect::new(area.x + 14, area.y + 34, area.w - 20, 24),
                "Tip: click objects in the viewport.",
                TEXT_DIM,
            );
            return;
        };
        let old = self.ui.canvas.clip;
        self.ui.canvas.set_clip(area);
        let x0 = area.x + 12;
        let fw = area.w - 24;
        let label_w = 104;
        let mut y = area.y + 10 - self.scroll_inspector;
        let sid = sel.to_string();
        // Entity basics
        {
            let e = scene.entity(sel).unwrap();
            let mut name = e.name.clone();
            let mut active = e.active;
            let mut tag = e.tag.clone();
            let mut groups = e.groups.join(", ");
            self.ui.label_in(Rect::new(x0, y, label_w, 24), "Name", TEXT_DIM);
            if self.ui.text_field(id_of(&["name", &sid]), Rect::new(x0 + label_w, y, fw - label_w - 30, 24), &mut name)
                && !name.trim().is_empty()
            {
                scene.entity_mut(sel).unwrap().name = name.trim().to_string();
            }
            if self.ui.checkbox(Rect::new(x0 + fw - 20, y, 20, 24), &mut active) {
                scene.entity_mut(sel).unwrap().active = active;
            }
            y += 30;
            self.ui.label_in(Rect::new(x0, y, label_w, 24), "Tag", TEXT_DIM);
            if self.ui.text_field(id_of(&["tag", &sid]), Rect::new(x0 + label_w, y, fw - label_w, 24), &mut tag) {
                scene.entity_mut(sel).unwrap().tag = tag.trim().to_string();
            }
            y += 30;
            self.ui.label_in(Rect::new(x0, y, label_w, 24), "Groups", TEXT_DIM);
            if self.ui.text_field(id_of(&["groups", &sid]), Rect::new(x0 + label_w, y, fw - label_w, 24), &mut groups) {
                let mut parsed = Vec::new();
                for group in groups.split(',').map(str::trim).filter(|group| !group.is_empty()) {
                    if !parsed.iter().any(|known: &String| known.as_str() == group) {
                        parsed.push(group.to_string());
                    }
                }
                scene.entity_mut(sel).unwrap().groups = parsed;
            }
            y += 30;
            self.ui.label_in(Rect::new(x0, y, fw, 18), &format!("id {}", sel), 0x6b6f77);
            y += 24;
        }
        let comps: Vec<(String, Vec<(&'static str, FieldValue)>)> =
            scene.entity(sel).unwrap().components.iter().map(|c| (c.type_name().to_string(), c.fields())).collect();
        let mut remove: Option<String> = None;
        for (cname, fields) in comps {
            let hr = Rect::new(area.x, y, area.w, 28);
            self.ui.canvas.fill_rect(hr, HEADER);
            self.ui.canvas.fill_rect(Rect::new(hr.x, hr.y, hr.w, 1), BORDER);
            self.ui.canvas.text(x0, y + 6, &cname, TEXT, FontId::UiBold);
            if cname != "Transform" {
                let xr = Rect::new(hr.right() - 30, y + 4, 22, 20);
                if self.ui.button(xr, "x") {
                    remove = Some(cname.clone());
                }
            }
            y += 34;
            for (fname, value) in fields {
                let row = Rect::new(x0, y, fw, 24);
                self.ui.label_in(Rect::new(row.x, row.y, label_w, 24), &pretty_label(fname), TEXT_DIM);
                let wr = Rect::new(row.x + label_w, row.y, fw - label_w, 24);
                let fid = |s: &str| id_of(&[&sid, &cname, fname, s]);
                let mut new_value: Option<FieldValue> = None;
                match &value {
                    FieldValue::Vec3(v) => {
                        let cw = (wr.w - 8) / 3;
                        let mut vals = [v.x, v.y, v.z];
                        let speed = if fname == "rotation" { 0.5 } else { 0.02 };
                        let accents = [AXIS_X, AXIS_Y, AXIS_Z];
                        let mut changed = false;
                        for i in 0..3 {
                            let cr = Rect::new(wr.x + i as i32 * (cw + 4), wr.y, cw, 24);
                            if self.ui.drag_number(fid(&i.to_string()), cr, &mut vals[i], speed, Some(accents[i])) {
                                changed = true;
                            }
                        }
                        if changed {
                            new_value = Some(FieldValue::Vec3(Vec3::from_array(vals)));
                        }
                    }
                    FieldValue::Vec2(v) => {
                        let cw = (wr.w - 4) / 2;
                        let mut vals = [v.x, v.y];
                        let accents = [AXIS_X, AXIS_Y];
                        let mut changed = false;
                        for i in 0..2 {
                            let cr = Rect::new(wr.x + i as i32 * (cw + 4), wr.y, cw, 24);
                            if self.ui.drag_number(fid(&i.to_string()), cr, &mut vals[i], 0.01, Some(accents[i])) {
                                changed = true;
                            }
                        }
                        if changed {
                            new_value = Some(FieldValue::Vec2(grodot_core::math::Vec2::new(vals[0], vals[1])));
                        }
                    }
                    FieldValue::Float(f) => {
                        let mut v = *f;
                        let speed = if fname == "field_of_view" || fname == "far_clip" { 0.5 } else { 0.01 };
                        if self.ui.drag_number(fid("f"), wr, &mut v, speed, None) {
                            new_value = Some(FieldValue::Float(v));
                        }
                    }
                    FieldValue::Bool(b) => {
                        let mut v = *b;
                        if self.ui.checkbox(wr, &mut v) {
                            new_value = Some(FieldValue::Bool(v));
                        }
                    }
                    FieldValue::Color(c) => {
                        let sw = Rect::new(wr.x, wr.y + 2, 30, 20);
                        self.ui.color_swatch(sw, c.to_u32());
                        let mut hex = c.to_hex();
                        if self.ui.text_field(fid("hex"), Rect::new(wr.x + 36, wr.y, wr.w - 36, 24), &mut hex) {
                            match Color::from_hex(hex.trim()) {
                                Some(c) => new_value = Some(FieldValue::Color(c)),
                                None => self.status = Some(("Colours are hex codes like #ff8800".into(), 3.0)),
                            }
                        }
                    }
                    FieldValue::Text(t) => {
                        let mut s = t.clone();
                        let pick = matches!(
                            (cname.as_str(), fname),
                            ("MeshRenderer", "mesh" | "texture") | ("AudioSource", "clip") | ("Script", "path")
                        );
                        let tw = if pick { wr.w - 30 } else { wr.w };
                        if self.ui.text_field(fid("t"), Rect::new(wr.x, wr.y, tw, 24), &mut s) {
                            new_value = Some(FieldValue::Text(s.trim().to_string()));
                        }
                        if pick && self.ui.button(Rect::new(wr.right() - 26, wr.y, 26, 24), "...") {
                            let options = asset_options(&self.assets, &cname, fname);
                            self.popup =
                                Popup::AssetPicker { component: cname.clone(), field: fname.to_string(), options };
                            self.popup_pos = (wr.x - 60, wr.bottom());
                        }
                    }
                    FieldValue::Choice(cur, opts) => {
                        if self.ui.button(wr, &format!("{}  >", cur)) {
                            let i = opts.iter().position(|o| o == cur).unwrap_or(0);
                            new_value = Some(FieldValue::Choice(opts[(i + 1) % opts.len()].to_string(), opts));
                        }
                    }
                }
                if let Some(nv) = new_value {
                    let e = scene.entity_mut(sel).unwrap();
                    if let Some(c) = e.get_mut(&cname) {
                        if let Err(err) = c.set_field(fname, nv) {
                            self.status = Some((err, 3.0));
                        }
                    }
                }
                y += 30;
                // Texture preview under the texture field.
                if let (true, FieldValue::Text(t)) = (fname == "texture", &value) {
                    if !t.is_empty() {
                        let pr = Rect::new(x0 + label_w, y, 64, 64);
                        match self.renderer.texture(t) {
                            Some(tex) => {
                                draw_checker(&mut self.ui.canvas, pr);
                                grodot_render::texture::draw_texture(&mut self.ui.canvas, pr, &tex);
                                self.ui.canvas.stroke_rect(pr, BORDER);
                                let info = format!(
                                    "{} x {}{}",
                                    tex.width(),
                                    tex.height(),
                                    if tex.has_alpha { ", alpha" } else { "" }
                                );
                                self.ui.label_in(
                                    Rect::new(pr.right() + 10, pr.y, fw - label_w - 74, 22),
                                    &info,
                                    TEXT_DIM,
                                );
                            }
                            None => {
                                self.ui.canvas.stroke_rect(pr, RED);
                                self.ui.label_in(
                                    Rect::new(pr.right() + 10, pr.y, fw - label_w - 74, 22),
                                    "cannot load",
                                    RED,
                                );
                            }
                        }
                        y += 72;
                    }
                }
            }
            if cname == "Script" && !playing {
                let path = match scene.entity(sel).unwrap().script() {
                    Some(s) => s.path.clone(),
                    None => String::new(),
                };
                let bw = (fw - label_w - 6) / 2;
                if path.is_empty() {
                    if self.ui.button_accent(Rect::new(x0 + label_w, y, fw - label_w, 24), "Create new script") {
                        actions.push(Action::NewScript(sel));
                    }
                } else if self.ui.button(Rect::new(x0 + label_w, y, bw, 24), "Open in editor") {
                    actions.push(Action::OpenExternal(path, 1));
                }
                y += 30;
            }
            y += 6;
        }
        if let Some(c) = remove {
            scene.entity_mut(sel).unwrap().remove(&c);
        }
        y += 6;
        let br = Rect::new(x0, y, fw, 28);
        if self.ui.button_accent(br, "Add Component") {
            self.popup = Popup::AddComponent;
            self.popup_pos = (br.x + 20, br.bottom());
        }
        y += 40;
        let content_h = y + self.scroll_inspector - area.y;
        self.scroll_inspector = self.scroll_inspector.min((content_h - area.h).max(0));
        self.ui.canvas.clip = old;
    }
}

const NO_TEXTURE: &str = "(no texture)";

/// Grey checkerboard behind transparent texture previews.
fn draw_checker(canvas: &mut grodot_render::Canvas, r: Rect) {
    for cy in (0..r.h).step_by(8) {
        for cx in (0..r.w).step_by(8) {
            let c = if (cx / 8 + cy / 8) % 2 == 0 { 0x5a5d63 } else { 0x3c3f45 };
            canvas.fill_rect(Rect::new(r.x + cx, r.y + cy, 8.min(r.w - cx), 8.min(r.h - cy)), c);
        }
    }
}

fn asset_options(assets: &[String], comp: &str, field: &str) -> Vec<String> {
    {
        let mut opts: Vec<String> = Vec::new();
        match (comp, field) {
            ("MeshRenderer", "mesh") => {
                opts.extend(PRIMITIVE_MESHES.iter().map(|s| s.to_string()));
                opts.extend(
                    assets
                        .iter()
                        .filter(|a| a.to_lowercase().ends_with(".obj") || grodot_core::gltf::is_model_path(a))
                        .cloned(),
                );
            }
            ("AudioSource", "clip") => {
                opts.extend(assets.iter().filter(|a| a.to_lowercase().ends_with(".wav")).cloned())
            }
            ("Script", "path") => opts.extend(assets.iter().filter(|a| a.ends_with(".gro")).cloned()),
            ("MeshRenderer", "texture") => {
                opts.push(NO_TEXTURE.to_string());
                opts.extend(assets.iter().filter(|a| grodot_render::scene_render::is_texture_path(a)).cloned());
            }
            _ => {}
        }
        opts
    }
}

impl Editor {
    fn draw_bottom(&mut self, r: Rect, actions: &mut Vec<Action>) {
        self.ui.panel(r, PANEL_DARK);
        self.ui.canvas.fill_rect(Rect::new(r.x, r.y, r.w, 1), BORDER);
        let tabs = Rect::new(r.x, r.y + 1, r.w, 30);
        self.ui.canvas.fill_rect(tabs, HEADER);
        let errors = self.log.entries.iter().filter(|e| e.level == LogLevel::Error).count();
        let console_label = if errors > 0 { format!("Console ({} errors)", errors) } else { "Console".to_string() };
        if self.ui.toggle(Rect::new(r.x + 6, r.y + 4, 130, 24), &console_label, self.bottom_tab == BottomTab::Console) {
            self.bottom_tab = BottomTab::Console;
        }
        if self.ui.toggle(Rect::new(r.x + 142, r.y + 4, 80, 24), "Assets", self.bottom_tab == BottomTab::Assets) {
            self.bottom_tab = BottomTab::Assets;
            self.scan_assets();
        }
        let body = Rect::new(r.x, r.y + 31, r.w, r.h - 31);
        match self.bottom_tab {
            BottomTab::Console => {
                if self.ui.button(Rect::new(r.right() - 70, r.y + 4, 62, 24), "Clear") {
                    self.log.clear();
                }
                self.draw_console(body, actions);
            }
            BottomTab::Assets => {
                if self.ui.button(Rect::new(r.right() - 84, r.y + 4, 76, 24), "Refresh") {
                    self.scan_assets();
                    self.renderer.clear_cache();
                }
                self.draw_assets(body, actions);
            }
        }
    }

    fn draw_console(&mut self, r: Rect, actions: &mut Vec<Action>) {
        let line_h = 18;
        let total: i32 = self.log.entries.iter().map(|e| e.message.lines().count().max(1) as i32 * line_h + 6).sum();
        let max_scroll = (total - r.h + 8).max(0);
        if self.ui.hovered(r) && self.ui.input.wheel != 0.0 {
            self.scroll_console = (self.scroll_console - (self.ui.input.wheel * 54.0) as i32).clamp(0, max_scroll);
            self.console_follow = self.scroll_console >= max_scroll;
        }
        if self.console_follow {
            self.scroll_console = max_scroll;
        }
        let old = self.ui.canvas.clip;
        self.ui.canvas.set_clip(r);
        let mut y = r.y + 4 - self.scroll_console;
        let mut open: Option<(String, u32)> = None;
        for e in &self.log.entries {
            let lines: Vec<&str> = e.message.lines().collect();
            let h = lines.len().max(1) as i32 * line_h + 6;
            let row = Rect::new(r.x, y, r.w, h);
            if row.bottom() >= r.y && row.y <= r.bottom() {
                let (c, tag) = match e.level {
                    LogLevel::Info => (TEXT, ""),
                    LogLevel::Warning => (YELLOW, "warning "),
                    LogLevel::Error => (RED, "error "),
                };
                if e.level == LogLevel::Error {
                    self.ui.canvas.fill_rect(row, 0x2a1f22);
                }
                if e.location.is_some() && self.ui.hovered(row) {
                    self.ui.canvas.fill_rect(row, 0x30333a);
                    self.ui.tooltip = Some("Click to open the script".into());
                    if self.ui.input.left_pressed {
                        open = e.location.clone();
                    }
                }
                let mut ly = y + 3;
                let mut x = r.x + 10;
                if !tag.is_empty() {
                    x = self.ui.canvas.text(x, ly, tag, c, FontId::UiBold);
                }
                for l in &lines {
                    self.ui.canvas.text(x, ly, l, c, FontId::Ui);
                    ly += line_h;
                }
                if e.count > 1 {
                    let badge = format!("x{}", e.count);
                    let bw = self.ui.canvas.measure(&badge, FontId::UiBold) + 10;
                    let br = Rect::new(r.right() - bw - 12, y + 3, bw, 18);
                    self.ui.canvas.fill_round_rect(br, BUTTON, 255);
                    self.ui.label_centered(br, &badge, TEXT, FontId::UiBold);
                }
            }
            y += h;
            self.ui.canvas.fill_rect(Rect::new(r.x, y - 1, r.w, 1), 0x2a2c30);
        }
        self.ui.canvas.clip = old;
        if let Some((f, l)) = open {
            actions.push(Action::OpenExternal(f, l));
        }
    }

    fn draw_assets(&mut self, r: Rect, actions: &mut Vec<Action>) {
        let list_w = (r.w * 3 / 5).max(200);
        let list = Rect::new(r.x, r.y, list_w, r.h);
        let side = Rect::new(r.x + list_w, r.y, r.w - list_w, r.h);
        self.ui.canvas.fill_rect(Rect::new(side.x, side.y, 1, side.h), BORDER);
        if self.ui.hovered(list) && self.ui.input.wheel != 0.0 {
            self.scroll_assets = (self.scroll_assets - (self.ui.input.wheel * 48.0) as i32).max(0);
        }
        let max_scroll = (self.assets.len() as i32 * 22 - list.h + 8).max(0);
        self.scroll_assets = self.scroll_assets.min(max_scroll);
        let old = self.ui.canvas.clip;
        self.ui.canvas.set_clip(list);
        let mut y = list.y + 4 - self.scroll_assets;
        let assets = self.assets.clone();
        for a in &assets {
            let rr = Rect::new(list.x + 2, y, list.w - 4, 22);
            y += 22;
            if rr.bottom() < list.y || rr.y > list.bottom() {
                continue;
            }
            let icon = asset_icon(a);
            let selected = self.asset_selected.as_deref() == Some(a.as_str());
            if self.ui.list_row(rr, a, selected, 0, Some(icon)) {
                if grodot_core::gltf::is_model_path(a) || a.to_lowercase().ends_with(".obj") {
                    self.drag_asset = Some((a.clone(), self.ui.input.mouse));
                }
                if self.ui.input.double_click && selected {
                    if a.ends_with(".gscene") {
                        actions.push(Action::OpenScene(a.clone()));
                    } else if a.ends_with(".gro") {
                        actions.push(Action::OpenExternal(a.clone(), 1));
                    }
                }
                self.asset_selected = Some(a.clone());
            }
        }
        self.ui.canvas.clip = old;
        // Side panel with actions for the selected asset.
        let x = side.x + 12;
        let mut y = side.y + 10;
        let Some(sel) = self.asset_selected.clone() else {
            self.ui.label_in(Rect::new(x, y, side.w - 20, 22), "Select an asset to see actions.", TEXT_DIM);
            self.ui.label_in(Rect::new(x, y + 22, side.w - 20, 22), "Double-click scenes to open them.", TEXT_DIM);
            return;
        };
        self.ui.label_in(Rect::new(x, y, side.w - 20, 22), &sel, TEXT);
        y += 30;
        let bw = (side.w - 24).min(220);
        let lower = sel.to_lowercase();
        let sel_entity = self.selection.filter(|_| self.play.is_none());
        if lower.ends_with(".gscene") {
            if self.ui.button(Rect::new(x, y, bw, 26), "Open scene") {
                actions.push(Action::OpenScene(sel.clone()));
            }
        } else if lower.ends_with(".gro") {
            if self.ui.button(Rect::new(x, y, bw, 26), "Open in text editor") {
                actions.push(Action::OpenExternal(sel.clone(), 1));
            }
            y += 32;
            if let Some(id) = sel_entity {
                if self.ui.button(Rect::new(x, y, bw, 26), "Attach to selected entity") {
                    let e = self.scene.entity_mut(id).unwrap();
                    if !e.has("Script") {
                        e.add(Component::new_default("Script").unwrap());
                    }
                    let _ = e.get_mut("Script").unwrap().set_field("path", FieldValue::Text(sel.clone()));
                }
            }
        } else if lower.ends_with(".wav") {
            if self.ui.button(Rect::new(x, y, bw, 26), "Preview sound") {
                actions.push(Action::PlayClip(sel.clone()));
            }
            y += 32;
            if let Some(id) = sel_entity {
                if self.ui.button(Rect::new(x, y, bw, 26), "Use as AudioSource clip") {
                    let e = self.scene.entity_mut(id).unwrap();
                    if !e.has("AudioSource") {
                        e.add(Component::new_default("AudioSource").unwrap());
                    }
                    let _ = e.get_mut("AudioSource").unwrap().set_field("clip", FieldValue::Text(sel.clone()));
                }
            }
        } else if grodot_render::scene_render::is_texture_path(&lower) {
            let pr = Rect::new(x, y, 96, 96);
            if let Some(tex) = self.renderer.texture(&sel) {
                draw_checker(&mut self.ui.canvas, pr);
                grodot_render::texture::draw_texture(&mut self.ui.canvas, pr, &tex);
                self.ui.canvas.stroke_rect(pr, BORDER);
                let info = format!("{} x {}", tex.width(), tex.height());
                self.ui.label_in(Rect::new(x + 106, y, 140, 22), &info, TEXT_DIM);
            }
            if let Some(id) = sel_entity {
                if self.ui.button(Rect::new(x + 106, y + 30, bw.min(side.w - 130), 26), "Use as texture") {
                    let e = self.scene.entity_mut(id).unwrap();
                    if !e.has("MeshRenderer") {
                        e.add(Component::new_default("MeshRenderer").unwrap());
                    }
                    let _ = e.get_mut("MeshRenderer").unwrap().set_field("texture", FieldValue::Text(sel.clone()));
                }
            }
        } else if grodot_core::gltf::is_model_path(&lower) {
            if let Some(m) = self.renderer.model(&sel) {
                let info = format!(
                    "{} nodes, {} triangles, {} materials",
                    m.gltf.nodes.len(),
                    m.gltf.triangle_count(),
                    m.gltf.materials.len()
                );
                self.ui.label_in(Rect::new(x, y, side.w - 20, 22), &info, TEXT_DIM);
                y += 26;
            }
            self.ui.label_in(Rect::new(x, y, side.w - 20, 22), "Drag into the viewport to place it.", TEXT_DIM);
            y += 28;
            if let Some(id) = sel_entity {
                if self.ui.button(Rect::new(x, y, bw, 26), "Use as mesh") {
                    let e = self.scene.entity_mut(id).unwrap();
                    if !e.has("MeshRenderer") {
                        e.add(Component::new_default("MeshRenderer").unwrap());
                    }
                    let mr = e.get_mut("MeshRenderer").unwrap();
                    let _ = mr.set_field("mesh", FieldValue::Text(sel.clone()));
                    let _ = mr.set_field("base_color", FieldValue::Color(Color::WHITE));
                }
            }
        } else if lower.ends_with(".obj") {
            if let Some(id) = sel_entity {
                if self.ui.button(Rect::new(x, y, bw, 26), "Use as mesh") {
                    let e = self.scene.entity_mut(id).unwrap();
                    if !e.has("MeshRenderer") {
                        e.add(Component::new_default("MeshRenderer").unwrap());
                    }
                    let _ = e.get_mut("MeshRenderer").unwrap().set_field("mesh", FieldValue::Text(sel.clone()));
                }
            }
        } else if lower.ends_with(".gprefab") && self.ui.button(Rect::new(x, y, bw, 26), "Add to scene") {
            actions.push(Action::SpawnPrefab(sel.clone()));
        }
    }

    fn draw_popup(&mut self, _actions: &mut Vec<Action>) {
        let popup = self.popup.clone();
        let (px, py) = self.popup_pos;
        match popup {
            Popup::None => {}
            Popup::AddEntity => {
                let items: Vec<String> = [
                    "Empty", "Cube", "Sphere", "Plane", "Cylinder", "Capsule", "Camera", "Light", "Sprite2D",
                    "Camera2D",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect();
                match self.ui.popup_menu(px, py, "Create entity", &items) {
                    PopupResult::Chosen(i) => {
                        self.popup = Popup::None;
                        self.apply(Action::CreateEntity(items[i].clone()));
                    }
                    PopupResult::Closed => self.popup = Popup::None,
                    PopupResult::Open => {}
                }
            }
            Popup::AddComponent => {
                let Some(sel) = self.selection else {
                    self.popup = Popup::None;
                    return;
                };
                let world_ref = self.play.as_ref().map(|rt| rt.world.clone());
                let items: Vec<String> = {
                    let wb = world_ref.as_ref().map(|w| w.borrow());
                    let scene = match &wb {
                        Some(w) => &w.scene,
                        None => &self.scene,
                    };
                    let Some(e) = scene.entity(sel) else {
                        self.popup = Popup::None;
                        return;
                    };
                    COMPONENT_TYPES.iter().filter(|t| !e.has(t)).map(|s| s.to_string()).collect()
                };
                if items.is_empty() {
                    self.popup = Popup::None;
                    return;
                }
                match self.ui.popup_menu(px, py, "Add component", &items) {
                    PopupResult::Chosen(i) => {
                        self.popup = Popup::None;
                        let mut wb = world_ref.as_ref().map(|w| w.borrow_mut());
                        let scene = match wb.as_mut() {
                            Some(w) => &mut w.scene,
                            None => &mut self.scene,
                        };
                        if let Some(e) = scene.entity_mut(sel) {
                            e.add(Component::new_default(&items[i]).unwrap());
                        }
                    }
                    PopupResult::Closed => self.popup = Popup::None,
                    PopupResult::Open => {}
                }
            }
            Popup::AssetPicker { component, field, options } => {
                let shown = if options.is_empty() {
                    vec!["(no matching files in the project)".to_string()]
                } else {
                    options.clone()
                };
                match self.ui.popup_menu(px, py, &format!("Choose {}", field), &shown) {
                    PopupResult::Chosen(i) => {
                        self.popup = Popup::None;
                        if !options.is_empty() {
                            if let Some(sel) = self.selection {
                                let world_ref = self.play.as_ref().map(|rt| rt.world.clone());
                                let mut wb = world_ref.as_ref().map(|w| w.borrow_mut());
                                let scene = match wb.as_mut() {
                                    Some(w) => &mut w.scene,
                                    None => &mut self.scene,
                                };
                                if let Some(c) = scene.entity_mut(sel).and_then(|e| e.get_mut(&component)) {
                                    let v = if options[i] == NO_TEXTURE { String::new() } else { options[i].clone() };
                                    let _ = c.set_field(&field, FieldValue::Text(v));
                                }
                            }
                        }
                    }
                    PopupResult::Closed => self.popup = Popup::None,
                    PopupResult::Open => {}
                }
            }
        }
    }
}

fn run_audio(audio: &mut Option<Audio>, cmds: Vec<AudioCmd>, log: &mut Log) {
    let Some(a) = audio else { return };
    for c in cmds {
        let r = match c {
            AudioCmd::Play { path, volume } => a.play(&path, volume, false, false),
            AudioCmd::PlayMusic { path, volume } => a.play(&path, volume, true, true),
            AudioCmd::StopMusic => {
                a.stop_music();
                Ok(())
            }
            AudioCmd::SetVolume(v) => {
                a.set_volume(v);
                Ok(())
            }
        };
        if let Err(e) = r {
            log.warn(format!("audio: {}", e));
        }
    }
}

fn push_entry(log: &mut Log, e: LogEntry) {
    if let Some(last) = log.entries.last_mut() {
        if last.message == e.message && last.level == e.level {
            last.count += e.count;
            log.revision += 1;
            return;
        }
    }
    log.revision += 1;
    log.entries.push(e);
    if log.entries.len() > 500 {
        log.entries.remove(0);
    }
}

fn open_external(project: &Project, rel: &str, line: u32, log: &mut Log) {
    let Ok(path) = project.resolve(rel) else { return };
    let editor = std::env::var("VISUAL").or_else(|_| std::env::var("GRODOT_EDITOR"));
    let r = match editor {
        Ok(ed) if !ed.trim().is_empty() => {
            let mut parts = ed.split_whitespace();
            let prog = parts.next().unwrap().to_string();
            std::process::Command::new(prog).args(parts).arg(format!("+{}", line)).arg(&path).spawn()
        }
        _ => std::process::Command::new("xdg-open").arg(&path).spawn(),
    };
    match r {
        Ok(_) => log.info(format!("Opened {}:{} in an external editor", rel, line)),
        Err(e) => log.warn(format!("could not open {} ({}); set $VISUAL to your code editor", rel, e)),
    }
}

/// Create a new entity of a given kind at `at` (placed on the ground plane).
pub fn create_entity(scene: &mut Scene, kind: &str, at: Vec3) -> EntityId {
    let base = kind.to_string();
    let mut name = base.clone();
    let mut n = 2;
    while scene.find_by_name(&name).is_some() {
        name = format!("{} {}", base, n);
        n += 1;
    }
    let id = scene.add_entity(&name);
    let e = scene.entity_mut(id).unwrap();
    let ground = vec3(at.x.round(), 0.0, at.z.round());
    match kind {
        "Sprite2D" => {
            e.add(Component::new_default("Sprite2D").unwrap());
            e.transform_mut().position = vec3(ground.x, ground.z, 0.0);
        }
        "Camera2D" => {
            e.add(Component::new_default("Camera2D").unwrap());
        }
        "Cube" | "Sphere" | "Cylinder" | "Capsule" | "Plane" => {
            let mesh = kind.to_lowercase();
            let mut mr = Component::new_default("MeshRenderer").unwrap();
            let _ = mr.set_field("mesh", FieldValue::Text(mesh.clone()));
            e.add(mr);
            let mut col = Component::new_default("Collider").unwrap();
            match kind {
                "Sphere" => {
                    let _ = col.set_field("shape", FieldValue::Text("sphere".into()));
                }
                "Plane" => {
                    let _ = col.set_field("size", FieldValue::Vec3(vec3(1.0, 0.01, 1.0)));
                }
                "Capsule" => {
                    let _ = col.set_field("size", FieldValue::Vec3(vec3(1.0, 2.0, 1.0)));
                }
                _ => {}
            }
            e.add(col);
            let half = if kind == "Capsule" {
                1.0
            } else if kind == "Plane" {
                0.0
            } else {
                0.5
            };
            e.transform_mut().position = ground + vec3(0.0, half, 0.0);
            if kind == "Plane" {
                e.transform_mut().scale = vec3(10.0, 1.0, 10.0);
            }
        }
        "Camera" => {
            e.add(Component::new_default("Camera").unwrap());
            e.transform_mut().position = ground + vec3(0.0, 4.0, 8.0);
            e.transform_mut().rotation = Quat::from_euler_deg(vec3(-20.0, 0.0, 0.0));
        }
        "Light" => {
            e.add(Component::new_default("Light").unwrap());
            e.transform_mut().position = ground + vec3(0.0, 6.0, 0.0);
            e.transform_mut().rotation = Quat::from_euler_deg(vec3(-50.0, 30.0, 0.0));
        }
        _ => {
            e.transform_mut().position = ground;
        }
    }
    id
}

fn asset_icon(path: &str) -> (u32, &'static str) {
    let l = path.to_lowercase();
    if l.ends_with(".gscene") {
        (ACCENT, "S")
    } else if l.ends_with(".gro") {
        (GREEN, "G")
    } else if l.ends_with(".wav") {
        (0xc07ae0, "A")
    } else if l.ends_with(".obj") || grodot_core::gltf::is_model_path(&l) {
        (0x6ea0ff, "M")
    } else if l.ends_with(".gprefab") {
        (0x4fc3c3, "P")
    } else if grodot_render::scene_render::is_texture_path(&l) {
        (0xe0a050, "T")
    } else {
        (0x80848c, "F")
    }
}

fn pretty_label(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("uv_") {
        return format!("UV {}", rest);
    }
    let mut out = String::new();
    for (i, w) in s.split('_').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut c = w.chars();
        if let Some(f) = c.next() {
            if i == 0 {
                out.extend(f.to_uppercase());
            } else {
                out.push(f);
            }
            out.push_str(c.as_str());
        }
    }
    out
}

fn to_snake(class: &str) -> String {
    let mut out = String::new();
    for (i, c) in class.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-6 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 + dx * t, a.1 + dy * t);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

#[allow(dead_code)]
fn fmt_vec(v: Vec3) -> String {
    format!("({}, {}, {})", fmt_f32(v.x), fmt_f32(v.y), fmt_f32(v.z))
}
