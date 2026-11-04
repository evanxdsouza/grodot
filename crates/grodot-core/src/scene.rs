//! The scene model shared by the editor and the runtime.
//!
//! A scene is a list of entities. Each entity has a stable id, a name, an
//! optional parent and a list of components. Scenes are stored as versioned
//! JSON (`.gscene`); prefabs (`.gprefab`) use the same format.

use crate::json::{self, Json};
use crate::math::{Color, Mat4, Quat, Vec2, Vec3};

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

/// Orthographic camera for 2D scenes. One world unit is `pixels_per_unit`
/// pixels at zoom 1; larger zoom values bring objects closer.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera2D {
    pub active: bool,
    pub zoom: f32,
    pub pixels_per_unit: f32,
}

/// A solid-colour 2D rectangle. Texture-backed sprites can use the same
/// transform and camera path once image assets are supported.
#[derive(Debug, Clone, PartialEq)]
pub struct Sprite2D {
    pub color: Color,
    pub size: crate::math::Vec3,
    pub visible: bool,
    pub z_index: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshRenderer {
    /// A primitive (`cube`, `sphere`, `plane`, `cylinder`, `capsule`) or a
    /// project-relative `.obj` path.
    pub mesh: String,
    pub material: Material,
    /// Unlit meshes ignore lighting (good for UI-like markers and skies).
    pub lit: bool,
    pub visible: bool,
    /// Block directional light (appear in shadow maps).
    pub cast_shadows: bool,
}

/// How a texture is sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    /// Blocky, no smoothing (pixel art).
    Nearest,
    /// Smooth between texels, no mipmaps.
    Bilinear,
    /// Smooth, with mipmaps blended for distant surfaces (the default).
    Trilinear,
}

/// What happens outside the 0..1 texture range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureWrap {
    Repeat,
    Clamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlphaMode {
    /// Alpha is ignored.
    Opaque,
    /// Pixels with alpha below `alpha_cutoff` are discarded (foliage, fences).
    Cutout,
    /// Alpha-blended, drawn after opaque surfaces from back to front (glass, water).
    Blend,
}

pub const FILTER_NAMES: &[&str] = &["nearest", "bilinear", "trilinear"];
pub const WRAP_NAMES: &[&str] = &["repeat", "clamp"];
pub const ALPHA_MODE_NAMES: &[&str] = &["opaque", "cutout", "blend"];

impl TextureFilter {
    pub fn name(self) -> &'static str {
        FILTER_NAMES[self as usize]
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "nearest" => TextureFilter::Nearest,
            "bilinear" => TextureFilter::Bilinear,
            "trilinear" => TextureFilter::Trilinear,
            _ => return None,
        })
    }
}

impl TextureWrap {
    pub fn name(self) -> &'static str {
        WRAP_NAMES[self as usize]
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "repeat" => TextureWrap::Repeat,
            "clamp" => TextureWrap::Clamp,
            _ => return None,
        })
    }
}

impl AlphaMode {
    pub fn name(self) -> &'static str {
        ALPHA_MODE_NAMES[self as usize]
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "opaque" => AlphaMode::Opaque,
            "cutout" => AlphaMode::Cutout,
            "blend" => AlphaMode::Blend,
            _ => return None,
        })
    }
}

/// Surface appearance of a mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    /// Multiplies the texture (or is the colour when there is no texture).
    pub base_color: Color,
    /// Project-relative `.png` or `.bmp`, usually in `assets/textures/`.
    pub texture: Option<String>,
    /// UV = mesh UV * uv_scale + uv_offset. Scale 4 repeats the texture 4 times.
    pub uv_scale: Vec2,
    pub uv_offset: Vec2,
    pub filter: TextureFilter,
    pub wrap: TextureWrap,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    /// Draw both sides of each triangle (needed for flat foliage cards).
    pub double_sided: bool,
    /// Light the surface gives off by itself, added after lighting (lamps, lava,
    /// screens). Black means no glow.
    pub emissive: Color,
    /// Optional texture multiplied by `emissive` (white parts glow fully).
    pub emissive_texture: Option<String>,
}

