//! The Grodot runtime: world state, physics, script host and game loop.
//!
//! The runtime is headless: it never opens a window. Hosts (the editor's Play
//! mode and the standalone player) feed it input, call [`Runtime::frame`] and
//! render `world.scene`. This makes the whole game loop testable without a GPU
//! or display.

pub mod animation;
pub mod bindings;
pub mod physics;
pub mod runtime;
pub mod world;

pub use runtime::{load_scene_file, Runtime};
pub use world::{AudioCmd, InputState, Log, LogEntry, LogLevel, World};
