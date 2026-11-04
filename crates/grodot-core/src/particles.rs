//! Plain data shared between the particle simulation (grodot-runtime) and the
//! renderer (grodot-render): one live particle, ready to draw.

use crate::math::Vec3;

/// One live particle as the renderer sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleSprite {
    /// World position of the particle's centre.
    pub pos: Vec3,
    /// Quad width and height in metres.
    pub size: f32,
    /// Colour and opacity (r, g, b, a), already evaluated for the particle's age.
    pub color: [f32; 4],
    /// Texture path, if the emitter has one.
    pub texture: Option<String>,
}
