//! Runtime side of glTF animation: per-entity clip state, playback,
//! crossfades and finished events.
//!
//! Each entity with an `AnimationPlayer` whose MeshRenderer shows a glTF model
//! gets an [`AnimState`]. Every frame the runtime advances it, writes the
//! node matrices into `World::poses` (read by the renderer, which skins
//! meshes on the CPU) and queues `on_animation_finished(clip)` events.

use crate::world::World;
use grodot_core::animation::{self, Trs};
use grodot_core::gltf::Gltf;
use grodot_core::math::Mat4;
use grodot_core::scene::{Component, EntityId};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct AnimState {
    pub model: String,
    pub gltf: Rc<Gltf>,
    /// Current clip index and playback time (seconds).
    pub clip: Option<usize>,
    pub time: f32,
    pub playing: bool,
    pub speed: f32,
    pub looping: bool,
    /// Pose to fade out of, and fade progress (seconds elapsed / total).
    pub fade_from: Option<Vec<Trs>>,
    pub fade_elapsed: f32,
    pub fade_time: f32,
    /// The pose shown this frame.
    pub pose: Vec<Trs>,
}

impl AnimState {
    pub fn clip_name(&self) -> Option<&str> {
        self.clip.and_then(|c| self.gltf.animations.get(c)).map(|a| a.name.as_str())
    }

    pub fn clip_names(&self) -> Vec<String> {
        self.gltf.animations.iter().map(|a| a.name.clone()).collect()
    }

    pub fn duration(&self) -> f32 {
        self.clip.and_then(|c| self.gltf.animations.get(c)).map(|a| a.duration).unwrap_or(0.0)
    }

    /// Sample the current clip (and crossfade) into `pose`.
    fn evaluate(&mut self) {
        let mut pose = animation::rest_pose(&self.gltf);
        if let Some(a) = self.clip.and_then(|c| self.gltf.animations.get(c)) {
            animation::apply(a, self.time, &mut pose);
        }
        if let Some(from) = &self.fade_from {
            let w = if self.fade_time > 0.0 { (self.fade_elapsed / self.fade_time).min(1.0) } else { 1.0 };
            pose = animation::blend(from, &pose, w);
            if w >= 1.0 {
                self.fade_from = None;
            }
        }
        self.pose = pose;
    }

    /// Switch to `clip`, fading from the current pose over `fade` seconds.
    pub fn play(&mut self, clip: usize, fade: f32, looping: bool, speed: f32) {
        if self.clip == Some(clip) && self.playing {
            return; // already playing: do not restart (call stop() first to restart)
        }
        self.fade_from = if fade > 0.0 && (self.clip.is_some() || self.fade_from.is_some()) {
            Some(self.pose.clone())
        } else {
            None
        };
        self.fade_elapsed = 0.0;
        self.fade_time = fade;
        self.clip = Some(clip);
        self.time = if speed < 0.0 { self.gltf.animations[clip].duration } else { 0.0 };
        self.playing = true;
        self.looping = looping;
        self.speed = speed;
        self.evaluate();
    }

    pub fn stop(&mut self) {
        self.clip = None;
        self.playing = false;
        self.time = 0.0;
        self.fade_from = None;
        self.evaluate();
    }

    /// Advance by `dt`. Returns the clip name if a non-looping clip just finished.
    pub fn advance(&mut self, dt: f32) -> Option<String> {
        let mut finished = None;
        if self.fade_from.is_some() {
            self.fade_elapsed += dt;
        }
        if self.playing {
            if let Some(c) = self.clip {
                let dur = self.gltf.animations[c].duration;
                self.time += dt * self.speed;
                if dur <= 0.0 {
                    self.time = 0.0;
                } else if self.looping {
                    self.time = self.time.rem_euclid(dur);
                } else if self.time >= dur || self.time <= 0.0 && self.speed < 0.0 {
                    self.time = self.time.clamp(0.0, dur);
                    self.playing = false;
                    finished = Some(self.gltf.animations[c].name.clone());
                }
            }
        }
        self.evaluate();
        finished
    }

