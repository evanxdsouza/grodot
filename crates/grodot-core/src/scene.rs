//! The scene model shared by the editor and the runtime.
//!
//! A scene is a list of entities. Each entity has a stable id, a name, an
//! optional parent and a list of components. Scenes are stored as versioned
//! JSON (`.gscene`); prefabs (`.gprefab`) use the same format.

use crate::json::{self, Json};
use crate::math::{Color, Mat4, Quat, Vec3};

pub const SCENE_VERSION: u32 = 1;

pub type EntityId = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Transform { position: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE }
    }
}

impl Transform {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_trs(self.position, self.rotation, self.scale)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshRenderer {
    /// A primitive (`cube`, `sphere`, `plane`, `cylinder`, `capsule`) or a
    /// project-relative `.obj` path.
    pub mesh: String,
    pub color: Color,
    /// Unlit meshes ignore lighting (good for UI-like markers and skies).
    pub lit: bool,
    pub visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyType {
    Static,
    Dynamic,
    Kinematic,
}

impl BodyType {
    pub fn name(&self) -> &'static str {
        match self {
            BodyType::Static => "static",
            BodyType::Dynamic => "dynamic",
            BodyType::Kinematic => "kinematic",
        }
    }
    pub fn parse(s: &str) -> Option<BodyType> {
        match s {
            "static" => Some(BodyType::Static),
            "dynamic" => Some(BodyType::Dynamic),
            "kinematic" => Some(BodyType::Kinematic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RigidBody {
    pub body_type: BodyType,
    pub mass: f32,
    pub velocity: Vec3,
    pub gravity: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Box,
    Sphere,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Collider {
    pub shape: Shape,
    /// Full box size (before the entity's scale).
    pub size: Vec3,
    pub radius: f32,
    pub offset: Vec3,
    /// Sensors detect overlaps but do not block movement.
    pub sensor: bool,
    pub friction: f32,
    pub restitution: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioSource {
    pub clip: String,
    pub volume: f32,
    pub looping: bool,
    pub autoplay: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub color: Color,
    pub intensity: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    Transform(Transform),
    Camera(Camera),
    MeshRenderer(MeshRenderer),
    RigidBody(RigidBody),
    Collider(Collider),
    AudioSource(AudioSource),
    Script(Script),
    Light(Light),
}

/// Every component type, in inspector order.
pub const COMPONENT_TYPES: [&str; 8] =
    ["Transform", "Camera", "MeshRenderer", "RigidBody", "Collider", "AudioSource", "Script", "Light"];

pub const PRIMITIVE_MESHES: [&str; 5] = ["cube", "sphere", "plane", "cylinder", "capsule"];

/// A reflected component field, used by the inspector and script bindings.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Float(f32),
    Bool(bool),
    Vec3(Vec3),
    Color(Color),
    Text(String),
    Choice(String, &'static [&'static str]),
}

impl Component {
    pub fn type_name(&self) -> &'static str {
        match self {
            Component::Transform(_) => "Transform",
            Component::Camera(_) => "Camera",
            Component::MeshRenderer(_) => "MeshRenderer",
            Component::RigidBody(_) => "RigidBody",
            Component::Collider(_) => "Collider",
            Component::AudioSource(_) => "AudioSource",
            Component::Script(_) => "Script",
            Component::Light(_) => "Light",
        }
    }

    /// A component with sensible defaults.
    pub fn new_default(type_name: &str) -> Option<Component> {
        Some(match type_name {
            "Transform" => Component::Transform(Transform::default()),
            "Camera" => Component::Camera(Camera { fov: 70.0, near: 0.1, far: 500.0, active: true }),
            "MeshRenderer" => Component::MeshRenderer(MeshRenderer {
                mesh: "cube".into(),
                color: Color::rgb(0.8, 0.8, 0.8),
                lit: true,
                visible: true,
            }),
            "RigidBody" => Component::RigidBody(RigidBody {
                body_type: BodyType::Dynamic,
                mass: 1.0,
                velocity: Vec3::ZERO,
                gravity: true,
            }),
            "Collider" => Component::Collider(Collider {
                shape: Shape::Box,
                size: Vec3::ONE,
                radius: 0.5,
                offset: Vec3::ZERO,
                sensor: false,
                friction: 0.5,
                restitution: 0.0,
            }),
            "AudioSource" => Component::AudioSource(AudioSource {
                clip: String::new(),
                volume: 1.0,
                looping: false,
                autoplay: false,
            }),
            "Script" => Component::Script(Script { path: String::new(), enabled: true }),
            "Light" => Component::Light(Light { color: Color::WHITE, intensity: 1.0 }),
            _ => return None,
        })
    }

    /// The editable fields of this component (name, value).
    pub fn fields(&self) -> Vec<(&'static str, FieldValue)> {
        use FieldValue as F;
        match self {
            Component::Transform(t) => vec![
                ("position", F::Vec3(t.position)),
                ("rotation", F::Vec3(t.rotation.to_euler_deg())),
                ("scale", F::Vec3(t.scale)),
            ],
            Component::Camera(c) => vec![
                ("field_of_view", F::Float(c.fov)),
                ("near_clip", F::Float(c.near)),
                ("far_clip", F::Float(c.far)),
                ("active", F::Bool(c.active)),
            ],
            Component::MeshRenderer(m) => vec![
                ("mesh", F::Text(m.mesh.clone())),
                ("color", F::Color(m.color)),
                ("lit", F::Bool(m.lit)),
                ("visible", F::Bool(m.visible)),
            ],
            Component::RigidBody(b) => vec![
                ("body_type", F::Choice(b.body_type.name().into(), &["static", "dynamic", "kinematic"])),
                ("mass", F::Float(b.mass)),
                ("velocity", F::Vec3(b.velocity)),
                ("gravity", F::Bool(b.gravity)),
            ],
            Component::Collider(c) => vec![
                (
                    "shape",
                    F::Choice(if c.shape == Shape::Box { "box" } else { "sphere" }.into(), &["box", "sphere"]),
                ),
                ("size", F::Vec3(c.size)),
                ("radius", F::Float(c.radius)),
                ("offset", F::Vec3(c.offset)),
                ("sensor", F::Bool(c.sensor)),
                ("friction", F::Float(c.friction)),
                ("restitution", F::Float(c.restitution)),
            ],
            Component::AudioSource(a) => vec![
                ("clip", F::Text(a.clip.clone())),
                ("volume", F::Float(a.volume)),
                ("looping", F::Bool(a.looping)),
                ("autoplay", F::Bool(a.autoplay)),
            ],
            Component::Script(s) => vec![("path", F::Text(s.path.clone())), ("enabled", F::Bool(s.enabled))],
            Component::Light(l) => vec![("color", F::Color(l.color)), ("intensity", F::Float(l.intensity))],
        }
    }

    /// Set a reflected field. Returns an error message for unknown fields or wrong types.
    pub fn set_field(&mut self, name: &str, v: FieldValue) -> Result<(), String> {
        use FieldValue as F;
        let type_name = self.type_name();
        let bad = || Err(format!("{}.{}: wrong value type", type_name, name));
        let fnum: Result<f32, String> = match &v {
            F::Float(f) if f.is_finite() => Ok(*f),
            _ => Err(format!("{}.{}: expected a number", type_name, name)),
        };
        match (self, name) {
            (Component::Transform(t), "position") => match v {
                F::Vec3(p) if p.is_finite() => t.position = p,
                _ => return bad(),
            },
            (Component::Transform(t), "rotation") => match v {
                F::Vec3(e) if e.is_finite() => t.rotation = Quat::from_euler_deg(e),
                _ => return bad(),
            },
            (Component::Transform(t), "scale") => match v {
                F::Vec3(s) if s.is_finite() => t.scale = s,
                _ => return bad(),
            },
            (Component::Camera(c), "field_of_view") => c.fov = { let f = fnum?; f.clamp(1.0, 170.0) },
            (Component::Camera(c), "near_clip") => c.near = { let f = fnum?; f.max(0.001) },
            (Component::Camera(c), "far_clip") => c.far = { let f = fnum?; f.max(0.01) },
            (Component::Camera(c), "active") => match v {
                F::Bool(b) => c.active = b,
                _ => return bad(),
            },
            (Component::MeshRenderer(m), "mesh") => match v {
                F::Text(s) => m.mesh = s,
                _ => return bad(),
            },
            (Component::MeshRenderer(m), "color") => match v {
                F::Color(c) => m.color = c,
                _ => return bad(),
            },
            (Component::MeshRenderer(m), "lit") => match v {
                F::Bool(b) => m.lit = b,
                _ => return bad(),
            },
            (Component::MeshRenderer(m), "visible") => match v {
                F::Bool(b) => m.visible = b,
                _ => return bad(),
            },
            (Component::RigidBody(b), "body_type") => match v {
                F::Choice(s, _) | F::Text(s) => {
                    b.body_type = BodyType::parse(&s).ok_or_else(|| format!("unknown body type '{}'", s))?
                }
                _ => return bad(),
            },
            (Component::RigidBody(b), "mass") => b.mass = { let f = fnum?; f.max(0.001) },
            (Component::RigidBody(b), "velocity") => match v {
                F::Vec3(p) if p.is_finite() => b.velocity = p,
                _ => return bad(),
            },
            (Component::RigidBody(b), "gravity") => match v {
                F::Bool(g) => b.gravity = g,
                _ => return bad(),
            },
            (Component::Collider(c), "shape") => match v {
                F::Choice(s, _) | F::Text(s) => {
                    c.shape = match s.as_str() {
                        "box" => Shape::Box,
                        "sphere" => Shape::Sphere,
                        _ => return Err(format!("unknown collider shape '{}'", s)),
                    }
                }
                _ => return bad(),
            },
            (Component::Collider(c), "size") => match v {
                F::Vec3(p) if p.is_finite() => c.size = p.abs(),
                _ => return bad(),
            },
            (Component::Collider(c), "radius") => c.radius = { let f = fnum?; f.abs() },
            (Component::Collider(c), "offset") => match v {
                F::Vec3(p) if p.is_finite() => c.offset = p,
                _ => return bad(),
            },
            (Component::Collider(c), "sensor") => match v {
                F::Bool(b) => c.sensor = b,
                _ => return bad(),
            },
            (Component::Collider(c), "friction") => c.friction = { let f = fnum?; f.clamp(0.0, 1.0) },
            (Component::Collider(c), "restitution") => c.restitution = { let f = fnum?; f.clamp(0.0, 1.0) },
            (Component::AudioSource(a), "clip") => match v {
                F::Text(s) => a.clip = s,
                _ => return bad(),
            },
            (Component::AudioSource(a), "volume") => a.volume = { let f = fnum?; f.clamp(0.0, 4.0) },
            (Component::AudioSource(a), "looping") => match v {
                F::Bool(b) => a.looping = b,
                _ => return bad(),
            },
            (Component::AudioSource(a), "autoplay") => match v {
                F::Bool(b) => a.autoplay = b,
                _ => return bad(),
            },
            (Component::Script(s), "path") => match v {
                F::Text(p) => s.path = p,
                _ => return bad(),
            },
            (Component::Script(s), "enabled") => match v {
                F::Bool(b) => s.enabled = b,
                _ => return bad(),
            },
            (Component::Light(l), "color") => match v {
                F::Color(c) => l.color = c,
                _ => return bad(),
            },
            (Component::Light(l), "intensity") => l.intensity = { let f = fnum?; f.max(0.0) },
            (_, _) => return Err(format!("{} has no property '{}'", type_name, name)),
        }
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        let base = Json::obj().with("type", self.type_name());
        match self {
            Component::Transform(t) => base
                .with("position", t.position.to_array())
                .with("rotation", t.rotation.to_array())
                .with("scale", t.scale.to_array()),
            Component::Camera(c) => base
                .with("fov", c.fov)
                .with("near", c.near)
                .with("far", c.far)
                .with("active", c.active),
            Component::MeshRenderer(m) => base
                .with("mesh", m.mesh.as_str())
                .with("color", m.color.to_hex())
                .with("lit", m.lit)
                .with("visible", m.visible),
            Component::RigidBody(b) => base
                .with("body_type", b.body_type.name())
                .with("mass", b.mass)
                .with("velocity", b.velocity.to_array())
                .with("gravity", b.gravity),
            Component::Collider(c) => base
                .with("shape", if c.shape == Shape::Box { "box" } else { "sphere" })
                .with("size", c.size.to_array())
                .with("radius", c.radius)
                .with("offset", c.offset.to_array())
                .with("sensor", c.sensor)
                .with("friction", c.friction)
                .with("restitution", c.restitution),
            Component::AudioSource(a) => base
                .with("clip", a.clip.as_str())
                .with("volume", a.volume)
                .with("looping", a.looping)
                .with("autoplay", a.autoplay),
            Component::Script(s) => base.with("path", s.path.as_str()).with("enabled", s.enabled),
            Component::Light(l) => base.with("color", l.color.to_hex()).with("intensity", l.intensity),
        }
    }

    pub fn from_json(j: &Json) -> Result<Component, String> {
        let ty = j.get("type").and_then(|t| t.as_str()).ok_or("component is missing \"type\"")?;
        let mut c = Component::new_default(ty).ok_or_else(|| {
            format!("unknown component type \"{}\" (known: {})", ty, COMPONENT_TYPES.join(", "))
        })?;
        let f32_of = |key: &str| -> Result<Option<f32>, String> {
            match j.get(key) {
                None => Ok(None),
                Some(Json::Num(n)) => Ok(Some(*n as f32)),
                Some(_) => Err(format!("{}.{} must be a number", ty, key)),
            }
        };
        let bool_of = |key: &str| -> Result<Option<bool>, String> {
            match j.get(key) {
                None => Ok(None),
                Some(Json::Bool(b)) => Ok(Some(*b)),
                Some(_) => Err(format!("{}.{} must be true or false", ty, key)),
            }
        };
        let str_of = |key: &str| -> Result<Option<String>, String> {
            match j.get(key) {
                None => Ok(None),
                Some(Json::Str(s)) => Ok(Some(s.clone())),
                Some(_) => Err(format!("{}.{} must be a string", ty, key)),
            }
        };
        let arr_of = |key: &str, n: usize| -> Result<Option<Vec<f32>>, String> {
            match j.get(key) {
                None => Ok(None),
                Some(Json::Arr(a)) if a.len() == n => {
                    let mut out = Vec::new();
                    for v in a {
                        out.push(v.as_f64().ok_or_else(|| format!("{}.{} must contain numbers", ty, key))? as f32);
                    }
                    Ok(Some(out))
                }
                Some(_) => Err(format!("{}.{} must be an array of {} numbers", ty, key, n)),
            }
        };
        let vec_of = |key: &str| -> Result<Option<Vec3>, String> {
            Ok(arr_of(key, 3)?.map(|a| Vec3::from_array([a[0], a[1], a[2]])))
        };
        let color_of = |key: &str| -> Result<Option<Color>, String> {
            match str_of(key)? {
                None => Ok(None),
                Some(s) => Color::from_hex(&s)
                    .map(Some)
                    .ok_or_else(|| format!("{}.{} must be a hex colour like \"#ff8800\"", ty, key)),
            }
        };
        match &mut c {
            Component::Transform(t) => {
                if let Some(v) = vec_of("position")? {
                    t.position = v;
                }
                if let Some(q) = arr_of("rotation", 4)? {
                    let q = Quat { x: q[0], y: q[1], z: q[2], w: q[3] };
                    let len2 = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
                    // Keep stored values bit-exact when they are already unit length.
                    t.rotation = if (len2 - 1.0).abs() > 1e-5 { q.normalized() } else { q };
                }
                if let Some(v) = vec_of("rotation_degrees")? {
                    t.rotation = Quat::from_euler_deg(v);
                }
                if let Some(v) = vec_of("scale")? {
                    t.scale = v;
                }
            }
            Component::Camera(cam) => {
                if let Some(v) = f32_of("fov")? {
                    cam.fov = v;
                }
                if let Some(v) = f32_of("near")? {
                    cam.near = v;
                }
                if let Some(v) = f32_of("far")? {
                    cam.far = v;
                }
                if let Some(v) = bool_of("active")? {
                    cam.active = v;
                }
            }
            Component::MeshRenderer(m) => {
                if let Some(v) = str_of("mesh")? {
                    m.mesh = v;
                }
                if let Some(v) = color_of("color")? {
                    m.color = v;
                }
                if let Some(v) = bool_of("lit")? {
                    m.lit = v;
                }
                if let Some(v) = bool_of("visible")? {
                    m.visible = v;
                }
            }
            Component::RigidBody(b) => {
                if let Some(v) = str_of("body_type")? {
                    b.body_type = BodyType::parse(&v)
                        .ok_or_else(|| format!("RigidBody.body_type must be static, dynamic or kinematic (got \"{}\")", v))?;
                }
                if let Some(v) = f32_of("mass")? {
                    b.mass = v.max(0.001);
                }
                if let Some(v) = vec_of("velocity")? {
                    b.velocity = v;
                }
                if let Some(v) = bool_of("gravity")? {
                    b.gravity = v;
                }
            }
            Component::Collider(col) => {
                if let Some(v) = str_of("shape")? {
                    col.shape = match v.as_str() {
                        "box" => Shape::Box,
                        "sphere" => Shape::Sphere,
                        _ => return Err(format!("Collider.shape must be box or sphere (got \"{}\")", v)),
                    };
                }
                if let Some(v) = vec_of("size")? {
                    col.size = v.abs();
                }
                if let Some(v) = f32_of("radius")? {
                    col.radius = v.abs();
                }
                if let Some(v) = vec_of("offset")? {
                    col.offset = v;
                }
                if let Some(v) = bool_of("sensor")? {
                    col.sensor = v;
                }
                if let Some(v) = f32_of("friction")? {
                    col.friction = v;
                }
                if let Some(v) = f32_of("restitution")? {
                    col.restitution = v;
                }
            }
            Component::AudioSource(a) => {
                if let Some(v) = str_of("clip")? {
                    a.clip = v;
                }
                if let Some(v) = f32_of("volume")? {
                    a.volume = v;
                }
                if let Some(v) = bool_of("looping")? {
                    a.looping = v;
                }
                if let Some(v) = bool_of("autoplay")? {
                    a.autoplay = v;
                }
            }
            Component::Script(s) => {
                if let Some(v) = str_of("path")? {
                    s.path = v;
                }
                if let Some(v) = bool_of("enabled")? {
                    s.enabled = v;
                }
            }
            Component::Light(l) => {
                if let Some(v) = color_of("color")? {
                    l.color = v;
                }
                if let Some(v) = f32_of("intensity")? {
                    l.intensity = v;
                }
            }
        }
        Ok(c)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub parent: Option<EntityId>,
    pub active: bool,
    pub tag: String,
    pub components: Vec<Component>,
}

impl Entity {
    pub fn get(&self, type_name: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.type_name() == type_name)
    }
    pub fn get_mut(&mut self, type_name: &str) -> Option<&mut Component> {
        self.components.iter_mut().find(|c| c.type_name() == type_name)
    }
    pub fn has(&self, type_name: &str) -> bool {
        self.get(type_name).is_some()
    }
    pub fn transform(&self) -> Transform {
        match self.get("Transform") {
            Some(Component::Transform(t)) => t.clone(),
            _ => Transform::default(),
        }
    }
    pub fn transform_mut(&mut self) -> &mut Transform {
        if !self.has("Transform") {
            self.components.insert(0, Component::Transform(Transform::default()));
        }
        match self.get_mut("Transform") {
            Some(Component::Transform(t)) => t,
            _ => unreachable!(),
        }
    }
    /// Add a component; returns false if one of that type already exists.
    pub fn add(&mut self, c: Component) -> bool {
        if self.has(c.type_name()) {
            return false;
        }
        self.components.push(c);
        true
    }
    pub fn remove(&mut self, type_name: &str) -> bool {
        let n = self.components.len();
        self.components.retain(|c| c.type_name() != type_name);
        n != self.components.len()
    }
    pub fn mesh(&self) -> Option<&MeshRenderer> {
        match self.get("MeshRenderer") {
            Some(Component::MeshRenderer(m)) => Some(m),
            _ => None,
        }
    }
    pub fn camera(&self) -> Option<&Camera> {
        match self.get("Camera") {
            Some(Component::Camera(c)) => Some(c),
            _ => None,
        }
    }
    pub fn script(&self) -> Option<&Script> {
        match self.get("Script") {
            Some(Component::Script(s)) => Some(s),
            _ => None,
        }
    }
    pub fn rigid_body(&self) -> Option<&RigidBody> {
        match self.get("RigidBody") {
            Some(Component::RigidBody(b)) => Some(b),
            _ => None,
        }
    }
    pub fn collider(&self) -> Option<&Collider> {
        match self.get("Collider") {
            Some(Component::Collider(c)) => Some(c),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    pub background: Color,
    pub ambient: f32,
}

impl Default for Environment {
    fn default() -> Self {
        Environment { background: Color::rgb(0.45, 0.62, 0.85), ambient: 0.35 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub name: String,
    pub environment: Environment,
    pub entities: Vec<Entity>,
    pub next_id: EntityId,
}

impl Scene {
    pub fn new(name: &str) -> Scene {
        Scene { name: name.into(), environment: Environment::default(), entities: Vec::new(), next_id: 1 }
    }

    /// Create an entity with a Transform and return its id.
    pub fn add_entity(&mut self, name: &str) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        self.entities.push(Entity {
            id,
            name: name.into(),
            parent: None,
            active: true,
            tag: String::new(),
            components: vec![Component::Transform(Transform::default())],
        });
        id
    }

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.id == id)
    }

    pub fn index_of(&self, id: EntityId) -> Option<usize> {
        self.entities.iter().position(|e| e.id == id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.name == name)
    }

    pub fn children(&self, id: EntityId) -> Vec<EntityId> {
        self.entities.iter().filter(|e| e.parent == Some(id)).map(|e| e.id).collect()
    }

    /// The entity and all of its descendants.
    pub fn subtree(&self, id: EntityId) -> Vec<EntityId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let c = self.children(out[i]);
            out.extend(c);
            i += 1;
        }
        out
    }

    /// Remove an entity and its descendants. Returns the removed ids.
    pub fn remove_entity(&mut self, id: EntityId) -> Vec<EntityId> {
        let ids = self.subtree(id);
        self.entities.retain(|e| !ids.contains(&e.id));
        ids
    }

    /// Set the parent, refusing cycles.
    pub fn set_parent(&mut self, id: EntityId, parent: Option<EntityId>) -> Result<(), String> {
        if let Some(p) = parent {
            if self.subtree(id).contains(&p) {
                return Err("cannot parent an entity to itself or its descendant".into());
            }
            if self.entity(p).is_none() {
                return Err(format!("parent entity {} does not exist", p));
            }
        }
        let e = self.entity_mut(id).ok_or("entity not found")?;
        e.parent = parent;
        Ok(())
    }

    /// World matrix of an entity (parent transforms applied).
    pub fn world_matrix(&self, id: EntityId) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        let mut cur = self.entity(id);
        let mut guard = 0;
        while let Some(e) = cur {
            m = e.transform().matrix() * m;
            cur = e.parent.and_then(|p| self.entity(p));
            guard += 1;
            if guard > 256 {
                break;
            }
        }
        m
    }

    /// An entity is active only if it and all its ancestors are active.
    pub fn is_active_in_hierarchy(&self, id: EntityId) -> bool {
        let mut cur = self.entity(id);
        let mut guard = 0;
        while let Some(e) = cur {
            if !e.active {
                return false;
            }
            cur = e.parent.and_then(|p| self.entity(p));
            guard += 1;
            if guard > 256 {
                break;
            }
        }
        true
    }

    /// Copy an entity subtree (from another scene or this one) into this scene with
    /// fresh ids. Returns the id of the copied root.
    pub fn instantiate(&mut self, src: &Scene, root: EntityId, new_parent: Option<EntityId>) -> Option<EntityId> {
        let ids = src.subtree(root);
        let mut map = std::collections::HashMap::new();
        for old in &ids {
            map.insert(*old, self.next_id);
            self.next_id += 1;
        }
        for old in &ids {
            let mut e = src.entity(*old)?.clone();
            e.id = map[old];
            e.parent = if *old == root { new_parent } else { e.parent.and_then(|p| map.get(&p).copied()) };
            self.entities.push(e);
        }
        map.get(&root).copied()
    }

    pub fn to_json(&self) -> Json {
        let entities: Vec<Json> = self
            .entities
            .iter()
            .map(|e| {
                Json::obj()
                    .with("id", e.id)
                    .with("name", e.name.as_str())
                    .with("parent", e.parent.map(Json::from).unwrap_or(Json::Null))
                    .with("active", e.active)
                    .with("tag", e.tag.as_str())
                    .with("components", Json::Arr(e.components.iter().map(|c| c.to_json()).collect()))
            })
            .collect();
        Json::obj()
            .with("scene_version", SCENE_VERSION as u64)
            .with("name", self.name.as_str())
            .with(
                "environment",
                Json::obj()
                    .with("background", self.environment.background.to_hex())
                    .with("ambient", self.environment.ambient),
            )
            .with("entities", Json::Arr(entities))
    }

    pub fn to_string_pretty(&self) -> String {
        self.to_json().to_pretty()
    }

    pub fn from_json_str(src: &str) -> Result<Scene, String> {
        let j = json::parse(src).map_err(|e| format!("invalid JSON: {}", e))?;
        Scene::from_json(&j)
    }

    pub fn from_json(j: &Json) -> Result<Scene, String> {
        let version = j
            .get("scene_version")
            .and_then(|v| v.as_f64())
            .ok_or("missing \"scene_version\"")? as u32;
        if version > SCENE_VERSION {
            return Err(format!(
                "scene_version {} is newer than this Grodot supports ({})",
                version, SCENE_VERSION
            ));
        }
        let mut scene = Scene::new(j.get("name").and_then(|v| v.as_str()).unwrap_or("Scene"));
        if let Some(env) = j.get("environment") {
            if let Some(bg) = env.get("background").and_then(|v| v.as_str()) {
                scene.environment.background =
                    Color::from_hex(bg).ok_or_else(|| format!("environment.background: bad colour \"{}\"", bg))?;
            }
            if let Some(a) = env.get("ambient").and_then(|v| v.as_f64()) {
                scene.environment.ambient = a as f32;
            }
        }
        let ents = j.get("entities").and_then(|v| v.as_array()).ok_or("missing \"entities\" array")?;
        let mut max_id = 0;
        for (i, ej) in ents.iter().enumerate() {
            let name = ej.get("name").and_then(|v| v.as_str()).unwrap_or("Entity").to_string();
            let ctx = |m: String| format!("entity #{} (\"{}\"): {}", i, name, m);
            let id = ej
                .get("id")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| ctx("missing numeric \"id\"".into()))? as EntityId;
            if scene.entities.iter().any(|e| e.id == id) {
                return Err(ctx(format!("duplicate id {}", id)));
            }
            max_id = max_id.max(id);
            let parent = match ej.get("parent") {
                None | Some(Json::Null) => None,
                Some(Json::Num(n)) => Some(*n as EntityId),
                Some(_) => return Err(ctx("\"parent\" must be an id or null".into())),
            };
            let mut components = Vec::new();
            if let Some(cs) = ej.get("components") {
                let cs = cs.as_array().ok_or_else(|| ctx("\"components\" must be an array".into()))?;
                for cj in cs {
                    let c = Component::from_json(cj).map_err(ctx)?;
                    if components.iter().any(|x: &Component| x.type_name() == c.type_name()) {
                        return Err(ctx(format!("has two {} components", c.type_name())));
                    }
                    components.push(c);
                }
            }
            scene.entities.push(Entity {
                id,
                name,
                parent,
                active: ej.get("active").and_then(|v| v.as_bool()).unwrap_or(true),
                tag: ej.get("tag").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                components,
            });
        }
        // Validate parents (and drop dangling references with an error).
        for e in &scene.entities {
            if let Some(p) = e.parent {
                if scene.entity(p).is_none() {
                    return Err(format!("entity \"{}\" has unknown parent id {}", e.name, p));
                }
            }
        }
        for e in &scene.entities {
            let mut seen = 0;
            let mut cur = e.parent;
            while let Some(p) = cur {
                seen += 1;
                if seen > scene.entities.len() {
                    return Err(format!("entity \"{}\" is part of a parent cycle", e.name));
                }
                cur = scene.entity(p).and_then(|x| x.parent);
            }
        }
        scene.next_id = max_id + 1;
        Ok(scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec3;

    fn sample() -> Scene {
        let mut s = Scene::new("Main");
        let p = s.add_entity("Player");
        s.entity_mut(p).unwrap().transform_mut().position = vec3(0.0, 1.0, 0.0);
        s.entity_mut(p).unwrap().add(Component::new_default("RigidBody").unwrap());
        s.entity_mut(p).unwrap().tag = "player".into();
        let c = s.add_entity("Camera");
        s.entity_mut(c).unwrap().add(Component::new_default("Camera").unwrap());
        s.set_parent(c, Some(p)).unwrap();
        s
    }

    #[test]
    fn json_roundtrip() {
        let s = sample();
        let text = s.to_string_pretty();
        let back = Scene::from_json_str(&text).unwrap();
        assert_eq!(s.entities, back.entities);
        assert_eq!(back.to_string_pretty(), text);
        assert!(text.contains("\"scene_version\": 1"));
    }

    #[test]
    fn rejects_unknown_component_with_context() {
        let src = r#"{"scene_version":1,"entities":[{"id":1,"name":"X","components":[{"type":"Banana"}]}]}"#;
        let e = Scene::from_json_str(src).unwrap_err();
        assert!(e.contains("\"X\"") && e.contains("Banana"), "{}", e);
    }

    #[test]
    fn rejects_bad_field_type() {
        let src = r#"{"scene_version":1,"entities":[{"id":1,"name":"X","components":[{"type":"Camera","fov":"wide"}]}]}"#;
        let e = Scene::from_json_str(src).unwrap_err();
        assert!(e.contains("Camera.fov must be a number"), "{}", e);
    }

    #[test]
    fn rejects_future_version() {
        assert!(Scene::from_json_str(r#"{"scene_version":99,"entities":[]}"#).is_err());
    }

    #[test]
    fn parent_cycles_refused() {
        let mut s = sample();
        let p = s.find_by_name("Player").unwrap().id;
        let c = s.find_by_name("Camera").unwrap().id;
        assert!(s.set_parent(p, Some(c)).is_err());
    }

    #[test]
    fn world_matrix_uses_parent() {
        let s = sample();
        let c = s.find_by_name("Camera").unwrap().id;
        let pos = s.world_matrix(c).transform_point(Vec3::ZERO);
        assert!((pos - vec3(0.0, 1.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn remove_entity_removes_children() {
        let mut s = sample();
        let p = s.find_by_name("Player").unwrap().id;
        assert_eq!(s.remove_entity(p).len(), 2);
        assert!(s.entities.is_empty());
    }

    #[test]
    fn instantiate_assigns_fresh_ids() {
        let src = sample();
        let mut dst = sample();
        let root = src.find_by_name("Player").unwrap().id;
        let new_root = dst.instantiate(&src, root, None).unwrap();
        assert_eq!(dst.entities.len(), 4);
        assert_ne!(new_root, root);
        let cam = dst.entities.last().unwrap();
        assert_eq!(cam.parent, Some(new_root));
    }

    #[test]
    fn reflected_fields() {
        let mut c = Component::new_default("Camera").unwrap();
        c.set_field("field_of_view", FieldValue::Float(90.0)).unwrap();
        assert!(matches!(c.fields()[0].1, FieldValue::Float(f) if f == 90.0));
        assert!(c.set_field("nope", FieldValue::Float(1.0)).is_err());
        assert!(c.set_field("active", FieldValue::Float(1.0)).is_err());
    }
}
