//! Host bindings: how Groscript sees entities, components and engine modules.
//!
//! Groscript never touches engine internals directly. Entities are exposed as
//! [`EntityHandle`] host objects, components as [`ComponentRef`], and engine
//! services as native modules (`Input`, `Time`, `Scene`, `Audio`, ...).

use crate::physics;
use crate::world::{AudioCmd, LogLevel, SignalConnection, SignalEvent, World};
use grodot_core::math::Vec2;
use grodot_core::math::{Color, Quat, Vec3};
use grodot_core::scene::{BodyType, Component, EntityId, FieldValue, Scene, COMPONENT_TYPES};
use groscript::builtins::{num_arg, str_arg, vec3_arg};
use groscript::{ErrorKind, GroError, GroHash, GroResult, HostObject, Interpreter, Module, Rgba, Value, V2, V3};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

pub type WorldRef = Rc<RefCell<World>>;

fn engine_err<T>(msg: impl Into<String>) -> GroResult<T> {
    Err(GroError::new(ErrorKind::Engine, msg))
}

pub fn v3(v: Vec3) -> Value {
    Value::Vec3(V3::new(v.x as f64, v.y as f64, v.z as f64))
}

pub fn to_vec3(v: &V3) -> Vec3 {
    Vec3 { x: v.x as f32, y: v.y as f32, z: v.z as f32 }
}

pub fn color_value(c: Color) -> Value {
    Value::Color(Rgba { r: c.r as f64, g: c.g as f64, b: c.b as f64, a: c.a as f64 })
}

pub fn value_color(v: &Value) -> Option<Color> {
    match v {
        Value::Color(c) => Some(Color { r: c.r as f32, g: c.g as f32, b: c.b as f32, a: c.a as f32 }),
        Value::Str(s) => Color::from_hex(s),
        _ => None,
    }
}

pub fn field_to_value(f: FieldValue) -> Value {
    match f {
        FieldValue::Float(x) => Value::Number(x as f64),
        FieldValue::Bool(b) => Value::Bool(b),
        FieldValue::Vec3(v) => v3(v),
        FieldValue::Vec2(v) => Value::Vec2(V2 { x: v.x as f64, y: v.y as f64 }),
        FieldValue::Color(c) => color_value(c),
        FieldValue::Text(s) => Value::str(&s),
        FieldValue::Choice(s, _) => Value::str(&s),
    }
}

pub fn value_to_field(v: &Value, like: &FieldValue, name: &str) -> GroResult<FieldValue> {
    let bad = || GroError::new(ErrorKind::Type, format!("cannot set '{}' to a {}", name, v.type_name()));
    Ok(match (like, v) {
        (FieldValue::Float(_), Value::Number(n)) => FieldValue::Float(*n as f32),
        (FieldValue::Bool(_), Value::Bool(b)) => FieldValue::Bool(*b),
        (FieldValue::Vec3(_), Value::Vec3(p)) => FieldValue::Vec3(to_vec3(p)),
        (FieldValue::Vec2(_), Value::Vec2(p)) => FieldValue::Vec2(Vec2::new(p.x as f32, p.y as f32)),
        (FieldValue::Text(_), Value::Nil) => FieldValue::Text(String::new()),
        (FieldValue::Text(_), Value::Host(h)) if h.as_any().downcast_ref::<Resource>().is_some() => {
            FieldValue::Text(h.as_any().downcast_ref::<Resource>().unwrap().path.clone())
        }
        (FieldValue::Color(_), _) => FieldValue::Color(value_color(v).ok_or_else(bad)?),
        (FieldValue::Text(_), Value::Str(s)) => FieldValue::Text(s.to_string()),
        (FieldValue::Choice(_, opts), Value::Str(s) | Value::Symbol(s)) => FieldValue::Choice(s.to_string(), opts),
        _ => return Err(bad()),
    })
}

// ------------------------------------------------------------------ Entity

pub struct EntityHandle {
    pub id: EntityId,
    pub world: WorldRef,
}

pub fn entity_value(world: &WorldRef, id: EntityId) -> Value {
    Value::Host(Rc::new(EntityHandle { id, world: world.clone() }))
}

/// Extract an entity id from an Entity handle or a script object.
pub fn entity_id_of(v: &Value) -> Option<EntityId> {
    let h = v.host()?;
    h.as_any().downcast_ref::<EntityHandle>().map(|e| e.id)
}

impl EntityHandle {
    fn dead(&self) -> GroError {
        let w = self.world.borrow();
        GroError::new(
            ErrorKind::Engine,
            if w.scene.entity(self.id).is_some() {
                format!("entity {} is being destroyed", self.id)
            } else {
                format!("entity {} has been destroyed (check entity.valid?() before using it)", self.id)
            },
        )
    }

    fn alive(&self) -> bool {
        self.world.borrow().is_alive(self.id)
    }

    fn with<R>(&self, f: impl FnOnce(&mut World) -> R) -> GroResult<R> {
        if !self.alive() {
            return Err(self.dead());
        }
        Ok(f(&mut self.world.borrow_mut()))
    }

    fn component_name(arg: &Value) -> GroResult<String> {
        let name = match arg {
            Value::Str(s) | Value::Symbol(s) => s.to_string(),
            other => {
                return Err(GroError::new(
                    ErrorKind::Type,
                    format!("component name must be a String, got {}", other.type_name()),
                ))
            }
        };
        if !COMPONENT_TYPES.contains(&name.as_str()) {
            return engine_err(format!("unknown component '{}' (known: {})", name, COMPONENT_TYPES.join(", ")));
        }
        Ok(name)
    }

    fn resolve_node_path(&self, path: &str) -> GroResult<Option<EntityId>> {
        self.with(|w| {
            if path.is_empty() {
                return None;
            }
            let mut parts = path.split('/').filter(|part| !part.is_empty()).peekable();
            let mut current = if path.starts_with('/') {
                let first = parts.next()?;
                let root_name = if first == "root" { parts.next()? } else { first };
                w.scene.entities.iter().find(|e| e.parent.is_none() && e.name.as_str() == root_name).map(|e| e.id)?
            } else {
                self.id
            };
            for part in parts {
                current = match part {
                    "." => current,
                    ".." => w.scene.entity(current)?.parent?,
                    name => w
                        .scene
                        .children(current)
                        .into_iter()
                        .find(|child| w.scene.entity(*child).map(|e| e.name.as_str() == name).unwrap_or(false))?,
                };
            }
            Some(current)
        })
    }
}

impl HostObject for EntityHandle {
    fn type_name(&self) -> String {
        "Entity".into()
    }

