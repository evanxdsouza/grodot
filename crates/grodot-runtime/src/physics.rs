//! A small fixed-step physics engine for game-jam scale scenes.
//!
//! * Shapes: axis-aligned boxes (rotated boxes use their world bounding box)
//!   and spheres.
//! * Bodies: static (no RigidBody, or body_type "static"), dynamic (gravity,
//!   velocity, pushed out of other bodies) and kinematic (moved only by
//!   velocity or scripts, never pushed).
//! * Rotation is not simulated; dynamic bodies keep their orientation.
//! * Sensors report overlaps but never block.

use crate::world::{CollisionKind, World};
use grodot_core::math::{Mat4, Vec3};
use grodot_core::scene::{BodyType, Collider, Component, EntityId, Scene, Shape};
use std::collections::HashSet;

pub const GRAVITY: f32 = -9.81;
const CONTACT_SLOP: f32 = 0.02;

#[derive(Debug, Clone, Copy)]
pub enum WShape {
    Aabb { min: Vec3, max: Vec3 },
    Sphere { c: Vec3, r: f32 },
}

impl WShape {
    fn bounds(&self) -> (Vec3, Vec3) {
        match *self {
            WShape::Aabb { min, max } => (min, max),
            WShape::Sphere { c, r } => (c - Vec3::splat(r), c + Vec3::splat(r)),
        }
    }
    fn inflate(&self, d: f32) -> WShape {
        match *self {
            WShape::Aabb { min, max } => WShape::Aabb { min: min - Vec3::splat(d), max: max + Vec3::splat(d) },
            WShape::Sphere { c, r } => WShape::Sphere { c, r: r + d },
        }
    }
    fn translate(&self, d: Vec3) -> WShape {
        match *self {
            WShape::Aabb { min, max } => WShape::Aabb { min: min + d, max: max + d },
            WShape::Sphere { c, r } => WShape::Sphere { c: c + d, r },
        }
    }
}

pub fn world_shape(world: &Mat4, c: &Collider) -> WShape {
    match c.shape {
        Shape::Box => {
            let h = c.size * 0.5;
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for i in 0..8 {
                let p = Vec3 {
                    x: if i & 1 == 0 { -h.x } else { h.x },
                    y: if i & 2 == 0 { -h.y } else { h.y },
                    z: if i & 4 == 0 { -h.z } else { h.z },
                } + c.offset;
                let w = world.transform_point(p);
                min = min.min(w);
                max = max.max(w);
            }
            WShape::Aabb { min, max }
        }
        Shape::Sphere => {
            let sx = Vec3 { x: world.c[0][0], y: world.c[0][1], z: world.c[0][2] }.length();
            let sy = Vec3 { x: world.c[1][0], y: world.c[1][1], z: world.c[1][2] }.length();
            let sz = Vec3 { x: world.c[2][0], y: world.c[2][1], z: world.c[2][2] }.length();
            WShape::Sphere { c: world.transform_point(c.offset), r: c.radius * sx.max(sy).max(sz) }
        }
    }
}

/// Minimum translation: normal points from `a` towards `b`, depth > 0 when overlapping.
pub fn penetration(a: &WShape, b: &WShape) -> Option<(Vec3, f32)> {
    match (*a, *b) {
        (WShape::Aabb { min: amin, max: amax }, WShape::Aabb { min: bmin, max: bmax }) => {
            let mut best = (Vec3::ZERO, f32::INFINITY);
            for i in 0..3 {
                let o = amax.get(i).min(bmax.get(i)) - amin.get(i).max(bmin.get(i));
                if o <= 0.0 {
                    return None;
                }
                if o < best.1 {
                    let ca = (amin.get(i) + amax.get(i)) * 0.5;
                    let cb = (bmin.get(i) + bmax.get(i)) * 0.5;
                    let mut n = Vec3::ZERO;
                    n.set(i, if cb >= ca { 1.0 } else { -1.0 });
                    best = (n, o);
                }
            }
            Some(best)
        }
        (WShape::Sphere { c: ca, r: ra }, WShape::Sphere { c: cb, r: rb }) => {
            let d = cb - ca;
            let dist = d.length();
            let depth = ra + rb - dist;
            if depth <= 0.0 {
                return None;
            }
            let n = if dist > 1e-6 { d / dist } else { Vec3::Y };
            Some((n, depth))
        }
        (WShape::Sphere { c, r }, WShape::Aabb { min, max }) => sphere_box(c, r, min, max),
        (WShape::Aabb { min, max }, WShape::Sphere { c, r }) => sphere_box(c, r, min, max).map(|(n, d)| (-n, d)),
    }
}

