//! CPU particle simulation for `ParticleEmitter` components.
//!
//! Each emitter keeps its live particles in world space. Every frame the
//! runtime spawns new particles (rate x time, plus any queued bursts), moves
//! them under their velocity and gravity, and retires the ones past their
//! lifetime. The renderer draws them through [`sprites`].
//!
//! Each emitter has its own random number generator, seeded from its entity
//! id, so a scene plays the same way every run. That keeps golden-image tests
//! stable.
//!
//! Cost: a particle is a few dozen floats, and drawing one is a small quad, so
//! hundreds of particles are cheap. The renderer sorts them with the blended
//! geometry, which costs a sort per frame over the particle count.

use crate::world::World;
use grodot_core::math::{Color, Vec3};
use grodot_core::particles::ParticleSprite;
use grodot_core::scene::{Component, EntityId, ParticleEmitter};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub age: f32,
    pub life: f32,
}

/// Live particles and the random state of one emitter.
#[derive(Debug, Clone)]
pub struct Emitter {
    pub particles: Vec<Particle>,
    /// Fractional particles carried over between frames.
    accum: f32,
    rng: u32,
}

impl Emitter {
    fn new(id: EntityId) -> Emitter {
        // Mix the id so neighbouring entities do not share a sequence.
        let seed = (id as u32).wrapping_mul(0x9E37_79B9) ^ 0x5bd1_e995;
        Emitter { particles: Vec::new(), accum: 0.0, rng: seed.max(1) }
    }

    /// A number in [0, 1).
    fn rand(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn spawn(&mut self, e: &ParticleEmitter, origin: Vec3, up: Vec3) {
        let spread = e.spread.clamp(0.0, 180.0).to_radians();
        // Pick a direction inside the cone around `up`, uniform over its solid angle.
        let cos_t = 1.0 - self.rand() * (1.0 - spread.cos());
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let phi = self.rand() * TAU;
        let helper = if up.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        let a = up.cross(helper).normalized();
        let b = up.cross(a);
        let dir = (up * cos_t + (a * phi.cos() + b * phi.sin()) * sin_t).normalized();
        let life = (e.lifetime * (1.0 - e.lifetime_random.clamp(0.0, 1.0) * self.rand())).max(0.001);
        self.particles.push(Particle { pos: origin, vel: dir * e.speed, age: 0.0, life });
    }
}

/// Advance every emitter by `dt` seconds and drop state for removed entities.
pub fn advance_all(world: &mut World, dt: f32) {
    let mut live: Vec<(EntityId, ParticleEmitter, Vec3, Vec3)> = Vec::new();
    for e in &world.scene.entities {
        if let Some(Component::ParticleEmitter(p)) = e.get("ParticleEmitter") {
            if !world.scene.is_active_in_hierarchy(e.id) {
                continue;
            }
            let m = world.scene.world_matrix(e.id);
            live.push((e.id, p.clone(), m.transform_point(Vec3::ZERO), m.transform_vector(Vec3::Y).normalized()));
        }
    }
    world.emitters.retain(|id, _| live.iter().any(|(l, _, _, _)| l == id));

    for (id, e, origin, up) in live {
        let bursts = world.particle_bursts.remove(&id).unwrap_or(0);
        let st = world.emitters.entry(id).or_insert_with(|| Emitter::new(id));
        let max = e.max_particles as usize;
        let mut spawn = bursts as usize;
        if e.playing {
            st.accum += e.rate.max(0.0) * dt;
            let whole = st.accum.floor();
            st.accum -= whole;
            spawn += whole as usize;
        }
        for _ in 0..spawn {
            if st.particles.len() >= max {
                break;
            }
            st.spawn(&e, origin, up);
        }
        let g = e.gravity;
        for p in &mut st.particles {
            p.age += dt;
            p.vel += g * dt;
            p.pos += p.vel * dt;
        }
        st.particles.retain(|p| p.age < p.life);
    }
}

/// Queue `n` particles for the next frame (the `emit_burst` script call).
pub fn queue_burst(world: &mut World, id: EntityId, n: u32) {
    *world.particle_bursts.entry(id).or_insert(0) += n;
}

/// How many particles are alive for an entity right now.
pub fn live_count(world: &World, id: EntityId) -> usize {
    world.emitters.get(&id).map(|e| e.particles.len()).unwrap_or(0)
}

/// Every live particle, ready to draw, with colour and size evaluated for its age.
pub fn sprites(world: &World) -> Vec<ParticleSprite> {
    let mut out = Vec::new();
    for e in &world.scene.entities {
        let Some(Component::ParticleEmitter(p)) = e.get("ParticleEmitter") else { continue };
        let Some(st) = world.emitters.get(&e.id) else { continue };
        if !world.scene.is_active_in_hierarchy(e.id) {
            continue;
        }
        for part in &st.particles {
            let t = (part.age / part.life).clamp(0.0, 1.0);
            out.push(ParticleSprite {
                pos: part.pos,
                size: lerp(p.size_start, p.size_end, t),
                color: lerp_color(p.color_start, p.color_end, t),
                texture: p.texture.clone(),
            });
        }
    }
    out
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_color(a: Color, b: Color, t: f32) -> [f32; 4] {
    [lerp(a.r, b.r, t), lerp(a.g, b.g, t), lerp(a.b, b.b, t), lerp(a.a, b.a, t)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = Emitter::new(7);
        let mut b = Emitter::new(7);
        for _ in 0..1000 {
            let x = a.rand();
            assert_eq!(x, b.rand());
            assert!((0.0..1.0).contains(&x));
        }
    }

    #[test]
    fn spread_zero_fires_straight_up() {
        let mut e = Emitter::new(1);
        let p = ParticleEmitter { spread: 0.0, speed: 2.0, ..Default::default() };
        for _ in 0..20 {
            e.spawn(&p, Vec3::ZERO, Vec3::Y);
        }
        for part in &e.particles {
            assert!((part.vel - Vec3::Y * 2.0).length() < 1e-4, "{:?}", part.vel);
        }
    }

    #[test]
    fn cone_directions_stay_inside_the_spread() {
        let mut e = Emitter::new(2);
        let p = ParticleEmitter { spread: 30.0, speed: 1.0, ..Default::default() };
        for _ in 0..500 {
            e.spawn(&p, Vec3::ZERO, Vec3::Y);
        }
        for part in &e.particles {
            let cos = part.vel.normalized().dot(Vec3::Y);
            assert!(cos >= 30f32.to_radians().cos() - 1e-4, "outside the cone: cos {}", cos);
        }
    }
}