    fn identity(&self) -> u64 {
        self.id
    }

    fn describe(&self) -> String {
        match self.world.borrow().scene.entity(self.id) {
            Some(e) => format!("#<Entity \"{}\" id={}>", e.name, self.id),
            None => format!("#<Entity (destroyed) id={}>", self.id),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn script_object(&self) -> Option<Value> {
        self.world.borrow().script_objects.get(&self.id).cloned()
    }

    fn get_property(&self, name: &str) -> Option<GroResult<Value>> {
        const PROPS: [&str; 14] = [
            "id",
            "name",
            "tag",
            "active",
            "position",
            "world_position",
            "rotation",
            "scale",
            "velocity",
            "parent",
            "color",
            "visible",
            "material",
            "animation",
        ];
        if !PROPS.contains(&name) {
            return None;
        }
        if name == "id" {
            return Some(Ok(Value::Number(self.id as f64)));
        }
        if !self.alive() {
            return Some(Err(self.dead()));
        }
        let w = self.world.borrow();
        let e = w.scene.entity(self.id).unwrap();
        Some(match name {
            "name" => Ok(Value::str(&e.name)),
            "tag" => Ok(Value::str(&e.tag)),
            "active" => Ok(Value::Bool(e.active)),
            "position" => Ok(v3(e.transform().position)),
            "world_position" => Ok(v3(w.scene.world_matrix(self.id).get_translation())),
            "rotation" => Ok(v3(e.transform().rotation.to_euler_deg())),
            "scale" => Ok(v3(e.transform().scale)),
            "velocity" => match e.rigid_body() {
                Some(rb) => Ok(v3(rb.velocity)),
                None => engine_err(format!("entity '{}' has no RigidBody, so it has no velocity", e.name)),
            },
            "parent" => Ok(match e.parent {
                Some(p) => entity_value(&self.world, p),
                None => Value::Nil,
            }),
            "color" => match e.mesh() {
                Some(m) => Ok(color_value(m.material.base_color)),
                None => engine_err(format!("entity '{}' has no MeshRenderer, so it has no color", e.name)),
            },
            "visible" => match e.mesh() {
                Some(m) => Ok(Value::Bool(m.visible)),
                None => engine_err(format!("entity '{}' has no MeshRenderer", e.name)),
            },
            "material" => match e.mesh() {
                Some(_) => Ok(Value::Host(Rc::new(ComponentRef::material(self.id, &self.world)))),
                None => engine_err(format!("entity '{}' has no MeshRenderer, so it has no material", e.name)),
            },
            "animation" => match e.has("AnimationPlayer") {
                true => Ok(Value::Host(Rc::new(ComponentRef::new(self.id, "AnimationPlayer", &self.world)))),
                false => engine_err(format!("entity '{}' has no AnimationPlayer component", e.name)),
            },
            _ => unreachable!(),
        })
    }

    fn set_property(&self, name: &str, value: Value) -> Option<GroResult<()>> {
        const PROPS: [&str; 10] = [
            "name",
            "tag",
            "active",
            "position",
            "world_position",
            "rotation",
            "scale",
            "velocity",
            "color",
            "visible",
        ];
        if !PROPS.contains(&name) {
            if name == "id" || name == "parent" || name == "material" || name == "animation" {
                return Some(engine_err(format!("entity.{} is read-only", name)));
            }
            return None;
        }
        if !self.alive() {
            return Some(Err(self.dead()));
        }
        let want_vec = |v: &Value| -> GroResult<Vec3> {
            match v {
                Value::Vec3(p) => {
                    let p = to_vec3(p);
                    if !p.is_finite() {
                        return engine_err(format!("entity.{} cannot be set to a non-finite vector", name));
                    }
                    Ok(p)
                }
                other => Err(GroError::new(
                    ErrorKind::Type,
                    format!("entity.{} must be a Vector3, got {}", name, other.type_name()),
                )),
            }
        };
        let mut w = self.world.borrow_mut();
        let parent_inv = w.scene.entity(self.id).and_then(|e| e.parent).map(|p| w.scene.world_matrix(p).inverse());
        let e = w.scene.entity_mut(self.id).unwrap();
        Some((|| {
            match name {
                "name" => e.name = value.to_display(),
                "tag" => e.tag = value.to_display(),
                "active" => e.active = value.truthy(),
                "position" => e.transform_mut().position = want_vec(&value)?,
                "world_position" => {
                    let p = want_vec(&value)?;
                    e.transform_mut().position = match parent_inv {
                        Some(inv) => inv.transform_point(p),
                        None => p,
                    };
                }
                "rotation" => e.transform_mut().rotation = Quat::from_euler_deg(want_vec(&value)?),
                "scale" => e.transform_mut().scale = want_vec(&value)?,
                "velocity" => {
                    let v = want_vec(&value)?;
                    match e.get_mut("RigidBody") {
                        Some(Component::RigidBody(rb)) => rb.velocity = v,
                        _ => return engine_err(format!("entity '{}' has no RigidBody, so it has no velocity", e.name)),
                    }
                }
                "color" => {
                    let c = value_color(&value).ok_or_else(|| {
                        GroError::new(ErrorKind::Type, "entity.color must be a Color (e.g. Color.new(1, 0, 0))")
                    })?;
                    match e.get_mut("MeshRenderer") {
                        Some(Component::MeshRenderer(m)) => m.material.base_color = c,
                        _ => return engine_err(format!("entity '{}' has no MeshRenderer", e.name)),
                    }
                }
                "visible" => match e.get_mut("MeshRenderer") {
                    Some(Component::MeshRenderer(m)) => m.visible = value.truthy(),
                    _ => return engine_err(format!("entity '{}' has no MeshRenderer", e.name)),
                },
                _ => unreachable!(),
            }
            Ok(())
        })())
    }

    fn call_method(&self, _interp: &mut Interpreter, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
        let argc = |n: usize| -> GroResult<()> {
            if args.len() != n {
                return Err(GroError::new(
                    ErrorKind::Type,
                    format!("{}() expects {} argument{}, got {}", name, n, if n == 1 { "" } else { "s" }, args.len()),
                ));
            }
            Ok(())
        };
        let r: GroResult<Value> = match name {
            "valid?" => Ok(Value::Bool(self.alive())),
            "translate" => (|| {
                argc(1)?;
                let d = to_vec3(&vec3_arg(args, 0, name)?);
                self.with(|w| w.scene.entity_mut(self.id).unwrap().transform_mut().position += d)?;
                Ok(Value::Nil)
            })(),
            "rotate" => (|| {
                argc(1)?;
                let d = to_vec3(&vec3_arg(args, 0, name)?);
                self.with(|w| {
                    let t = w.scene.entity_mut(self.id).unwrap().transform_mut();
                    t.rotation = (t.rotation * Quat::from_euler_deg(d)).normalized();
                })?;
                Ok(Value::Nil)
            })(),
            "look_at" => (|| {
                argc(1)?;
                let target = match &args[0] {
                    Value::Vec3(v) => to_vec3(v),
                    other => match entity_id_of(other) {
                        Some(id) => self.world.borrow().scene.world_matrix(id).get_translation(),
                        None => return Err(GroError::new(ErrorKind::Type, "look_at() needs a Vector3 or an Entity")),
                    },
                };
                self.with(|w| {
                    let pos = w.scene.world_matrix(self.id).get_translation();
                    let q = Quat::look_rotation(target - pos, Vec3::Y);
                    w.scene.entity_mut(self.id).unwrap().transform_mut().rotation = q;
                })?;
                Ok(Value::Nil)
            })(),
            "forward" | "right" | "up" => (|| {
                argc(0)?;
                let axis = match name {
                    "forward" => Vec3::FORWARD,
                    "right" => Vec3::X,
                    _ => Vec3::Y,
                };
                let v = self.with(|w| w.scene.world_matrix(self.id).transform_vector(axis).normalized())?;
                Ok(v3(v))
            })(),
            "destroy" | "queue_free" => (|| {
                argc(0)?;
                let mut w = self.world.borrow_mut();
                if w.scene.entity(self.id).is_some() && !w.pending_destroy.contains(&self.id) {
                    w.pending_destroy.push(self.id);
                }
                Ok(Value::Nil)
            })(),
            "connect" => (|| {
                if args.len() != 3 {
                    return Err(GroError::new(
                        ErrorKind::Type,
                        "connect() expects a signal, target entity and method name",
                    ));
                }
                let signal = str_arg(args, 0, name)?;
                let target = entity_id_of(&args[1])
                    .ok_or_else(|| GroError::new(ErrorKind::Type, "connect() target must be an Entity"))?;
                let method = str_arg(args, 2, name)?;
                if signal.trim().is_empty() || method.trim().is_empty() {
                    return engine_err("signal and method names cannot be empty");
                }
                let mut w = self.world.borrow_mut();
                if !w.is_alive(self.id) {
                    drop(w);
                    return Err(self.dead());
                }
                if !w.is_alive(target) {
                    return engine_err(format!("connect() target entity {} is not alive", target));
                }
                let connection = SignalConnection {
                    source: self.id,
                    signal: signal.to_string(),
                    target,
                    method: method.to_string(),
                };
                if w.signal_connections.contains(&connection) {
                    Ok(Value::Bool(false))
                } else {
                    w.signal_connections.push(connection);
                    Ok(Value::Bool(true))
                }
            })(),
            "disconnect" | "is_connected?" => (|| {
                if args.len() != 3 {
                    return Err(GroError::new(
                        ErrorKind::Type,
                        format!("{}() expects a signal, target and method", name),
                    ));
                }
                let signal = str_arg(args, 0, name)?;
                let target = entity_id_of(&args[1])
                    .ok_or_else(|| GroError::new(ErrorKind::Type, format!("{}() target must be an Entity", name)))?;
                let method = str_arg(args, 2, name)?;
                let mut w = self.world.borrow_mut();
                if !w.is_alive(self.id) {
                    drop(w);
                    return Err(self.dead());
                }
                let connection = SignalConnection {
                    source: self.id,
                    signal: signal.to_string(),
                    target,
                    method: method.to_string(),
                };
                if name == "is_connected?" {
                    Ok(Value::Bool(w.signal_connections.contains(&connection)))
                } else {
                    let count = w.signal_connections.len();
                    w.signal_connections.retain(|c| c != &connection);
                    Ok(Value::Bool(w.signal_connections.len() != count))
                }
            })(),
            "emit_signal" => (|| {
                if args.is_empty() {
                    return Err(GroError::new(ErrorKind::Type, "emit_signal() expects a signal name"));
                }
                let signal = str_arg(args, 0, name)?;
                if signal.trim().is_empty() {
                    return engine_err("signal name cannot be empty");
                }
                let mut w = self.world.borrow_mut();
                if !w.is_alive(self.id) {
                    drop(w);
                    return Err(self.dead());
                }
                w.signal_events.push_back(SignalEvent {
                    source: self.id,
                    signal: signal.to_string(),
                    args: args[1..].to_vec(),
                });
                Ok(Value::Nil)
            })(),
            "add_to_group" => (|| {
                argc(1)?;
                let group = str_arg(args, 0, name)?;
                if group.trim().is_empty() {
                    return engine_err("group name cannot be empty");
                }
                self.with(|w| Value::Bool(w.scene.entity_mut(self.id).unwrap().add_to_group(&group)))
            })(),
            "remove_from_group" => (|| {
                argc(1)?;
                let group = str_arg(args, 0, name)?;
                self.with(|w| Value::Bool(w.scene.entity_mut(self.id).unwrap().remove_from_group(&group)))
            })(),
            "is_in_group?" => (|| {
                argc(1)?;
                let group = str_arg(args, 0, name)?;
                self.with(|w| Value::Bool(w.scene.entity(self.id).unwrap().is_in_group(&group)))
            })(),
            "get_groups" => (|| {
                argc(0)?;
                self.with(|w| {
                    Value::array(w.scene.entity(self.id).unwrap().groups.iter().map(|g| Value::str(g)).collect())
                })
            })(),
            "has_component?" => (|| {
                argc(1)?;
                let n = Self::component_name(&args[0])?;
                self.with(|w| Value::Bool(w.scene.entity(self.id).unwrap().has(&n)))
            })(),
            "get_component" => (|| {
                argc(1)?;
                let n = Self::component_name(&args[0])?;
                let has = self.with(|w| w.scene.entity(self.id).unwrap().has(&n))?;
                Ok(if has { Value::Host(Rc::new(ComponentRef::new(self.id, &n, &self.world))) } else { Value::Nil })
            })(),
            "add_component" => (|| {
                argc(1)?;
                let n = Self::component_name(&args[0])?;
                let added =
                    self.with(|w| w.scene.entity_mut(self.id).unwrap().add(Component::new_default(&n).unwrap()))?;
                if !added {
                    return engine_err(format!("entity already has a {} component", n));
                }
                if n == "Script" {
                    return engine_err(
                        "scripts cannot be attached from scripts in v0.1; add the Script component in the editor",
                    );
                }
                Ok(Value::Host(Rc::new(ComponentRef::new(self.id, &n, &self.world))))
            })(),
            "remove_component" => (|| {
                argc(1)?;
                let n = Self::component_name(&args[0])?;
                if n == "Transform" || n == "Script" {
                    return engine_err(format!("the {} component cannot be removed at runtime", n));
                }
                self.with(|w| Value::Bool(w.scene.entity_mut(self.id).unwrap().remove(&n)))
            })(),
            "duplicate" => (|| {
                argc(0)?;
                let id = self.with(|w| {
                    let src = w.scene.clone();
                    let parent = src.entity(self.id).unwrap().parent;
                    let new_id = w.scene.instantiate(&src, self.id, parent).unwrap();
                    let ids = w.scene.subtree(new_id);
                    w.spawned.extend(ids);
                    new_id
                })?;
                Ok(entity_value(&self.world, id))
            })(),
            "apply_impulse" | "apply_force" => (|| {
                argc(1)?;
                let v = to_vec3(&vec3_arg(args, 0, name)?);
                let mut w = self.world.borrow_mut();
                if !w.is_alive(self.id) {
                    drop(w);
                    return Err(self.dead());
                }
                let e = w.scene.entity_mut(self.id).unwrap();
                let ename = e.name.clone();
                match e.get_mut("RigidBody") {
                    Some(Component::RigidBody(rb)) if rb.body_type == BodyType::Dynamic => {
                        if name == "apply_impulse" {
                            rb.velocity += v / rb.mass.max(0.001);
                        } else {
                            *w.forces.entry(self.id).or_insert(Vec3::ZERO) += v;
                        }
                        Ok(Value::Nil)
                    }
                    _ => engine_err(format!("{}() needs a dynamic RigidBody on '{}'", name, ename)),
                }
            })(),
            "on_floor?" => self.with(|w| Value::Bool(w.grounded.contains(&self.id))),
            "distance_to" => (|| {
                argc(1)?;
                let target = match &args[0] {
                    Value::Vec3(v) => to_vec3(v),
                    other => match entity_id_of(other) {
                        Some(id) => self.world.borrow().scene.world_matrix(id).get_translation(),
                        None => {
                            return Err(GroError::new(ErrorKind::Type, "distance_to() needs a Vector3 or an Entity"))
                        }
                    },
                };
                self.with(|w| Value::Number((w.scene.world_matrix(self.id).get_translation() - target).length() as f64))
            })(),
            "children" => self.with(|w| {
                let ids = w.scene.children(self.id);
                Value::array(ids.into_iter().map(|c| entity_value(&self.world, c)).collect())
            }),
            "get_parent" => self.with(|w| {
                w.scene
                    .entity(self.id)
                    .and_then(|e| e.parent)
                    .map(|id| entity_value(&self.world, id))
                    .unwrap_or(Value::Nil)
            }),
            "get_node" | "get_node_or_null" => (|| {
                argc(1)?;
                let path = str_arg(args, 0, name)?;
                match self.resolve_node_path(&path)? {
                    Some(id) => Ok(entity_value(&self.world, id)),
                    None if name == "get_node_or_null" => Ok(Value::Nil),
                    None => engine_err(format!("node path '{}' was not found from '{}'", path, self.id)),
                }
            })(),
            "find_child" => (|| {
                argc(1)?;
                let n = str_arg(args, 0, name)?;
                self.with(|w| {
                    w.scene
                        .subtree(self.id)
                        .into_iter()
                        .skip(1)
                        .find(|c| w.scene.entity(*c).map(|e| *e.name == *n).unwrap_or(false))
                        .map(|c| entity_value(&self.world, c))
                        .unwrap_or(Value::Nil)
                })
            })(),
            "play_sound" => (|| {
                let mut w = self.world.borrow_mut();
                let clip = match w.scene.entity(self.id).and_then(|e| e.get("AudioSource")) {
                    Some(Component::AudioSource(a)) => (a.clip.clone(), a.volume),
                    _ => return engine_err("play_sound() needs an AudioSource component"),
                };
                match w.resolve(&clip.0) {
                    Ok(path) if path.is_file() => {
                        w.audio_cmds.push(AudioCmd::Play { path, volume: clip.1 });
                        Ok(Value::Bool(true))
                    }
                    _ => {
                        w.log.warn(format!("audio clip '{}' not found", clip.0));
                        Ok(Value::Bool(false))
                    }
                }
            })(),
            _ => return None,
        };
        Some(r)
    }
}

// ------------------------------------------------------------------ Component

pub struct ComponentRef {
    pub entity: EntityId,
    pub type_name: String,
    pub world: WorldRef,
    /// When set, only these fields are visible (e.g. `entity.material` shows the
    /// material part of the MeshRenderer).
    pub only: Option<&'static [&'static str]>,
    /// Name scripts see (`MeshRenderer`, or `Material` for the material view).
    pub label: String,
}

impl ComponentRef {
    pub fn new(entity: EntityId, type_name: &str, world: &WorldRef) -> ComponentRef {
        ComponentRef { entity, type_name: type_name.into(), world: world.clone(), only: None, label: type_name.into() }
    }

