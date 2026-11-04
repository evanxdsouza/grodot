//! The Grodot software renderer.
//!
//! Everything is drawn on the CPU into a [`Canvas`]: a 3D rasterizer with depth
//! testing and simple lighting, plus 2D primitives and text for the editor UI
//! and in-game HUD. Rendering in software keeps the engine dependency-free and
//! lets tests render headlessly (e.g. to PNG) without a GPU.

pub mod canvas;
pub mod font;
pub mod raster;
pub mod scene_render;
pub mod texture;

pub use canvas::{rgb, Canvas, Rect, Rgb};
pub use font::FontId;
pub use raster::View;
pub use scene_render::{RenderOptions, SceneRenderer};
