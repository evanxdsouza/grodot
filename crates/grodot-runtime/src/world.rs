//! Runtime world state shared between the game loop and script bindings.

use grodot_core::math::{Color, Vec3};
use grodot_core::scene::{EntityId, Scene};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    /// How many times this exact message was logged in a row.
    pub count: u32,
    /// Optional source location (`file:line`) for click-to-open.
    pub location: Option<(String, u32)>,
}

#[derive(Debug, Default, Clone)]
pub struct Log {
    pub entries: Vec<LogEntry>,
    /// Increases every time something is logged.
    pub revision: u64,
}

impl Log {
    pub fn push(&mut self, level: LogLevel, message: impl Into<String>) {
        self.push_at(level, message, None);
    }

    pub fn push_at(&mut self, level: LogLevel, message: impl Into<String>, location: Option<(String, u32)>) {
        let message = message.into();
        self.revision += 1;
        if let Some(last) = self.entries.last_mut() {
            if last.message == message && last.level == level {
                last.count += 1;
                return;
            }
        }
        self.entries.push(LogEntry { level, message, count: 1, location });
        if self.entries.len() > 500 {
            self.entries.remove(0);
        }
    }

    pub fn info(&mut self, m: impl Into<String>) {
        self.push(LogLevel::Info, m)
    }
    pub fn warn(&mut self, m: impl Into<String>) {
        self.push(LogLevel::Warning, m)
    }
    pub fn error(&mut self, m: impl Into<String>) {
        self.push(LogLevel::Error, m)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.revision += 1;
    }
    pub fn has_errors(&self) -> bool {
        self.entries.iter().any(|e| e.level == LogLevel::Error)
    }
}

/// Keyboard and mouse state with named actions.
#[derive(Debug, Default, Clone)]
pub struct InputState {
    pub down: HashSet<String>,
    pub pressed: HashSet<String>,
    pub released: HashSet<String>,
    pub mouse_position: (f32, f32),
    pub mouse_delta: (f32, f32),
    pub actions: BTreeMap<String, Vec<String>>,
}

impl InputState {
    pub fn with_actions(actions: BTreeMap<String, Vec<String>>) -> Self {
        InputState { actions, ..Default::default() }
    }

    pub fn press(&mut self, key: &str) {
        if self.down.insert(key.to_string()) {
            self.pressed.insert(key.to_string());
        }
    }

    pub fn release(&mut self, key: &str) {
        if self.down.remove(key) {
            self.released.insert(key.to_string());
        }
    }

    /// Release everything (e.g. when the window loses focus).
    pub fn release_all(&mut self) {
        let keys: Vec<String> = self.down.drain().collect();
        for k in keys {
            self.released.insert(k);
        }
    }

    pub fn end_frame(&mut self) {
        self.pressed.clear();
        self.released.clear();
        self.mouse_delta = (0.0, 0.0);
    }

    /// Keys bound to an action. An unknown action that is itself a key name maps to that key.
    pub fn keys_for(&self, action: &str) -> Option<Vec<String>> {
        if let Some(k) = self.actions.get(action) {
            return Some(k.clone());
        }
        if is_key_name(action) {
            return Some(vec![action.to_string()]);
        }
        None
    }

    pub fn action_down(&self, action: &str) -> Option<bool> {
        Some(self.keys_for(action)?.iter().any(|k| self.down.contains(k)))
    }
    pub fn action_pressed(&self, action: &str) -> Option<bool> {
        Some(self.keys_for(action)?.iter().any(|k| self.pressed.contains(k)))
    }
    pub fn action_released(&self, action: &str) -> Option<bool> {
        Some(self.keys_for(action)?.iter().any(|k| self.released.contains(k)))
    }
}

pub const KEY_NAMES: &[&str] = &[
    "Space", "Enter", "Escape", "Tab", "Shift", "Ctrl", "Alt", "Backspace", "Delete", "Up", "Down", "Left", "Right",
    "MouseLeft", "MouseRight", "MouseMiddle", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

pub fn is_key_name(s: &str) -> bool {
    if s.len() == 1 {
        let c = s.chars().next().unwrap();
        return c.is_ascii_uppercase() || c.is_ascii_digit();
    }
    KEY_NAMES.contains(&s)
}

#[derive(Debug, Clone)]
pub struct TimeState {
    pub delta: f32,
    pub elapsed: f64,
    pub frame: u64,
    pub fixed_delta: f32,
    pub fps: f32,
}

impl Default for TimeState {
    fn default() -> Self {
        TimeState { delta: 0.0, elapsed: 0.0, frame: 0, fixed_delta: 1.0 / 60.0, fps: 0.0 }
    }
}

/// Immediate-mode HUD requests from scripts (cleared every frame).
#[derive(Debug, Default, Clone)]
pub struct Hud {
    pub lines: Vec<String>,
    pub message: Option<String>,
    pub sub_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AudioCmd {
    Play { path: PathBuf, volume: f32 },
    PlayMusic { path: PathBuf, volume: f32 },
    StopMusic,
    SetVolume(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionKind {
    Enter,
    Exit,
}

pub struct World {
    pub scene: Scene,
    pub project_root: Option<PathBuf>,
    pub input: InputState,
    pub time: TimeState,
    pub log: Log,
    pub hud: Hud,
    pub debug_lines: Vec<(Vec3, Vec3, Color)>,
    pub audio_cmds: Vec<AudioCmd>,
    pub pending_destroy: Vec<EntityId>,
    pub pending_scene_load: Option<String>,
    pub quit_requested: bool,
    /// Entities that were spawned this frame and still need scripts attached.
    pub spawned: Vec<EntityId>,
    /// Physics: current contacts, grounded bodies and pending collision events.
    pub contacts: HashSet<(EntityId, EntityId)>,
    pub grounded: HashSet<EntityId>,
    pub collision_events: Vec<(EntityId, EntityId, CollisionKind)>,
    /// Forces accumulated by scripts for the next physics step.
    pub forces: HashMap<EntityId, Vec3>,
    /// Script objects attached to entities (for `other.health` style access).
    pub script_objects: HashMap<EntityId, groscript::Value>,
    /// Scene file this world was loaded from (for Scene.reload()).
    pub scene_path: Option<String>,
}

impl World {
    pub fn new(scene: Scene, project_root: Option<PathBuf>, actions: BTreeMap<String, Vec<String>>) -> World {
        World {
            scene,
            project_root,
            input: InputState::with_actions(actions),
            time: TimeState::default(),
            log: Log::default(),
            hud: Hud::default(),
            debug_lines: Vec::new(),
            audio_cmds: Vec::new(),
            pending_destroy: Vec::new(),
            pending_scene_load: None,
            quit_requested: false,
            spawned: Vec::new(),
            contacts: HashSet::new(),
            grounded: HashSet::new(),
            collision_events: Vec::new(),
            forces: HashMap::new(),
            script_objects: HashMap::new(),
            scene_path: None,
        }
    }

    pub fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        match &self.project_root {
            Some(r) => grodot_core::config::resolve_in(r, rel),
            None => grodot_core::config::resolve_in(&std::env::current_dir().unwrap_or_default(), rel),
        }
    }

    pub fn is_alive(&self, id: EntityId) -> bool {
        self.scene.entity(id).is_some() && !self.pending_destroy.contains(&id)
    }
}