    pub fn material(entity: EntityId, world: &WorldRef) -> ComponentRef {
        ComponentRef {
            entity,
            type_name: "MeshRenderer".into(),
            world: world.clone(),
            only: Some(grodot_core::scene::MATERIAL_FIELDS),
            label: "Material".into(),
        }
    }

    /// Run `f` on this entity's animation state.
    fn with_anim<R>(&self, f: impl FnOnce(&mut crate::animation::AnimState) -> GroResult<R>) -> GroResult<R> {
        let mut w = self.world.borrow_mut();
        let name = w.scene.entity(self.entity).map(|e| e.name.clone()).unwrap_or_default();
        match w.animations.get_mut(&self.entity) {
            Some(st) => f(st),
            None => engine_err(format!(
                "'{}' has no animation to control (its MeshRenderer must show a .glb/.gltf model with animation clips)",
                name
            )),
        }
    }

    fn animation_method(&self, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
        let (looping, speed, crossfade) = {
            let w = self.world.borrow();
            match w.scene.entity(self.entity).and_then(|e| e.get("AnimationPlayer")) {
                Some(Component::AnimationPlayer(a)) => (a.looping, a.speed, a.crossfade),
                _ => return Some(Err(self.missing())),
            }
        };
        Some(match name {
            "play" => (|| {
                let clip = match args.first() {
                    Some(Value::Str(s)) | Some(Value::Symbol(s)) => s.to_string(),
                    _ => return engine_err("play() needs a clip name, e.g. animation.play(\"run\")"),
                };
                let fade = match args.get(1) {
                    None | Some(Value::Nil) => crossfade,
                    Some(Value::Number(n)) => (*n as f32).max(0.0),
                    Some(other) => {
                        return Err(GroError::new(
                            ErrorKind::Type,
                            format!("play() crossfade must be a Number of seconds, got {}", other.type_name()),
                        ))
                    }
                };
                let looping = match args.get(2) {
                    None | Some(Value::Nil) => looping,
                    Some(Value::Bool(b)) => *b,
                    Some(other) => {
                        return Err(GroError::new(
                            ErrorKind::Type,
                            format!("play() loop flag must be true or false, got {}", other.type_name()),
                        ))
                    }
                };
                self.with_anim(|st| match st.gltf.animation_index(&clip) {
                    Some(i) => {
                        st.play(i, fade, looping, speed);
                        Ok(Value::Bool(true))
                    }
                    None => engine_err(format!(
                        "no animation clip named '{}' (this model has: {})",
                        clip,
                        if st.gltf.animations.is_empty() { "no clips".to_string() } else { st.clip_names().join(", ") }
                    )),
                })
            })(),
            "stop" => self.with_anim(|st| {
                st.stop();
                Ok(Value::Nil)
            }),
            "pause" => self.with_anim(|st| {
                st.playing = false;
                Ok(Value::Nil)
            }),
            "resume" => self.with_anim(|st| {
                st.playing = st.clip.is_some();
                Ok(Value::Nil)
            }),
            "playing?" => self.with_anim(|st| Ok(Value::Bool(st.playing))),
            "clips" => self.with_anim(|st| Ok(Value::array(st.clip_names().iter().map(|c| Value::str(c)).collect()))),
            "has_clip?" => {
                let n = args.first().map(|v| v.to_display()).unwrap_or_default();
                self.with_anim(|st| Ok(Value::Bool(st.gltf.animation_index(&n).is_some())))
            }
            _ => return None,
        })
    }