/// Sphere vs box, normal from sphere to box.
fn sphere_box(c: Vec3, r: f32, min: Vec3, max: Vec3) -> Option<(Vec3, f32)> {
    let closest = c.max(min).min(max);
    let d = closest - c;
    let dist = d.length();
    if dist > 1e-6 {
        if dist >= r {
            return None;
        }
        return Some((d / dist, r - dist));
    }
    // Centre inside the box: push out along the shallowest axis.
    penetration(&WShape::Aabb { min: c - Vec3::splat(r), max: c + Vec3::splat(r) }, &WShape::Aabb { min, max })
}

struct Body {
    id: EntityId,
    index: usize,
    kind: BodyType,
    sensor: bool,
    shape: WShape,
    inv_mass: f32,
    friction: f32,
    restitution: f32,
    parent_inv: Option<Mat4>,
}

fn collect(scene: &Scene) -> Vec<Body> {
    let mut out = Vec::new();
    for (index, e) in scene.entities.iter().enumerate() {
        let Some(col) = e.collider() else { continue };
        if !scene.is_active_in_hierarchy(e.id) {
            continue;
        }
        let (kind, mass) = match e.rigid_body() {
            Some(rb) => (rb.body_type, rb.mass),
            None => (BodyType::Static, 0.0),
        };
        let world = scene.world_matrix(e.id);
        out.push(Body {
            id: e.id,
            index,
            kind,
            sensor: col.sensor,
            shape: world_shape(&world, col),
            inv_mass: if kind == BodyType::Dynamic { 1.0 / mass.max(0.001) } else { 0.0 },
            friction: col.friction,
            restitution: col.restitution,
            parent_inv: e.parent.map(|p| scene.world_matrix(p).inverse()),
        });
    }
    out
}

fn move_entity(scene: &mut Scene, b: &Body, delta: Vec3) {
    let local = match &b.parent_inv {
        Some(inv) => inv.transform_vector(delta),
        None => delta,
    };
    scene.entities[b.index].transform_mut().position += local;
}

fn velocity_mut(scene: &mut Scene, index: usize) -> Option<&mut Vec3> {
    match scene.entities[index].get_mut("RigidBody") {
        Some(Component::RigidBody(rb)) => Some(&mut rb.velocity),
        _ => None,
    }
}