impl Default for Material {
    fn default() -> Self {
        Material {
            base_color: Color::rgb(0.8, 0.8, 0.8),
            texture: None,
            uv_scale: Vec2::ONE,
            uv_offset: Vec2::ZERO,
            filter: TextureFilter::Trilinear,
            wrap: TextureWrap::Repeat,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            emissive: Color::BLACK,
            emissive_texture: None,
        }
    }
}

/// Names of the MeshRenderer fields that belong to its material (used by `entity.material`).
pub const MATERIAL_FIELDS: &[&str] = &[
    "base_color",
    "texture",
    "uv_scale",
    "uv_offset",
    "filter",
    "wrap",
    "alpha_mode",
    "alpha_cutoff",
    "double_sided",
    "emissive",
    "emissive_texture",
];

impl Material {
    pub fn fields(&self) -> Vec<(&'static str, FieldValue)> {
        use FieldValue as F;
        vec![
            ("base_color", F::Color(self.base_color)),
            ("texture", F::Text(self.texture.clone().unwrap_or_default())),
            ("uv_scale", F::Vec2(self.uv_scale)),
            ("uv_offset", F::Vec2(self.uv_offset)),
            ("filter", F::Choice(self.filter.name().into(), FILTER_NAMES)),
            ("wrap", F::Choice(self.wrap.name().into(), WRAP_NAMES)),
            ("alpha_mode", F::Choice(self.alpha_mode.name().into(), ALPHA_MODE_NAMES)),
            ("alpha_cutoff", F::Float(self.alpha_cutoff)),
            ("double_sided", F::Bool(self.double_sided)),
            ("emissive", F::Color(self.emissive)),
            ("emissive_texture", F::Text(self.emissive_texture.clone().unwrap_or_default())),
        ]
    }

    /// Returns Ok(true) if the field belongs to the material and was set.
    pub fn set_field(&mut self, name: &str, v: FieldValue) -> Result<bool, String> {
        use FieldValue as F;
        let bad = |what: &str| Err(format!("material.{} must be {}", name, what));
        match (name, v) {
            ("base_color" | "color", F::Color(c)) => self.base_color = c,
            ("base_color" | "color", _) => return bad("a Color"),
            ("texture", F::Text(t)) => {
                let t = t.trim().to_string();
                self.texture = if t.is_empty() { None } else { Some(t) };
            }
            ("texture", _) => return bad("a texture path string (or \"\" for none)"),
            ("emissive_texture", F::Text(t)) => {
                let t = t.trim().to_string();
                self.emissive_texture = if t.is_empty() { None } else { Some(t) };
            }
            ("emissive_texture", _) => return bad("a texture path string (or \"\" for none)"),
            ("emissive", F::Color(c)) => self.emissive = c,
            ("emissive", _) => return bad("a Color"),
            ("uv_scale", F::Vec2(v)) if v.is_finite() => self.uv_scale = v,
            ("uv_offset", F::Vec2(v)) if v.is_finite() => self.uv_offset = v,
            ("uv_scale" | "uv_offset", _) => return bad("a Vector2"),
            ("filter", F::Choice(s, _) | F::Text(s)) => {
                self.filter = TextureFilter::parse(&s)
                    .ok_or_else(|| format!("filter must be one of {}", FILTER_NAMES.join(", ")))?
            }
            ("wrap", F::Choice(s, _) | F::Text(s)) => {
                self.wrap =
                    TextureWrap::parse(&s).ok_or_else(|| format!("wrap must be one of {}", WRAP_NAMES.join(", ")))?
            }
            ("alpha_mode", F::Choice(s, _) | F::Text(s)) => {
                self.alpha_mode = AlphaMode::parse(&s)
                    .ok_or_else(|| format!("alpha_mode must be one of {}", ALPHA_MODE_NAMES.join(", ")))?
            }
            ("filter" | "wrap" | "alpha_mode", _) => return bad("a string"),
            ("alpha_cutoff", F::Float(f)) if f.is_finite() => self.alpha_cutoff = f.clamp(0.0, 1.0),
            ("alpha_cutoff", _) => return bad("a number between 0 and 1"),
            ("double_sided", F::Bool(b)) => self.double_sided = b,
            ("double_sided", _) => return bad("true or false"),
            _ => return Ok(false),
        }
        Ok(true)
    }

