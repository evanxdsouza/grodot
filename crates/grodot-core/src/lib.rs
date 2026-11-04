//! Shared data types for Grodot: math, JSON, project configuration and the
//! scene/entity/component model. This crate has no dependencies.

pub mod config;
pub mod gltf;
pub mod image;
pub mod inflate;
pub mod json;
pub mod math;
pub mod mesh;
pub mod scene;

pub use config::{Project, ProjectConfig};
pub use math::{vec3, Color, Mat4, Quat, Vec3};
pub use scene::{Component, Entity, EntityId, FieldValue, Scene};

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