    fn visible_field(&self, name: &str) -> bool {
        self.only.map(|o| o.contains(&name)).unwrap_or(true)
    }

    /// Check a texture path when a script assigns it, so the error points at that line.
    fn check_texture(&self, value: &Value) -> GroResult<()> {
        let path = match value {
            Value::Str(s) if !s.trim().is_empty() => s.trim().to_string(),
            Value::Host(h) => match h.as_any().downcast_ref::<Resource>() {
                Some(r) => r.path.clone(),
                None => return Ok(()),
            },
            _ => return Ok(()),
        };
        let lower = path.to_lowercase();
        if !(lower.ends_with(".png") || lower.ends_with(".bmp")) {
            return engine_err(format!("texture '{}' must be a .png or .bmp file", path));
        }
        let full = self.world.borrow().resolve(&path).map_err(|e| GroError::new(ErrorKind::Engine, e))?;
        if !full.is_file() {
            return engine_err(format!("texture '{}' not found (textures usually live in assets/textures/)", path));
        }
        Ok(())
    }

    fn missing(&self) -> GroError {
        GroError::new(
            ErrorKind::Engine,
            format!("the {} component was removed or its entity destroyed", self.type_name),
        )
    }
}

impl HostObject for ComponentRef {
    fn type_name(&self) -> String {
        self.label.clone()
    }
    fn identity(&self) -> u64 {
        self.entity
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn describe(&self) -> String {
        format!("#<{} on entity {}>", self.label, self.entity)
    }

    fn get_property(&self, name: &str) -> Option<GroResult<Value>> {
        if name == "entity" {
            return Some(Ok(entity_value(&self.world, self.entity)));
        }
        if self.type_name == "AnimationPlayer" && matches!(name, "current_clip" | "time" | "length") {
            return Some(self.with_anim(|st| {
                Ok(match name {
                    "current_clip" => st.clip_name().map(Value::str).unwrap_or(Value::Nil),
                    "time" => Value::Number(st.time as f64),
                    _ => Value::Number(st.duration() as f64),
                })
            }));
        }
        let w = self.world.borrow();
        let comp = match w.scene.entity(self.entity).and_then(|e| e.get(&self.type_name)) {
            Some(c) => c,
            None => return Some(Err(self.missing())),
        };
        if !self.visible_field(name) {
            return None;
        }
        comp.fields().into_iter().find(|(n, _)| *n == name).map(|(_, v)| {
            Ok(match (name, v) {
                // An unset texture reads as nil rather than "".
                ("texture" | "emissive_texture", FieldValue::Text(t)) if t.is_empty() => Value::Nil,
                (_, v) => field_to_value(v),
            })
        })
    }

    fn set_property(&self, name: &str, value: Value) -> Option<GroResult<()>> {
        if !self.visible_field(name) {
            return None;
        }
        if self.type_name == "AnimationPlayer" {
            match name {
                "time" => {
                    let t = match value {
                        Value::Number(n) => n as f32,
                        other => {
                            return Some(Err(GroError::new(
                                ErrorKind::Type,
                                format!("animation.time must be a Number, got {}", other.type_name()),
                            )))
                        }
                    };
                    return Some(self.with_anim(|st| {
                        st.time = t.clamp(0.0, st.duration());
                        Ok(())
                    }));
                }
                "current_clip" | "length" => {
                    return Some(engine_err(format!("animation.{} is read-only; use play(\"name\")", name)))
                }
                // Changing speed or looping also affects the clip that is playing now.
                "speed" | "looping" => {
                    let v = value.clone();
                    let _ = self.with_anim(|st| {
                        match (name, &v) {
                            ("speed", Value::Number(n)) => st.speed = *n as f32,
                            ("looping", b) => st.looping = b.truthy(),
                            _ => {}
                        }
                        Ok(())
                    });
                }
                _ => {}
            }
        }
        if name == "texture" || name == "emissive_texture" {
            if let Err(e) = self.check_texture(&value) {
                return Some(Err(e));
            }
        }
        let mut w = self.world.borrow_mut();
        let comp = match w.scene.entity_mut(self.entity).and_then(|e| e.get_mut(&self.type_name)) {
            Some(c) => c,
            None => return Some(Err(self.missing())),
        };
        let like = comp.fields().into_iter().find(|(n, _)| *n == name)?.1;
        Some(
            value_to_field(&value, &like, name)
                .and_then(|f| comp.set_field(name, f).map_err(|e| GroError::new(ErrorKind::Engine, e))),
        )
    }

    fn call_method(&self, _interp: &mut Interpreter, name: &str, args: &[Value]) -> Option<GroResult<Value>> {
        if self.type_name == "AnimationPlayer" {
            if let Some(r) = self.animation_method(name, args) {
                return Some(r);
            }
        }
        match name {
            "fields" => {
                let w = self.world.borrow();
                let comp = w.scene.entity(self.entity).and_then(|e| e.get(&self.type_name))?;
                Some(Ok(Value::array(
                    comp.fields()
                        .into_iter()
                        .filter(|(n, _)| self.visible_field(n))
                        .map(|(n, _)| Value::str(n))
                        .collect(),
                )))
            }
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ Resource

pub struct Resource {
    pub path: String,
    pub kind: &'static str,
}

impl HostObject for Resource {
    fn type_name(&self) -> String {
        "Resource".into()
    }
    fn identity(&self) -> u64 {
        // Stable per path.
        self.path.bytes().fold(1469598103934665603u64, |h, b| (h ^ b as u64).wrapping_mul(1099511628211))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn describe(&self) -> String {
        format!("#<Resource {} \"{}\">", self.kind, self.path)
    }
    fn get_property(&self, name: &str) -> Option<GroResult<Value>> {
        match name {
            "path" => Some(Ok(Value::str(&self.path))),
            "kind" => Some(Ok(Value::str(self.kind))),
            _ => None,
        }
    }
    fn set_property(&self, _name: &str, _value: Value) -> Option<GroResult<()>> {
        None
    }
    fn call_method(&self, _i: &mut Interpreter, _n: &str, _a: &[Value]) -> Option<GroResult<Value>> {
        None
    }
}

fn resource_kind(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".wav") {
        "audio"
    } else if lower.ends_with(".obj") {
        "mesh"
    } else if lower.ends_with(".gscene") || lower.ends_with(".gprefab") {
        "scene"
    } else if lower.ends_with(".gro") {
        "script"
    } else {
        "file"
    }
}

/// A path argument may be a String or a Resource.
fn path_arg(args: &[Value], i: usize, fname: &str) -> GroResult<String> {
    if let Some(Value::Host(h)) = args.get(i) {
        if let Some(r) = h.as_any().downcast_ref::<Resource>() {
            return Ok(r.path.clone());
        }
    }
    Ok(str_arg(args, i, fname)?.to_string())
}

// ------------------------------------------------------------------ Timers

pub struct Timer {
    pub id: u64,
    pub due: f64,
    pub interval: Option<f64>,
    pub callback: Value,
    pub receiver: Value,
    pub entity: Option<EntityId>,
}

#[derive(Default)]
pub struct Timers {
    pub list: Vec<Timer>,
    pub next_id: u64,
}

// ------------------------------------------------------------------ modules

pub fn install(interp: &mut Interpreter, world: &WorldRef, timers: &Rc<RefCell<Timers>>) {
    interp.define_native_class("Entity");

    // print() goes to the console.
    {
        let w = world.clone();
        interp.set_output(move |s| w.borrow_mut().log.info(s.to_string()));
    }

    // ---- Input
    let input = Module::new("Input");
    for (fname, kind) in [("down", 0), ("just_pressed", 1), ("just_released", 2)] {
        let w = world.clone();
        input.native(fname, 1, Some(1), move |_, a| {
            let action = str_arg(a, 0, fname)?;
            let w = w.borrow();
            let r = match kind {
                0 => w.input.action_down(&action),
                1 => w.input.action_pressed(&action),
                _ => w.input.action_released(&action),
            };
            match r {
                Some(b) => Ok(Value::Bool(b)),
                None => engine_err(format!(
                    "unknown input action '{}' (add it to the [input] section of grodot.toml, or use a key name like \"Space\")",
                    action
                )),
            }
        });
    }
    {
        let w = world.clone();
        input.native("axis", 2, Some(2), move |_, a| {
            let neg = str_arg(a, 0, "axis")?;
            let pos = str_arg(a, 1, "axis")?;
            let w = w.borrow();
            let n = w
                .input
                .action_down(&neg)
                .ok_or_else(|| GroError::new(ErrorKind::Engine, format!("unknown input action '{}'", neg)))?;
            let p = w
                .input
                .action_down(&pos)
                .ok_or_else(|| GroError::new(ErrorKind::Engine, format!("unknown input action '{}'", pos)))?;
            Ok(Value::Number((p as i32 - n as i32) as f64))
        });
    }
    {
        let w = world.clone();
        input.native("mouse_position", 0, Some(0), move |_, _| {
            let (x, y) = w.borrow().input.mouse_position;
            Ok(Value::Vec2(V2 { x: x as f64, y: y as f64 }))
        });
        let w = world.clone();
        input.native("mouse_delta", 0, Some(0), move |_, _| {
            let (x, y) = w.borrow().input.mouse_delta;
            Ok(Value::Vec2(V2 { x: x as f64, y: y as f64 }))
        });
        let w = world.clone();
        input.native("mouse_button_down", 1, Some(1), move |_, a| {
            let key = match &a[0] {
                Value::Number(n) => match *n as i32 {
                    0 => "MouseLeft",
                    1 => "MouseRight",
                    2 => "MouseMiddle",
                    _ => return engine_err("mouse buttons are 0 (left), 1 (right) and 2 (middle)"),
                },
                Value::Str(s) | Value::Symbol(s) => match s.as_ref() {
                    "left" => "MouseLeft",
                    "right" => "MouseRight",
                    "middle" => "MouseMiddle",
                    _ => return engine_err("mouse buttons are \"left\", \"right\" and \"middle\""),
                },
                other => {
                    return Err(GroError::new(
                        ErrorKind::Type,
                        format!("mouse button must be a String, got {}", other.type_name()),
                    ))
                }
            };
            Ok(Value::Bool(w.borrow().input.down.contains(key)))
        });
    }
    interp.add_module(input);

    // ---- Time
    let time = Module::new("Time");
    {
        let w = world.clone();
        time.native("delta", 0, Some(0), move |_, _| Ok(Value::Number(w.borrow().time.delta as f64)));
        let w = world.clone();
        time.native("elapsed", 0, Some(0), move |_, _| Ok(Value::Number(w.borrow().time.elapsed)));
        let w = world.clone();
        time.native("frame_count", 0, Some(0), move |_, _| Ok(Value::Number(w.borrow().time.frame as f64)));
        let w = world.clone();
        time.native("fps", 0, Some(0), move |_, _| Ok(Value::Number((w.borrow().time.fps as f64).round())));
        let w = world.clone();
        time.native("fixed_delta", 0, Some(0), move |_, _| Ok(Value::Number(w.borrow().time.fixed_delta as f64)));
    }
    interp.add_module(time);

    // ---- Debug
    let debug = Module::new("Debug");
    for (fname, level) in [("log", LogLevel::Info), ("warning", LogLevel::Warning), ("error", LogLevel::Error)] {
        let w = world.clone();
        debug.native(fname, 1, None, move |_, a| {
            let msg: Vec<String> = a.iter().map(|v| v.to_display()).collect();
            w.borrow_mut().log.push(level, msg.join(" "));
            Ok(Value::Nil)
        });
    }
    {
        let w = world.clone();
        debug.native("draw_line", 2, Some(3), move |_, a| {
            let p = to_vec3(&vec3_arg(a, 0, "Debug.draw_line")?);
            let q = to_vec3(&vec3_arg(a, 1, "Debug.draw_line")?);
            let c = a.get(2).and_then(value_color).unwrap_or(Color::rgb(1.0, 0.2, 0.2));
            w.borrow_mut().debug_lines.push((p, q, c));
            Ok(Value::Nil)
        });
        let w = world.clone();
        debug.native("draw_ray", 2, Some(3), move |_, a| {
            let p = to_vec3(&vec3_arg(a, 0, "Debug.draw_ray")?);
            let d = to_vec3(&vec3_arg(a, 1, "Debug.draw_ray")?);
            let c = a.get(2).and_then(value_color).unwrap_or(Color::rgb(0.2, 1.0, 0.2));
            w.borrow_mut().debug_lines.push((p, p + d, c));
            Ok(Value::Nil)
        });
    }
    interp.add_module(debug);

    // ---- Assets
    let assets = Module::new("Assets");
    {
        let w = world.clone();
        assets.native("exists?", 1, Some(1), move |_, a| {
            let p = str_arg(a, 0, "Assets.exists?")?;
            Ok(Value::Bool(w.borrow().resolve(&p).map(|p| p.exists()).unwrap_or(false)))
        });
        let w = world.clone();
        assets.native("load", 1, Some(1), move |_, a| {
            let p = str_arg(a, 0, "Assets.load")?;
            let full = w.borrow().resolve(&p).map_err(|e| GroError::new(ErrorKind::Engine, e))?;
            if !full.is_file() {
                return engine_err(format!("asset '{}' not found", p));
            }
            Ok(Value::Host(Rc::new(Resource { kind: resource_kind(&p), path: p.to_string() })))
        });
    }
    interp.add_module(assets);

    // ---- Audio
    let audio = Module::new("Audio");
    for (fname, music) in [("play", false), ("play_music", true)] {
        let w = world.clone();
        audio.native(fname, 1, Some(2), move |_, a| {
            let clip = path_arg(a, 0, fname)?;
            let volume = if a.len() > 1 { num_arg(a, 1, fname)? as f32 } else { 1.0 };
            let mut w = w.borrow_mut();
            match w.resolve(&clip) {
                Ok(path) if path.is_file() => {
                    w.audio_cmds.push(if music {
                        AudioCmd::PlayMusic { path, volume }
                    } else {
                        AudioCmd::Play { path, volume }
                    });
                    Ok(Value::Bool(true))
                }
                Ok(_) => {
                    w.log.warn(format!("Audio.{}: clip '{}' not found", fname, clip));
                    Ok(Value::Bool(false))
                }
                Err(e) => {
                    w.log.warn(format!("Audio.{}: {}", fname, e));
                    Ok(Value::Bool(false))
                }
            }
        });
    }
    {
        let w = world.clone();
        audio.native("stop_music", 0, Some(0), move |_, _| {
            w.borrow_mut().audio_cmds.push(AudioCmd::StopMusic);
            Ok(Value::Nil)
        });
        let w = world.clone();
        audio.native("set_volume", 1, Some(1), move |_, a| {
            let v = num_arg(a, 0, "Audio.set_volume")? as f32;
            w.borrow_mut().audio_cmds.push(AudioCmd::SetVolume(v.clamp(0.0, 2.0)));
            Ok(Value::Nil)
        });
    }
    interp.add_module(audio);

    // ---- Scene
    let scene = Module::new("Scene");
    {
        let w = world.clone();
        scene.native("find", 1, Some(1), move |_, a| {
            let name = str_arg(a, 0, "Scene.find")?;
            let wb = w.borrow();
            let found = wb.scene.entities.iter().find(|e| *e.name == *name && wb.is_alive(e.id)).map(|e| e.id);
            drop(wb);
            Ok(found.map(|id| entity_value(&w, id)).unwrap_or(Value::Nil))
        });
        let w = world.clone();
        scene.native("find_all", 1, Some(1), move |_, a| {
            let tag = str_arg(a, 0, "Scene.find_all")?;
            let wb = w.borrow();
            let ids: Vec<EntityId> = wb
                .scene
                .entities
                .iter()
                .filter(|e| (*e.tag == *tag || *e.name == *tag) && wb.is_alive(e.id))
                .map(|e| e.id)
                .collect();
            drop(wb);
            Ok(Value::array(ids.into_iter().map(|id| entity_value(&w, id)).collect()))
        });
        let w = world.clone();
        scene.native("all", 0, Some(0), move |_, _| {
            let wb = w.borrow();
            let ids: Vec<EntityId> = wb.scene.entities.iter().filter(|e| wb.is_alive(e.id)).map(|e| e.id).collect();
            drop(wb);
            Ok(Value::array(ids.into_iter().map(|id| entity_value(&w, id)).collect()))
        });
        let w = world.clone();
        scene.native("get_nodes_in_group", 1, Some(1), move |_, a| {
            let group = str_arg(a, 0, "Scene.get_nodes_in_group")?;
            let wb = w.borrow();
            let ids: Vec<EntityId> =
                wb.scene.entities.iter().filter(|e| e.is_in_group(&group) && wb.is_alive(e.id)).map(|e| e.id).collect();
            drop(wb);
            Ok(Value::array(ids.into_iter().map(|id| entity_value(&w, id)).collect()))
        });
        let w = world.clone();
        scene.native("create", 1, Some(2), move |_, a| {
            let name = str_arg(a, 0, "Scene.create")?;
            let pos = if a.len() > 1 { Some(to_vec3(&vec3_arg(a, 1, "Scene.create")?)) } else { None };
            let id = {
                let mut wb = w.borrow_mut();
                let id = wb.scene.add_entity(&name);
                if let Some(p) = pos {
                    wb.scene.entity_mut(id).unwrap().transform_mut().position = p;
                }
                id
            };
            Ok(entity_value(&w, id))
        });
        let w = world.clone();
        scene.native("spawn", 1, Some(2), move |_, a| {
            let path = path_arg(a, 0, "Scene.spawn")?;
            let pos = if a.len() > 1 { Some(to_vec3(&vec3_arg(a, 1, "Scene.spawn")?)) } else { None };
            let full = w.borrow().resolve(&path).map_err(|e| GroError::new(ErrorKind::Engine, e))?;
            let src = std::fs::read_to_string(&full)
                .map_err(|e| GroError::new(ErrorKind::Engine, format!("cannot load prefab '{}': {}", path, e)))?;
            let prefab = Scene::from_json_str(&src)
                .map_err(|e| GroError::new(ErrorKind::Engine, format!("prefab '{}': {}", path, e)))?;
            let roots: Vec<EntityId> = prefab.entities.iter().filter(|e| e.parent.is_none()).map(|e| e.id).collect();
            if roots.is_empty() {
                return engine_err(format!("prefab '{}' contains no entities", path));
            }
            let first = {
                let mut wb = w.borrow_mut();
                let mut first = None;
                for r in roots {
                    let new_id = wb.scene.instantiate(&prefab, r, None).unwrap();
                    let ids = wb.scene.subtree(new_id);
                    wb.spawned.extend(ids);
                    if first.is_none() {
                        first = Some(new_id);
                        if let Some(p) = pos {
                            wb.scene.entity_mut(new_id).unwrap().transform_mut().position = p;
                        }
                    }
                }
                first.unwrap()
            };
            Ok(entity_value(&w, first))
        });
        let w = world.clone();
        scene.native("load", 1, Some(1), move |_, a| {
            let path = path_arg(a, 0, "Scene.load")?;
            let full = w.borrow().resolve(&path).map_err(|e| GroError::new(ErrorKind::Engine, e))?;
            if !full.is_file() {
                return engine_err(format!("scene '{}' not found", path));
            }
            w.borrow_mut().pending_scene_load = Some(path);
            Ok(Value::Nil)
        });
        let w = world.clone();
        scene.native("reload", 0, Some(0), move |_, _| {
            let mut wb = w.borrow_mut();
            match wb.scene_path.clone() {
                Some(p) => {
                    wb.pending_scene_load = Some(p);
                    Ok(Value::Nil)
                }
                None => engine_err("this scene was not loaded from a file, so it cannot be reloaded"),
            }
        });
        let w = world.clone();
        scene.native("name", 0, Some(0), move |_, _| Ok(Value::str(&w.borrow().scene.name)));
        let w = world.clone();
        scene.native("raycast", 2, Some(3), move |it, a| {
            let o = to_vec3(&vec3_arg(a, 0, "Scene.raycast")?);
            let d = to_vec3(&vec3_arg(a, 1, "Scene.raycast")?);
            let max = if a.len() > 2 { num_arg(a, 2, "Scene.raycast")? as f32 } else { 1000.0 };
            let ignore = entity_id_of(&it.current_self());
            let hit = physics::raycast(&w.borrow().scene, o, d, max, ignore);
            Ok(match hit {
                None => Value::Nil,
                Some(h) => {
                    let mut m = GroHash::default();
                    m.insert(Value::sym("entity").hash_key().unwrap(), entity_value(&w, h.entity));
                    m.insert(Value::sym("point").hash_key().unwrap(), v3(h.point));
                    m.insert(Value::sym("normal").hash_key().unwrap(), v3(h.normal));
                    m.insert(Value::sym("distance").hash_key().unwrap(), Value::Number(h.distance as f64));
                    Value::hash(m)
                }
            })
        });
    }
    interp.add_module(scene);

    // ---- Timer
    let timer = Module::new("Timer");
    for (fname, repeat) in [("after", false), ("every", true)] {
        let w = world.clone();
        let t = timers.clone();
        timer.native(fname, 2, Some(2), move |it, a| {
            let secs = num_arg(a, 0, fname)?;
            if secs < 0.0 || (repeat && secs < 0.001) {
                return engine_err(format!("Timer.{} needs a positive number of seconds", fname));
            }
            let cb = a[1].clone();
            let receiver = it.current_self();
            match &cb {
                Value::Symbol(s) => {
                    if !matches!(receiver, Value::Object(_)) {
                        return engine_err(format!(
                            "Timer.{}(..., :{}) with a symbol only works inside a script method; pass a function instead",
                            fname, s
                        ));
                    }
                }
                Value::Function(_) | Value::Native(_) => {}
                other => {
                    return Err(GroError::new(
                        ErrorKind::Type,
                        format!("Timer.{} callback must be a :method_name symbol or a function, got {}", fname, other.type_name()),
                    ))
                }
            }
            let now = w.borrow().time.elapsed;
            let mut t = t.borrow_mut();
            t.next_id += 1;
            let id = t.next_id;
            let entity = entity_id_of(&receiver);
            t.list.push(Timer { id, due: now + secs, interval: if repeat { Some(secs) } else { None }, callback: cb, receiver, entity });
            Ok(Value::Number(id as f64))
        });
    }
    {
        let t = timers.clone();
        timer.native("cancel", 1, Some(1), move |_, a| {
            let id = num_arg(a, 0, "Timer.cancel")? as u64;
            let mut t = t.borrow_mut();
            let n = t.list.len();
            t.list.retain(|x| x.id != id);
            Ok(Value::Bool(n != t.list.len()))
        });
    }
    interp.add_module(timer);

    // ---- UI (immediate mode HUD)
    let ui = Module::new("UI");
    {
        let w = world.clone();
        ui.native("text", 1, None, move |_, a| {
            let parts: Vec<String> = a.iter().map(|v| v.to_display()).collect();
            w.borrow_mut().hud.lines.push(parts.join(" "));
            Ok(Value::Nil)
        });
        let w = world.clone();
        ui.native("message", 1, Some(2), move |_, a| {
            let mut wb = w.borrow_mut();
            wb.hud.message = Some(a[0].to_display());
            wb.hud.sub_message = a.get(1).map(|v| v.to_display());
            Ok(Value::Nil)
        });
    }
    interp.add_module(ui);

    // ---- Game
    let game = Module::new("Game");
    {
        let w = world.clone();
        game.native("quit", 0, Some(0), move |_, _| {
            w.borrow_mut().quit_requested = true;
            Ok(Value::Nil)
        });
    }
    interp.add_module(game);
}