    pub fn to_json(&self) -> Json {
        let mut j = Json::obj().with("base_color", self.base_color.to_hex());
        if let Some(t) = &self.texture {
            j = j.with("texture", t.as_str());
        }
        j = j
            .with("uv_scale", Json::Arr(vec![Json::from(self.uv_scale.x), Json::from(self.uv_scale.y)]))
            .with("uv_offset", Json::Arr(vec![Json::from(self.uv_offset.x), Json::from(self.uv_offset.y)]))
            .with("filter", self.filter.name())
            .with("wrap", self.wrap.name())
            .with("alpha_mode", self.alpha_mode.name())
            .with("alpha_cutoff", self.alpha_cutoff)
            .with("double_sided", self.double_sided);
        if self.emissive != Color::BLACK {
            j = j.with("emissive", self.emissive.to_hex());
        }
        if let Some(t) = &self.emissive_texture {
            j = j.with("emissive_texture", t.as_str());
        }
        j
    }

    pub fn from_json(j: &Json) -> Result<Material, String> {
        let mut m = Material::default();
        let ctx = |k: &str, what: &str| format!("MeshRenderer.material.{} must be {}", k, what);
        if let Some(v) = j.get("base_color") {
            m.base_color = v
                .as_str()
                .and_then(Color::from_hex)
                .ok_or_else(|| ctx("base_color", "a hex colour like \"#ff8800\""))?;
        }
        match j.get("texture") {
            None | Some(Json::Null) => {}
            Some(Json::Str(s)) if s.is_empty() => {}
            Some(Json::Str(s)) => m.texture = Some(s.clone()),
            Some(_) => return Err(ctx("texture", "a string")),
        }
        let vec2 = |k: &str| -> Result<Option<Vec2>, String> {
            match j.get(k) {
                None => Ok(None),
                Some(Json::Arr(a)) if a.len() == 2 => match (a[0].as_f64(), a[1].as_f64()) {
                    (Some(x), Some(y)) => Ok(Some(Vec2::new(x as f32, y as f32))),
                    _ => Err(ctx(k, "an array of 2 numbers")),
                },
                Some(_) => Err(ctx(k, "an array of 2 numbers")),
            }
        };
        if let Some(v) = vec2("uv_scale")? {
            m.uv_scale = v;
        }
        if let Some(v) = vec2("uv_offset")? {
            m.uv_offset = v;
        }
        let choice = |k: &str| j.get(k).map(|v| v.as_str().map(|s| s.to_string()));
        if let Some(v) = choice("filter") {
            m.filter = v
                .as_deref()
                .and_then(TextureFilter::parse)
                .ok_or_else(|| ctx("filter", "nearest, bilinear or trilinear"))?;
        }
        if let Some(v) = choice("wrap") {
            m.wrap = v.as_deref().and_then(TextureWrap::parse).ok_or_else(|| ctx("wrap", "repeat or clamp"))?;
        }
        if let Some(v) = choice("alpha_mode") {
            m.alpha_mode =
                v.as_deref().and_then(AlphaMode::parse).ok_or_else(|| ctx("alpha_mode", "opaque, cutout or blend"))?;
        }
        if let Some(v) = j.get("alpha_cutoff") {
            m.alpha_cutoff = v.as_f64().ok_or_else(|| ctx("alpha_cutoff", "a number"))? as f32;
        }
        if let Some(v) = j.get("double_sided") {
            m.double_sided = v.as_bool().ok_or_else(|| ctx("double_sided", "true or false"))?;
        }
        if let Some(v) = j.get("emissive") {
            m.emissive =
                v.as_str().and_then(Color::from_hex).ok_or_else(|| ctx("emissive", "a hex colour like \"#ff8800\""))?;
        }
        match j.get("emissive_texture") {
            None | Some(Json::Null) => {}
            Some(Json::Str(s)) if s.is_empty() => {}
            Some(Json::Str(s)) => m.emissive_texture = Some(s.clone()),
            Some(_) => return Err(ctx("emissive_texture", "a string")),
        }
        Ok(m)
    }
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
    pub kind: LightKind,
    pub color: Color,
    pub intensity: f32,
    /// Point and spot lights: distance (metres) at which the light fades to nothing.
    pub range: f32,
    /// Spot lights: full cone angle in degrees.
    pub spot_angle: f32,
    /// Directional lights: cast real shadows (a shadow map rendered each frame).
    pub shadows: bool,
}