    pub fn local_matrices(&self) -> Vec<Mat4> {
        animation::local_matrices(&self.pose)
    }
}

/// Load (and cache) a glTF model for animation.
pub fn model(world: &mut World, path: &str) -> Result<Rc<Gltf>, String> {
    if let Some(m) = world.models.get(path) {
        return m.clone();
    }
    let r = world
        .resolve(path)
        .and_then(|full| grodot_core::gltf::load(&full).map_err(|e| e.0))
        .map(Rc::new)
        .map_err(|e| format!("model '{}': {}", path, e));
    world.models.insert(path.to_string(), r.clone());
    r
}

/// Create animation state for an entity that has an AnimationPlayer (and autoplay its clip).
pub fn attach(world: &mut World, id: EntityId) {
    let Some(e) = world.scene.entity(id) else { return };
    let Some(Component::AnimationPlayer(ap)) = e.get("AnimationPlayer").cloned() else { return };
    let name = e.name.clone();
    let Some(mesh) = e.mesh().map(|m| m.mesh.clone()) else {
        world.log.warn(format!("'{}': AnimationPlayer needs a MeshRenderer showing a .glb or .gltf model", name));
        return;
    };
    if !grodot_core::gltf::is_model_path(&mesh) {
        world.log.warn(format!("'{}': AnimationPlayer needs a glTF model, but the mesh is '{}'", name, mesh));
        return;
    }
    let gltf = match model(world, &mesh) {
        Ok(g) => g,
        Err(e) => return world.log.error(e),
    };
    let mut st = AnimState {
        model: mesh,
        pose: animation::rest_pose(&gltf),
        gltf,
        clip: None,
        time: 0.0,
        playing: false,
        speed: ap.speed,
        looping: ap.looping,
        fade_from: None,
        fade_elapsed: 0.0,
        fade_time: 0.0,
    };
    if ap.autoplay && !st.gltf.animations.is_empty() {
        let clip = if ap.clip.is_empty() { Some(0) } else { st.gltf.animation_index(&ap.clip) };
        match clip {
            Some(c) => st.play(c, 0.0, ap.looping, ap.speed),
            None => world.log.warn(format!(
                "'{}': AnimationPlayer clip '{}' is not in the model (clips: {})",
                name,
                ap.clip,
                st.clip_names().join(", ")
            )),
        }
    }
    world.poses.insert(id, st.local_matrices());
    world.animations.insert(id, st);
}

/// Advance every animation; returns (entity, clip) for clips that finished this frame.
pub fn advance_all(world: &mut World, dt: f32) -> Vec<(EntityId, String)> {
    let mut finished = Vec::new();
    let ids: Vec<EntityId> = world.animations.keys().copied().collect();
    let mut ids = ids;
    ids.sort();
    for id in ids {
        if !world.is_alive(id) {
            world.animations.remove(&id);
            world.poses.remove(&id);
            continue;
        }
        let st = world.animations.get_mut(&id).unwrap();
        if let Some(clip) = st.advance(dt) {
            finished.push((id, clip));
        }
        let m = st.local_matrices();
        world.poses.insert(id, m);
    }
    finished
}

/// World matrices of every node of an entity's animated model (model space).
pub fn node_world_matrices(world: &World, id: EntityId) -> Option<Vec<Mat4>> {
    let st = world.animations.get(&id)?;
    Some(st.gltf.world_from_locals(&st.local_matrices()))
}

/// Skinning matrices (joint world * inverse bind) for an entity's skin.
pub fn joint_matrices(world: &World, id: EntityId, skin: usize) -> Option<Vec<Mat4>> {
    let st = world.animations.get(&id)?;
    let w = st.gltf.world_from_locals(&st.local_matrices());
    Some(animation::joint_matrices(&st.gltf, skin, &w))
}