/// Advance the simulation by one fixed step.
pub fn step(world: &mut World, dt: f32) {
    let scene = &mut world.scene;
    // 1. Integrate velocities and positions.
    for i in 0..scene.entities.len() {
        let id = scene.entities[i].id;
        if !scene.is_active_in_hierarchy(id) {
            continue;
        }
        let (kind, gravity, mass) = match scene.entities[i].rigid_body() {
            Some(rb) => (rb.body_type, rb.gravity, rb.mass),
            None => continue,
        };
        if kind == BodyType::Static {
            continue;
        }
        let force = world.forces.remove(&id).unwrap_or(Vec3::ZERO);
        let parent_inv = scene.entities[i].parent.map(|p| scene.world_matrix(p).inverse());
        let v = {
            let v = velocity_mut(scene, i).unwrap();
            if kind == BodyType::Dynamic {
                if gravity {
                    v.y += GRAVITY * dt;
                }
                *v += force * (dt / mass.max(0.001));
                // Terminal velocity keeps tunnelling in check.
                v.y = v.y.max(-50.0);
            }
            *v
        };
        let delta = v * dt;
        let local = match parent_inv {
            Some(inv) => inv.transform_vector(delta),
            None => delta,
        };
        scene.entities[i].transform_mut().position += local;
    }
    world.forces.clear();

    // 2. Resolve penetrations.
    let mut grounded = HashSet::new();
    for _iter in 0..3 {
        let mut bodies = collect(scene);
        let n = bodies.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let (a, b) = (&bodies[i], &bodies[j]);
                if a.sensor || b.sensor {
                    continue;
                }
                if a.inv_mass == 0.0 && b.inv_mass == 0.0 {
                    continue;
                }
                let Some((normal, depth)) = penetration(&a.shape, &b.shape) else { continue };
                let total = a.inv_mass + b.inv_mass;
                let (wa, wb) = (a.inv_mass / total, b.inv_mass / total);
                let push_a = -normal * (depth * wa);
                let push_b = normal * (depth * wb);
                if a.inv_mass > 0.0 {
                    move_entity(scene, &bodies[i], push_a);
                    bodies[i].shape = bodies[i].shape.translate(push_a);
                    if normal.y < -0.5 {
                        grounded.insert(bodies[i].id);
                    }
                }
                if bodies[j].inv_mass > 0.0 {
                    move_entity(scene, &bodies[j], push_b);
                    bodies[j].shape = bodies[j].shape.translate(push_b);
                    if normal.y > 0.5 {
                        grounded.insert(bodies[j].id);
                    }
                }
                let (a, b) = (&bodies[i], &bodies[j]);
                let e = a.restitution.max(b.restitution);
                let fr = (a.friction * b.friction).sqrt();
                let va = velocity_mut(scene, a.index).copied().unwrap_or(Vec3::ZERO);
                let vb = velocity_mut(scene, b.index).copied().unwrap_or(Vec3::ZERO);
                let rel = (vb - va).dot(normal);
                if rel < 0.0 {
                    let jimp = -(1.0 + e) * rel / total;
                    let fric = |v: Vec3, n: Vec3| {
                        let vn = n * v.dot(n);
                        let vt = v - vn;
                        vn + vt * (1.0 - (fr * 12.0 * dt).min(1.0))
                    };
                    if a.inv_mass > 0.0 {
                        if let Some(v) = velocity_mut(scene, a.index) {
                            *v = fric(*v - normal * (jimp * a.inv_mass), normal);
                        }
                    }
                    if b.inv_mass > 0.0 {
                        if let Some(v) = velocity_mut(scene, b.index) {
                            *v = fric(*v + normal * (jimp * b.inv_mass), normal);
                        }
                    }
                }
            }
        }
    }

    // 3. Contacts (with a little slop so resting contacts persist) and events.
    let bodies = collect(scene);
    let mut contacts = HashSet::new();
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            let (a, b) = (&bodies[i], &bodies[j]);
            if a.kind == BodyType::Static && b.kind == BodyType::Static {
                continue;
            }
            let (amin, amax) = a.shape.bounds();
            let (bmin, bmax) = b.shape.bounds();
            if amax.x + CONTACT_SLOP < bmin.x || bmax.x + CONTACT_SLOP < amin.x {
                continue;
            }
            if amax.y + CONTACT_SLOP < bmin.y || bmax.y + CONTACT_SLOP < amin.y {
                continue;
            }
            if amax.z + CONTACT_SLOP < bmin.z || bmax.z + CONTACT_SLOP < amin.z {
                continue;
            }
            if penetration(&a.shape.inflate(CONTACT_SLOP), &b.shape).is_some() {
                let key = if a.id < b.id { (a.id, b.id) } else { (b.id, a.id) };
                contacts.insert(key);
                // Resting on top of something counts as grounded.
                if a.inv_mass > 0.0 && amin.y >= bmax.y - CONTACT_SLOP * 2.0 && !b.sensor {
                    grounded.insert(a.id);
                }
                if b.inv_mass > 0.0 && bmin.y >= amax.y - CONTACT_SLOP * 2.0 && !a.sensor {
                    grounded.insert(b.id);
                }
            }
        }
    }
    let mut new: Vec<_> = contacts.difference(&world.contacts).copied().collect();
    let mut gone: Vec<_> = world.contacts.difference(&contacts).copied().collect();
    new.sort();
    gone.sort();
    for (a, b) in new {
        world.collision_events.push((a, b, CollisionKind::Enter));
    }
    for (a, b) in gone {
        world.collision_events.push((a, b, CollisionKind::Exit));
    }
    world.contacts = contacts;
    world.grounded = grounded;
}

#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub entity: EntityId,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

/// Cast a ray against all non-sensor colliders.
pub fn raycast(scene: &Scene, origin: Vec3, dir: Vec3, max_dist: f32, ignore: Option<EntityId>) -> Option<RayHit> {
    let dir = dir.normalized();
    let mut best: Option<RayHit> = None;
    for b in collect(scene) {
        if b.sensor || Some(b.id) == ignore {
            continue;
        }
        let hit = match b.shape {
            WShape::Aabb { min, max } => ray_box(origin, dir, min, max),
            WShape::Sphere { c, r } => {
                let oc = origin - c;
                let bq = oc.dot(dir);
                let cq = oc.dot(oc) - r * r;
                let disc = bq * bq - cq;
                if disc < 0.0 {
                    None
                } else {
                    let t = -bq - disc.sqrt();
                    let t = if t < 0.0 { -bq + disc.sqrt() } else { t };
                    if t < 0.0 {
                        None
                    } else {
                        let p = origin + dir * t;
                        Some((t, (p - c).normalized()))
                    }
                }
            }
        };
        if let Some((t, n)) = hit {
            if t <= max_dist && best.map(|h| t < h.distance).unwrap_or(true) {
                best = Some(RayHit { entity: b.id, point: origin + dir * t, normal: n, distance: t });
            }
        }
    }
    best
}