impl Default for Light {
    fn default() -> Self {
        Light {
            kind: LightKind::Directional,
            color: Color::WHITE,
            intensity: 1.0,
            range: 10.0,
            spot_angle: 45.0,
            shadows: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightKind {
    /// Sunlight: parallel rays along the entity's forward (-Z) direction.
    Directional,
    /// A bulb: shines in every direction from the entity's position.
    Point,
    /// A torch: a cone along the entity's forward direction.
    Spot,
}

pub const LIGHT_KIND_NAMES: &[&str] = &["directional", "point", "spot"];

impl LightKind {
    pub fn name(self) -> &'static str {
        LIGHT_KIND_NAMES[self as usize]
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "directional" | "sun" => LightKind::Directional,
            "point" => LightKind::Point,
            "spot" => LightKind::Spot,
            _ => return None,
        })
    }
}

/// Marks a skinned glTF model as animated: its MeshRenderer's model is drawn
/// with the joints posed by the AnimationPlayer (or in bind pose without one).
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    /// Which skin of the model to use (most files have exactly one).
    pub skin: u32,
    /// Draw the joint hierarchy as lines (debugging aid, editor and game).
    pub show_bones: bool,
}

/// Plays the animation clips stored in the entity's glTF model.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationPlayer {
    /// Clip name to play when the scene starts (with `autoplay`).
    pub clip: String,
    pub autoplay: bool,
    pub looping: bool,
    /// Playback speed (1 = normal, negative plays backwards).
    pub speed: f32,
    /// Default crossfade time in seconds when switching clips.
    pub crossfade: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Component {
    Transform(Transform),
    Camera(Camera),
    Camera2D(Camera2D),
    Sprite2D(Sprite2D),
    MeshRenderer(MeshRenderer),
    RigidBody(RigidBody),
    Collider(Collider),
    AudioSource(AudioSource),
    Script(Script),
    Light(Light),
    Skeleton(Skeleton),
    AnimationPlayer(AnimationPlayer),
}

/// Every component type, in inspector order.
pub const COMPONENT_TYPES: [&str; 12] = [
    "Transform",
    "Camera",
    "Camera2D",
    "Sprite2D",
    "MeshRenderer",
    "RigidBody",
    "Collider",
    "AudioSource",
    "Script",
    "Light",
    "Skeleton",
    "AnimationPlayer",
];

pub const PRIMITIVE_MESHES: [&str; 6] = ["cube", "sphere", "plane", "quad", "cylinder", "capsule"];

/// A reflected component field, used by the inspector and script bindings.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Float(f32),
    Bool(bool),
    Vec3(Vec3),
    Color(Color),
    Vec2(Vec2),
    Text(String),
    Choice(String, &'static [&'static str]),
}

impl Component {
    pub fn type_name(&self) -> &'static str {
        match self {
            Component::Transform(_) => "Transform",
            Component::Camera(_) => "Camera",
            Component::Camera2D(_) => "Camera2D",
            Component::Sprite2D(_) => "Sprite2D",
            Component::MeshRenderer(_) => "MeshRenderer",
            Component::RigidBody(_) => "RigidBody",
            Component::Collider(_) => "Collider",
            Component::AudioSource(_) => "AudioSource",
            Component::Script(_) => "Script",
            Component::Light(_) => "Light",
            Component::Skeleton(_) => "Skeleton",
            Component::AnimationPlayer(_) => "AnimationPlayer",
        }
    }

    /// A component with sensible defaults.
    pub fn new_default(type_name: &str) -> Option<Component> {
        Some(match type_name {
            "Transform" => Component::Transform(Transform::default()),
            "Camera" => Component::Camera(Camera { fov: 70.0, near: 0.1, far: 500.0, active: true }),
            "Camera2D" => Component::Camera2D(Camera2D { active: true, zoom: 1.0, pixels_per_unit: 64.0 }),
            "Sprite2D" => Component::Sprite2D(Sprite2D {
                color: Color::WHITE,
                size: Vec3 { x: 1.0, y: 1.0, z: 1.0 },
                visible: true,
                z_index: 0,
            }),
            "MeshRenderer" => Component::MeshRenderer(MeshRenderer {
                mesh: "cube".into(),
                material: Material::default(),
                lit: true,
                visible: true,
                cast_shadows: true,
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
            "Light" => Component::Light(Light::default()),
            "Skeleton" => Component::Skeleton(Skeleton { skin: 0, show_bones: false }),
            "AnimationPlayer" => Component::AnimationPlayer(AnimationPlayer {
                clip: String::new(),
                autoplay: true,
                looping: true,
                speed: 1.0,
                crossfade: 0.2,
            }),
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
            Component::Camera2D(c) => vec![
                ("active", F::Bool(c.active)),
                ("zoom", F::Float(c.zoom)),
                ("pixels_per_unit", F::Float(c.pixels_per_unit)),
            ],
            Component::Sprite2D(s) => vec![
                ("color", F::Color(s.color)),
                ("size", F::Vec3(s.size)),
                ("visible", F::Bool(s.visible)),
                ("z_index", F::Float(s.z_index as f32)),
            ],
            Component::MeshRenderer(m) => {
                let mut f = vec![("mesh", F::Text(m.mesh.clone()))];
                f.extend(m.material.fields());
                f.push(("lit", F::Bool(m.lit)));
                f.push(("visible", F::Bool(m.visible)));
                f.push(("cast_shadows", F::Bool(m.cast_shadows)));
                f
            }
            Component::RigidBody(b) => vec![
                ("body_type", F::Choice(b.body_type.name().into(), &["static", "dynamic", "kinematic"])),
                ("mass", F::Float(b.mass)),
                ("velocity", F::Vec3(b.velocity)),
                ("gravity", F::Bool(b.gravity)),
            ],
            Component::Collider(c) => vec![
                ("shape", F::Choice(if c.shape == Shape::Box { "box" } else { "sphere" }.into(), &["box", "sphere"])),
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
            Component::Light(l) => vec![
                ("light_type", F::Choice(l.kind.name().into(), LIGHT_KIND_NAMES)),
                ("color", F::Color(l.color)),
                ("intensity", F::Float(l.intensity)),
                ("range", F::Float(l.range)),
                ("spot_angle", F::Float(l.spot_angle)),
                ("shadows", F::Bool(l.shadows)),
            ],
            Component::Skeleton(k) => vec![("skin", F::Float(k.skin as f32)), ("show_bones", F::Bool(k.show_bones))],
            Component::AnimationPlayer(a) => vec![
                ("clip", F::Text(a.clip.clone())),
                ("autoplay", F::Bool(a.autoplay)),
                ("looping", F::Bool(a.looping)),
                ("speed", F::Float(a.speed)),
                ("crossfade", F::Float(a.crossfade)),
            ],
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
            (Component::Camera(c), "field_of_view") => {
                c.fov = {
                    let f = fnum?;
                    f.clamp(1.0, 170.0)
                }
            }
            (Component::Camera(c), "near_clip") => {
                c.near = {
                    let f = fnum?;
                    f.max(0.001)
                }
            }
            (Component::Camera(c), "far_clip") => {
                c.far = {
                    let f = fnum?;
                    f.max(0.01)
                }
            }
            (Component::Camera(c), "active") => match v {
                F::Bool(b) => c.active = b,
                _ => return bad(),
            },
            (Component::Camera2D(c), "active") => match v {
                F::Bool(b) => c.active = b,
                _ => return bad(),
            },
            (Component::Camera2D(c), "zoom") => c.zoom = fnum?.clamp(0.01, 100.0),
            (Component::Camera2D(c), "pixels_per_unit") => c.pixels_per_unit = fnum?.clamp(1.0, 4096.0),
            (Component::Sprite2D(s), "color") => match v {
                F::Color(color) => s.color = color,
                _ => return bad(),
            },
            (Component::Sprite2D(s), "size") => match v {
                F::Vec3(size) if size.is_finite() => s.size = Vec3 { x: size.x.abs(), y: size.y.abs(), z: size.z },
                _ => return bad(),
            },
            (Component::Sprite2D(s), "visible") => match v {
                F::Bool(b) => s.visible = b,
                _ => return bad(),
            },
            (Component::Sprite2D(s), "z_index") => {
                let z = fnum?;
                if z < i32::MIN as f32 || z > i32::MAX as f32 {
                    return Err("Sprite2D.z_index is out of range".into());
                }
                s.z_index = z as i32;
            }
            (Component::MeshRenderer(m), "mesh") => match v {
                F::Text(s) => m.mesh = s,
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
            (Component::MeshRenderer(m), "cast_shadows") => match v {
                F::Bool(b) => m.cast_shadows = b,
                _ => return bad(),
            },
            (Component::MeshRenderer(m), field) => {
                if !m.material.set_field(field, v)? {
                    return Err(format!("MeshRenderer has no property '{}'", field));
                }
            }
            (Component::RigidBody(b), "body_type") => match v {
                F::Choice(s, _) | F::Text(s) => {
                    b.body_type = BodyType::parse(&s).ok_or_else(|| format!("unknown body type '{}'", s))?
                }
                _ => return bad(),
            },
            (Component::RigidBody(b), "mass") => {
                b.mass = {
                    let f = fnum?;
                    f.max(0.001)
                }
            }
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
            (Component::Collider(c), "radius") => {
                c.radius = {
                    let f = fnum?;
                    f.abs()
                }
            }
            (Component::Collider(c), "offset") => match v {
                F::Vec3(p) if p.is_finite() => c.offset = p,
                _ => return bad(),
            },
            (Component::Collider(c), "sensor") => match v {
                F::Bool(b) => c.sensor = b,
                _ => return bad(),
            },
            (Component::Collider(c), "friction") => {
                c.friction = {
                    let f = fnum?;
                    f.clamp(0.0, 1.0)
                }
            }
            (Component::Collider(c), "restitution") => {
                c.restitution = {
                    let f = fnum?;
                    f.clamp(0.0, 1.0)
                }
            }
            (Component::AudioSource(a), "clip") => match v {
                F::Text(s) => a.clip = s,
                _ => return bad(),
            },
            (Component::AudioSource(a), "volume") => {
                a.volume = {
                    let f = fnum?;
                    f.clamp(0.0, 4.0)
                }
            }
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
            (Component::Light(l), "intensity") => {
                l.intensity = {
                    let f = fnum?;
                    f.max(0.0)
                }
            }
            (Component::Light(l), "light_type" | "type") => match v {
                F::Choice(s, _) | F::Text(s) => {
                    l.kind = LightKind::parse(&s)
                        .ok_or_else(|| format!("Light type must be one of {}", LIGHT_KIND_NAMES.join(", ")))?
                }
                _ => return bad(),
            },
            (Component::Light(l), "range") => l.range = fnum?.max(0.01),
            (Component::Light(l), "spot_angle") => l.spot_angle = fnum?.clamp(1.0, 179.0),
            (Component::Light(l), "shadows") => match v {
                F::Bool(b) => l.shadows = b,
                _ => return bad(),
            },
            (Component::Skeleton(k), "skin") => k.skin = fnum?.max(0.0) as u32,
            (Component::Skeleton(k), "show_bones") => match v {
                F::Bool(b) => k.show_bones = b,
                _ => return bad(),
            },
            (Component::AnimationPlayer(a), "clip") => match v {
                F::Text(c) => a.clip = c,
                _ => return bad(),
            },
            (Component::AnimationPlayer(a), "autoplay") => match v {
                F::Bool(b) => a.autoplay = b,
                _ => return bad(),
            },
            (Component::AnimationPlayer(a), "looping") => match v {
                F::Bool(b) => a.looping = b,
                _ => return bad(),
            },
            (Component::AnimationPlayer(a), "speed") => a.speed = fnum?.clamp(-10.0, 10.0),
            (Component::AnimationPlayer(a), "crossfade") => a.crossfade = fnum?.clamp(0.0, 5.0),
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
            Component::Camera(c) => {
                base.with("fov", c.fov).with("near", c.near).with("far", c.far).with("active", c.active)
            }
            Component::Camera2D(c) => {
                base.with("active", c.active).with("zoom", c.zoom).with("pixels_per_unit", c.pixels_per_unit)
            }
            Component::Sprite2D(s) => base
                .with("color", s.color.to_hex())
                .with("size", s.size.to_array())
                .with("visible", s.visible)
                .with("z_index", s.z_index as f32),
            Component::MeshRenderer(m) => base
                .with("mesh", m.mesh.as_str())
                .with("material", m.material.to_json())
                .with("lit", m.lit)
                .with("visible", m.visible)
                .with("cast_shadows", m.cast_shadows),
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
            Component::Light(l) => base
                .with("light_type", l.kind.name())
                .with("color", l.color.to_hex())
                .with("intensity", l.intensity)
                .with("range", l.range)
                .with("spot_angle", l.spot_angle)
                .with("shadows", l.shadows),
            Component::Skeleton(k) => base.with("skin", k.skin as u64).with("show_bones", k.show_bones),
            Component::AnimationPlayer(a) => base
                .with("clip", a.clip.as_str())
                .with("autoplay", a.autoplay)
                .with("looping", a.looping)
                .with("speed", a.speed)
                .with("crossfade", a.crossfade),
        }
    }

    pub fn from_json(j: &Json) -> Result<Component, String> {
        let ty = j.get("type").and_then(|t| t.as_str()).ok_or("component is missing \"type\"")?;
        let mut c = Component::new_default(ty)
            .ok_or_else(|| format!("unknown component type \"{}\" (known: {})", ty, COMPONENT_TYPES.join(", ")))?;
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
            Component::Camera2D(cam) => {
                if let Some(v) = bool_of("active")? {
                    cam.active = v;
                }
                if let Some(v) = f32_of("zoom")? {
                    cam.zoom = v.clamp(0.01, 100.0);
                }
                if let Some(v) = f32_of("pixels_per_unit")? {
                    cam.pixels_per_unit = v.clamp(1.0, 4096.0);
                }
            }
            Component::Sprite2D(sprite) => {
                if let Some(v) = color_of("color")? {
                    sprite.color = v;
                }
                if let Some(v) = vec_of("size")? {
                    sprite.size = Vec3 { x: v.x.abs(), y: v.y.abs(), z: v.z };
                }
                if let Some(v) = bool_of("visible")? {
                    sprite.visible = v;
                }
                if let Some(v) = f32_of("z_index")? {
                    sprite.z_index = v.clamp(i32::MIN as f32, i32::MAX as f32) as i32;
                }
            }
            Component::MeshRenderer(m) => {
                if let Some(v) = str_of("mesh")? {
                    m.mesh = v;
                }
                if let Some(mj) = j.get("material") {
                    m.material = Material::from_json(mj)?;
                }
                // Scenes written before materials existed store a flat "color".
                if let Some(v) = color_of("color")? {
                    m.material.base_color = v;
                }
                if let Some(v) = bool_of("lit")? {
                    m.lit = v;
                }
                if let Some(v) = bool_of("visible")? {
                    m.visible = v;
                }
                if let Some(v) = bool_of("cast_shadows")? {
                    m.cast_shadows = v;
                }
            }
            Component::RigidBody(b) => {
                if let Some(v) = str_of("body_type")? {
                    b.body_type = BodyType::parse(&v).ok_or_else(|| {
                        format!("RigidBody.body_type must be static, dynamic or kinematic (got \"{}\")", v)
                    })?;
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
                if let Some(v) = str_of("light_type")? {
                    l.kind = LightKind::parse(&v).ok_or_else(|| {
                        format!("Light.light_type must be directional, point or spot (got \"{}\")", v)
                    })?;
                }
                if let Some(v) = f32_of("range")? {
                    l.range = v.max(0.01);
                }
                if let Some(v) = f32_of("spot_angle")? {
                    l.spot_angle = v.clamp(1.0, 179.0);
                }
                if let Some(v) = bool_of("shadows")? {
                    l.shadows = v;
                }
            }
            Component::Skeleton(k) => {
                if let Some(v) = f32_of("skin")? {
                    k.skin = v.max(0.0) as u32;
                }
                if let Some(v) = bool_of("show_bones")? {
                    k.show_bones = v;
                }
            }
            Component::AnimationPlayer(a) => {
                if let Some(v) = str_of("clip")? {
                    a.clip = v;
                }
                if let Some(v) = bool_of("autoplay")? {
                    a.autoplay = v;
                }
                if let Some(v) = bool_of("looping")? {
                    a.looping = v;
                }
                if let Some(v) = f32_of("speed")? {
                    a.speed = v;
                }
                if let Some(v) = f32_of("crossfade")? {
                    a.crossfade = v.max(0.0);
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
    /// Named groups used for broad scene queries (Godot-style node groups).
    pub groups: Vec<String>,
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
    pub fn add_to_group(&mut self, group: &str) -> bool {
        if group.trim().is_empty() || self.groups.iter().any(|g| g.as_str() == group) {
            return false;
        }
        self.groups.push(group.to_string());
        true
    }
    pub fn remove_from_group(&mut self, group: &str) -> bool {
        let old_len = self.groups.len();
        self.groups.retain(|g| g.as_str() != group);
        self.groups.len() != old_len
    }
    pub fn is_in_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g.as_str() == group)
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
            groups: Vec::new(),
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
                    .with("groups", Json::Arr(e.groups.iter().map(|g| Json::from(g.as_str())).collect()))
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
        let version = j.get("scene_version").and_then(|v| v.as_f64()).ok_or("missing \"scene_version\"")? as u32;
        if version > SCENE_VERSION {
            return Err(format!("scene_version {} is newer than this Grodot supports ({})", version, SCENE_VERSION));
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
            let id =
                ej.get("id").and_then(|v| v.as_f64()).ok_or_else(|| ctx("missing numeric \"id\"".into()))? as EntityId;
            if scene.entities.iter().any(|e| e.id == id) {
                return Err(ctx(format!("duplicate id {}", id)));
            }
            max_id = max_id.max(id);
            let parent = match ej.get("parent") {
                None | Some(Json::Null) => None,
                Some(Json::Num(n)) => Some(*n as EntityId),
                Some(_) => return Err(ctx("\"parent\" must be an id or null".into())),
            };
            let mut groups = Vec::new();
            if let Some(gs) = ej.get("groups") {
                let gs = gs.as_array().ok_or_else(|| ctx("\"groups\" must be an array of strings".into()))?;
                for group in gs {
                    let group = group.as_str().ok_or_else(|| ctx("\"groups\" must contain only strings".into()))?;
                    if group.trim().is_empty() {
                        return Err(ctx("group names cannot be empty".into()));
                    }
                    if !groups.iter().any(|g: &String| g.as_str() == group) {
                        groups.push(group.to_string());
                    }
                }
            }
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
                groups,
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
        let src =
            r#"{"scene_version":1,"entities":[{"id":1,"name":"X","components":[{"type":"Camera","fov":"wide"}]}]}"#;
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