fn ray_box(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<(f32, Vec3)> {
    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    let mut n = Vec3::ZERO;
    for i in 0..3 {
        let (oi, di) = (o.get(i), d.get(i));
        if di.abs() < 1e-9 {
            if oi < lo.get(i) || oi > hi.get(i) {
                return None;
            }
            continue;
        }
        let mut t1 = (lo.get(i) - oi) / di;
        let mut t2 = (hi.get(i) - oi) / di;
        let mut sign = -1.0;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
            sign = 1.0;
        }
        if t1 > tmin {
            tmin = t1;
            n = Vec3::ZERO;
            n.set(i, sign);
        }
        tmax = tmax.min(t2);
    }
    if tmax < tmin.max(0.0) {
        return None;
    }
    if tmin < 0.0 {
        return Some((0.0, -d));
    }
    Some((tmin, n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grodot_core::math::vec3;
    use grodot_core::FieldValue;

    fn world_with_box_on_floor(height: f32) -> World {
        let mut s = Scene::new("p");
        let floor = s.add_entity("Floor");
        let e = s.entity_mut(floor).unwrap();
        let mut col = Component::new_default("Collider").unwrap();
        col.set_field("size", FieldValue::Vec3(vec3(20.0, 1.0, 20.0))).unwrap();
        e.add(col);
        e.transform_mut().position = vec3(0.0, -0.5, 0.0);
        let b = s.add_entity("Box");
        let e = s.entity_mut(b).unwrap();
        e.add(Component::new_default("Collider").unwrap());
        e.add(Component::new_default("RigidBody").unwrap());
        e.transform_mut().position = vec3(0.0, height, 0.0);
        World::new(s, None, Default::default())
    }

    #[test]
    fn box_falls_and_rests_on_floor() {
        let mut w = world_with_box_on_floor(3.0);
        for _ in 0..240 {
            step(&mut w, 1.0 / 60.0);
        }
        let y = w.scene.find_by_name("Box").unwrap().transform().position.y;
        assert!((y - 0.5).abs() < 0.05, "box y = {}", y);
        assert!(w.grounded.contains(&2));
        // One enter event between floor and box, no exit while resting.
        let enters = w.collision_events.iter().filter(|e| e.2 == CollisionKind::Enter).count();
        let exits = w.collision_events.iter().filter(|e| e.2 == CollisionKind::Exit).count();
        assert_eq!((enters, exits), (1, 0), "{:?}", w.collision_events);
    }

    #[test]
    fn sensor_reports_but_does_not_block() {
        let mut w = world_with_box_on_floor(3.0);
        let s = w.scene.add_entity("Trigger");
        let e = w.scene.entity_mut(s).unwrap();
        let mut col = Component::new_default("Collider").unwrap();
        col.set_field("sensor", FieldValue::Bool(true)).unwrap();
        col.set_field("size", FieldValue::Vec3(vec3(2.0, 0.5, 2.0))).unwrap();
        e.add(col);
        e.transform_mut().position = vec3(0.0, 1.5, 0.0);
        for _ in 0..240 {
            step(&mut w, 1.0 / 60.0);
        }
        let y = w.scene.find_by_name("Box").unwrap().transform().position.y;
        assert!((y - 0.5).abs() < 0.05);
        assert!(w.collision_events.iter().any(|e| e == &(2, 3, CollisionKind::Enter)));
        assert!(w.collision_events.iter().any(|e| e == &(2, 3, CollisionKind::Exit)));
    }

    #[test]
    fn sphere_box_penetration() {
        let s = WShape::Sphere { c: vec3(0.0, 0.9, 0.0), r: 0.5 };
        let b = WShape::Aabb { min: vec3(-1.0, -1.0, -1.0), max: vec3(1.0, 0.5, 1.0) };
        let (n, d) = penetration(&s, &b).unwrap();
        assert!(n.y < -0.9 && (d - 0.1).abs() < 1e-4);
    }

    #[test]
    fn raycast_hits_floor() {
        let w = world_with_box_on_floor(5.0);
        let hit = raycast(&w.scene, vec3(3.0, 10.0, 0.0), vec3(0.0, -1.0, 0.0), 100.0, None).unwrap();
        assert_eq!(hit.entity, 1);
        assert!((hit.point.y - 0.0).abs() < 1e-4);
        assert!(hit.normal.y > 0.9);
        assert!(raycast(&w.scene, vec3(30.0, 10.0, 0.0), vec3(0.0, -1.0, 0.0), 100.0, None).is_none());
    }
}
