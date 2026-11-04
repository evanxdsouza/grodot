//! Renders a [`Scene`] (shared by the editor viewport and the game runtime).

use crate::canvas::{Canvas, Rect};
use crate::raster::{self, DirLight, DrawItem, Lighting, LightingMode, LocalLight, RMaterial, Spot, View};
use crate::texture::Texture;
use grodot_core::math::{Color, Mat4, Quat, Vec3};
use grodot_core::mesh::Mesh;
use grodot_core::scene::{Component, EntityId, LightKind, Material, Scene, Shape};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Image formats the texture loader understands.
pub const TEXTURE_EXTENSIONS: &[&str] = &["png", "bmp"];

pub fn is_texture_path(p: &str) -> bool {
    let l = p.to_lowercase();
    TEXTURE_EXTENSIONS.iter().any(|e| l.ends_with(&format!(".{}", e)))
}

type SpritePick = (i32, usize, EntityId, Vec<(f32, f32)>);

/// Per-frame data from the runtime that is not part of the saved scene.
#[derive(Default)]
pub struct RenderExtras<'a> {
    /// Animated node-local matrices of each entity's glTF model.
    pub poses: Option<&'a HashMap<EntityId, Vec<Mat4>>>,
}

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub grid: bool,
    pub selection: Option<EntityId>,
    pub show_colliders: bool,
    pub show_camera_icons: bool,
}

/// One drawable piece of a glTF model.
pub struct ModelPart {
    pub mesh: Rc<Mesh>,
    /// Node index in the glTF (its transform comes from the node hierarchy).
    pub node: usize,
    pub material: RMaterial,
    pub lines: bool,
}

/// A loaded glTF model ready to draw.
pub struct RenderModel {
    pub gltf: grodot_core::gltf::Gltf,
    pub parts: Vec<ModelPart>,
    pub bounds: (Vec3, Vec3),
}

impl RenderModel {
    fn from_gltf(gltf: grodot_core::gltf::Gltf) -> RenderModel {
        let textures: Vec<Option<Arc<Texture>>> =
            gltf.images.iter().map(|img| img.as_ref().map(|i| Arc::new(Texture::from_image(i)))).collect();
        let mut parts = Vec::new();
        for (ni, node) in gltf.nodes.iter().enumerate() {
            let Some(mi) = node.mesh else { continue };
            for prim in &gltf.meshes[mi].primitives {
                let gm = prim.material.and_then(|m| gltf.materials.get(m)).cloned().unwrap_or_default();
                let mut mat = RMaterial::solid(gm.base_color, true);
                if let Some(t) = &gm.base_color_texture {
                    mat.texture = textures.get(t.image).cloned().flatten();
                    mat.filter = t.filter;
                    mat.wrap = t.wrap;
                }
                mat.emissive = [gm.emissive.r, gm.emissive.g, gm.emissive.b];
                if let Some(t) = &gm.emissive_texture {
                    mat.emissive_texture = textures.get(t.image).cloned().flatten();
                }
                mat.alpha_mode = gm.alpha_mode;
                mat.alpha_cutoff = gm.alpha_cutoff;
                mat.double_sided = gm.double_sided;
                parts.push(ModelPart {
                    mesh: Rc::new(prim.mesh.clone()),
                    node: ni,
                    material: mat,
                    lines: prim.kind == grodot_core::gltf::PrimitiveKind::Lines,
                });
            }
        }
        let bounds = gltf.bounds();
        RenderModel { gltf, parts, bounds }
    }
}

/// Combine a model's own material with the MeshRenderer's settings: the
/// MeshRenderer colour tints, its texture (if set) replaces the model's, and
/// its UV scale/offset, alpha mode and double-sidedness apply on top.
pub fn combine_materials(model: &RMaterial, mr: &RMaterial, mr_has_texture: bool) -> RMaterial {
    let mut m = model.clone();
    for i in 0..4 {
        m.base_color[i] *= mr.base_color[i];
    }
    if mr_has_texture {
        m.texture = mr.texture.clone();
        m.filter = mr.filter;
        m.wrap = mr.wrap;
    }
    m.uv_scale = mr.uv_scale;
    m.uv_offset = mr.uv_offset;
    if mr.alpha_mode != grodot_core::scene::AlphaMode::Opaque {
        m.alpha_mode = mr.alpha_mode;
        m.alpha_cutoff = mr.alpha_cutoff;
    }
    m.double_sided |= mr.double_sided;
    m.lit = mr.lit;
    // The MeshRenderer's emissive adds to the model's own glow.
    for i in 0..3 {
        m.emissive[i] += mr.emissive[i];
    }
    if mr.emissive_texture.is_some() {
        m.emissive_texture = mr.emissive_texture.clone();
    }
    m
}

pub struct SceneRenderer {
    meshes: HashMap<String, Option<Rc<Mesh>>>,
    models: HashMap<String, Option<Rc<RenderModel>>>,
    textures: HashMap<String, Option<Arc<Texture>>>,
    /// Project root used to resolve `.obj` mesh paths.
    pub root: Option<PathBuf>,
    /// Asset problems found while rendering (each reported once).
    pub warnings: Vec<String>,
    /// Shadow map resolution (texels per side) for each shadowed directional light.
    pub shadow_map_size: usize,
    /// Shadows are rendered up to this distance (metres) from the camera.
    pub shadow_distance: f32,
    /// Per-pixel (default) or per-vertex lighting.
    pub lighting_mode: LightingMode,
}

/// Default shadow map size. See docs/DECISIONS.md for the measured cost.
pub const DEFAULT_SHADOW_MAP_SIZE: usize = 1024;

impl SceneRenderer {
    pub fn new(root: Option<PathBuf>) -> Self {
        SceneRenderer {
            meshes: HashMap::new(),
            models: HashMap::new(),
            textures: HashMap::new(),
            root,
            warnings: Vec::new(),
            shadow_map_size: DEFAULT_SHADOW_MAP_SIZE,
            shadow_distance: 40.0,
            lighting_mode: LightingMode::PerPixel,
        }
    }

    /// Forget cached meshes and textures (e.g. after files changed on disk).
    pub fn clear_cache(&mut self) {
        self.meshes.clear();
        self.models.clear();
        self.textures.clear();
    }

    /// Load (and cache) a glTF model (`.gltf` or `.glb`).
    pub fn model(&mut self, path: &str) -> Option<Rc<RenderModel>> {
        if let Some(m) = self.models.get(path) {
            return m.clone();
        }
        let loaded = (|| -> Result<grodot_core::gltf::Gltf, String> {
            let root = self.root.as_ref().ok_or("no project folder")?;
            let full = grodot_core::config::resolve_in(root, path)?;
            grodot_core::gltf::load(&full).map_err(|e| e.0)
        })();
        let model = match loaded {
            Ok(g) => {
                for w in &g.warnings {
                    self.warnings.push(format!("model '{}': {}", path, w));
                }
                Some(Rc::new(RenderModel::from_gltf(g)))
            }
            Err(e) => {
                self.warnings.push(format!("model '{}': {}", path, e));
                None
            }
        };
        self.models.insert(path.to_string(), model.clone());
        model
    }

    /// Local-space bounds of whatever a MeshRenderer's `mesh` names.
    pub fn mesh_bounds(&mut self, name: &str) -> Option<(Vec3, Vec3)> {
        if grodot_core::gltf::is_model_path(name) {
            self.model(name).map(|m| m.bounds)
        } else {
            self.mesh(name).map(|m| m.bounds())
        }
    }

    /// Forget one cached asset so it is reloaded next frame (hot reload).
    pub fn invalidate(&mut self, path: &str) {
        self.meshes.remove(path);
        self.models.remove(path);
        self.textures.remove(path);
    }

    /// Load (and cache) a project texture. Problems are reported once in `warnings`.
    pub fn texture(&mut self, path: &str) -> Option<Arc<Texture>> {
        if let Some(t) = self.textures.get(path) {
            return t.clone();
        }
        let loaded = (|| -> Result<Arc<Texture>, String> {
            let root = self.root.as_ref().ok_or("no project folder")?;
            let full = grodot_core::config::resolve_in(root, path)?;
            let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
            let img = grodot_core::image::decode(&bytes).map_err(|e| e.to_string())?;
            Ok(Arc::new(Texture::from_image(&img)))
        })();
        let t = match loaded {
            Ok(t) => Some(t),
            Err(e) => {
                self.warnings.push(format!("texture '{}': {}", path, e));
                None
            }
        };
        self.textures.insert(path.to_string(), t.clone());
        t
    }

    /// Resolve a scene material (loading its texture) for the rasterizer.
    pub fn material(&mut self, m: &Material, lit: bool) -> RMaterial {
        let c = m.base_color;
        RMaterial {
            base_color: [c.r, c.g, c.b, c.a],
            texture: m.texture.as_deref().and_then(|t| self.texture(t)),
            uv_scale: [m.uv_scale.x, m.uv_scale.y],
            uv_offset: [m.uv_offset.x, m.uv_offset.y],
            filter: m.filter,
            wrap: m.wrap,
            alpha_mode: m.alpha_mode,
            alpha_cutoff: m.alpha_cutoff,
            double_sided: m.double_sided,
            lit,
            emissive: [m.emissive.r, m.emissive.g, m.emissive.b],
            emissive_texture: m.emissive_texture.as_deref().and_then(|t| self.texture(t)),
        }
    }

    pub fn mesh(&mut self, name: &str) -> Option<Rc<Mesh>> {
        if let Some(m) = self.meshes.get(name) {
            return m.clone();
        }
        let loaded = if let Some(p) = Mesh::primitive(name) {
            Some(Rc::new(p))
        } else if name.is_empty() || grodot_core::gltf::is_model_path(name) {
            None
        } else {
            match &self.root {
                Some(root) => match grodot_core::config::resolve_in(root, name) {
                    Err(e) => {
                        self.warnings.push(format!("mesh '{}': {}", name, e));
                        None
                    }
                    Ok(path) => match std::fs::read_to_string(&path) {
                        Ok(src) => match Mesh::from_obj(&src) {
                            Ok(m) => Some(Rc::new(m)),
                            Err(e) => {
                                self.warnings.push(format!("mesh '{}': invalid OBJ: {}", name, e));
                                None
                            }
                        },
                        Err(e) => {
                            self.warnings.push(format!(
                                "mesh '{}': {} (expected a primitive: cube, sphere, plane, cylinder, capsule, or an .obj file)",
                                name, e
                            ));
                            None
                        }
                    },
                },
                None => {
                    self.warnings.push(format!("mesh '{}' not found", name));
                    None
                }
            }
        };
        self.meshes.insert(name.to_string(), loaded.clone());
        loaded
    }

    /// View for the scene's active camera.
    pub fn camera_view(scene: &Scene, viewport: Rect) -> Option<View> {
        if let Some(cam_ent) = scene
            .entities
            .iter()
            .find(|e| e.camera().map(|c| c.active).unwrap_or(false) && scene.is_active_in_hierarchy(e.id))
        {
            let cam = cam_ent.camera()?;
            let world = scene.world_matrix(cam_ent.id);
            return Some(view_from_matrix(&world, cam.fov, cam.near, cam.far, viewport));
        }
        let cam_ent = scene.entities.iter().find(|e| {
            matches!(e.get("Camera2D"), Some(Component::Camera2D(c)) if c.active) && scene.is_active_in_hierarchy(e.id)
        })?;
        // The 2D pass projects directly onto the canvas. This view only keeps
        // the existing 3D/editor overlays usable in mixed scenes.
        let center = scene.world_matrix(cam_ent.id).get_translation();
        let world = Mat4::from_trs(center + Vec3::Z * 10.0, Quat::IDENTITY, Vec3::ONE);
        Some(view_from_matrix(&world, 70.0, 0.1, 1000.0, viewport))
    }

    /// Collect every active Light. Scenes with no Light get a default sun.
    /// Shadow maps are not rendered here (see `render_ex`).
    pub fn lighting(scene: &Scene, view: &View) -> Lighting {
        let mut l = Lighting { ambient: scene.environment.ambient, ..Default::default() };
        let mut any = false;
        let mut suns = Vec::new();
        for e in &scene.entities {
            let Some(Component::Light(light)) = e.get("Light") else { continue };
            if !scene.is_active_in_hierarchy(e.id) {
                continue;
            }
            any = true;
            let world = scene.world_matrix(e.id);
            let forward = world.transform_vector(Vec3::FORWARD).normalized();
            match light.kind {
                LightKind::Directional => suns.push(DirLight {
                    direction: forward,
                    color: light.color,
                    intensity: light.intensity,
                    casts_shadows: light.shadows,
                    shadow: None,
                }),
                LightKind::Point | LightKind::Spot => l.locals.push(LocalLight {
                    position: world.get_translation(),
                    color: light.color,
                    intensity: light.intensity,
                    range: light.range.max(0.01),
                    spot: (light.kind == LightKind::Spot).then(|| Spot::new(forward, light.spot_angle)),
                }),
            }
        }
        if any {
            l.suns = suns;
        }
        let bg = scene.environment.background;
        l.fog_color = horizon(bg);
        l.fog_end = view.far.min(400.0);
        l.fog_start = l.fog_end * 0.45;
        l
    }

    pub fn draw_background(canvas: &mut Canvas, viewport: Rect, bg: Color) {
        let top = bg.to_u32();
        let bottom = horizon(bg).to_u32();
        let old = canvas.clip;
        canvas.set_clip(viewport.intersect(&old));
        canvas.vgradient(viewport, top, bottom);
        canvas.clip = old;
        canvas.clear_depth_rect(viewport);
    }

    /// Render a shadow map for each directional light that casts shadows
    /// (at most `raster::MAX_SHADOWED_SUNS`).
    pub fn render_shadow_maps(&self, light: &mut Lighting, view: &View, items: &[DrawItem]) {
        if self.shadow_map_size == 0 || !items.iter().any(|i| i.cast_shadows) {
            return;
        }
        let mut done = 0;
        for sun in light.suns.iter_mut() {
            if !sun.casts_shadows || done >= raster::MAX_SHADOWED_SUNS {
                continue;
            }
            done += 1;
            let (vp, texel, dpm) = shadow_camera(view, sun.direction, self.shadow_map_size, self.shadow_distance);
            sun.shadow = Some(Arc::new(raster::render_shadow_map(self.shadow_map_size, vp, texel, dpm, items)));
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, viewport: Rect, scene: &Scene, view: &View, opts: &RenderOptions) {
        self.render_ex(canvas, viewport, scene, view, opts, &RenderExtras::default())
    }

    /// Render with runtime extras (animation poses).
    ///
    /// Skinned meshes are deformed on the CPU into a scratch mesh every frame:
    /// cost is roughly (vertices x influences) matrix multiplies per skinned
    /// model, which is small next to rasterization for game-jam characters
    /// (a 1-2k vertex character skins in well under a millisecond).
    pub fn render_ex(
        &mut self,
        canvas: &mut Canvas,
        viewport: Rect,
        scene: &Scene,
        view: &View,
        opts: &RenderOptions,
        extras: &RenderExtras,
    ) {
        Self::draw_background(canvas, viewport, scene.environment.background);
        let mut bones: Vec<(Vec3, Vec3)> = Vec::new();
        let mut light = Self::lighting(scene, view);
        light.mode = self.lighting_mode;
        let mut meshes: Vec<(Rc<Mesh>, Mat4, RMaterial, bool)> = Vec::new();
        let mut line_parts: Vec<(Rc<Mesh>, Mat4, RMaterial)> = Vec::new();
        for e in &scene.entities {
            let Some(mr) = e.mesh() else { continue };
            if !mr.visible || !scene.is_active_in_hierarchy(e.id) {
                continue;
            }
            let world = scene.world_matrix(e.id);
            let mat = self.material(&mr.material, mr.lit);
            if grodot_core::gltf::is_model_path(&mr.mesh) {
                if let Some(model) = self.model(&mr.mesh) {
                    let pose = extras.poses.and_then(|p| p.get(&e.id));
                    let nodes = match pose {
                        Some(locals) if locals.len() == model.gltf.nodes.len() => model.gltf.world_from_locals(locals),
                        _ => model.gltf.node_world_matrices(),
                    };
                    let skeleton = match e.get("Skeleton") {
                        Some(Component::Skeleton(k)) => Some(k.clone()),
                        _ => None,
                    };
                    let skin_joints = skeleton.as_ref().and_then(|k| {
                        let skin = k.skin as usize;
                        (skin < model.gltf.skins.len())
                            .then(|| grodot_core::animation::joint_matrices(&model.gltf, skin, &nodes))
                    });
                    for part in &model.parts {
                        let m = combine_materials(&part.material, &mat, mr.material.texture.is_some());
                        let skinned = !part.mesh.joints.is_empty() && model.gltf.nodes[part.node].skin.is_some();
                        if part.lines {
                            line_parts.push((part.mesh.clone(), world * nodes[part.node], m));
                        } else if skinned {
                            // Skinned vertices live in model space; the mesh node's own transform is ignored (glTF rule).
                            match &skin_joints {
                                Some(j) => meshes.push((
                                    Rc::new(grodot_core::animation::skin_mesh(&part.mesh, j)),
                                    world,
                                    m,
                                    mr.cast_shadows,
                                )),
                                None => meshes.push((part.mesh.clone(), world, m, mr.cast_shadows)),
                            }
                        } else {
                            meshes.push((part.mesh.clone(), world * nodes[part.node], m, mr.cast_shadows));
                        }
                    }
                    if let (Some(k), true) = (&skeleton, skeleton.as_ref().map(|k| k.show_bones).unwrap_or(false)) {
                        if let Some(skin) = model.gltf.skins.get(k.skin as usize) {
                            for &j in &skin.joints {
                                let parent = model.gltf.nodes.iter().position(|n| n.children.contains(&j));
                                if let Some(p) = parent.filter(|p| skin.joints.contains(p)) {
                                    bones.push((
                                        (world * nodes[p]).get_translation(),
                                        (world * nodes[j]).get_translation(),
                                    ));
                                }
                            }
                        }
                    }
                }
            } else if let Some(mesh) = self.mesh(&mr.mesh) {
                meshes.push((mesh, world, mat, mr.cast_shadows));
            }
        }
        let mut items: Vec<DrawItem> = meshes
            .iter()
            .map(|(m, model, mat, cast)| DrawItem {
                mesh: m,
                model: *model,
                material: mat.clone(),
                cast_shadows: *cast,
            })
            .collect();
        self.render_shadow_maps(&mut light, view, &items);
        raster::draw_meshes(canvas, viewport, view, &light, &items);
        items.clear();
        for (mesh, w, m) in &line_parts {
            let c = raster::pack(m.base_color[0], m.base_color[1], m.base_color[2]);
            for seg in mesh.indices.chunks_exact(2) {
                let (Some(a), Some(b)) = (mesh.positions.get(seg[0] as usize), mesh.positions.get(seg[1] as usize))
                else {
                    continue;
                };
                raster::draw_line_3d(canvas, viewport, view, w.transform_point(*a), w.transform_point(*b), c, true);
            }
        }
        for (a, b) in &bones {
            raster::draw_line_3d(canvas, viewport, view, *a, *b, 0xffe040, false);
        }
        if opts.grid {
            draw_grid(canvas, viewport, view);
        }
        if opts.show_colliders {
            for e in &scene.entities {
                if let Some(c) = e.collider() {
                    let world = scene.world_matrix(e.id);
                    let (lo, hi) = match c.shape {
                        Shape::Box => (c.offset - c.size * 0.5, c.offset + c.size * 0.5),
                        Shape::Sphere => (c.offset - Vec3::splat(c.radius), c.offset + Vec3::splat(c.radius)),
                    };
                    let col = if c.sensor { 0x40e0ff } else { 0x60ff60 };
                    draw_box(canvas, viewport, view, &world, lo, hi, col, true);
                }
            }
        }
        if opts.show_camera_icons {
            for e in &scene.entities {
                if let Some(Component::Light(l)) = e.get("Light") {
                    draw_light_gizmo(canvas, viewport, view, &scene.world_matrix(e.id), l);
                } else if e.has("Camera") || e.has("Camera2D") {
                    let world = scene.world_matrix(e.id);
                    let col = 0xffffff;
                    draw_box(canvas, viewport, view, &world, Vec3::splat(-0.2), Vec3::splat(0.2), col, false);
                    let tip = world.transform_point(Vec3::FORWARD * 0.8);
                    raster::draw_line_3d(canvas, viewport, view, world.transform_point(Vec3::ZERO), tip, col, false);
                }
            }
        }
        if let Some(sel) = opts.selection {
            if let Some(e) = scene.entity(sel) {
                let world = scene.world_matrix(sel);
                let (lo, hi) =
                    e.mesh().and_then(|m| self.mesh_bounds(&m.mesh)).unwrap_or((Vec3::splat(-0.25), Vec3::splat(0.25)));
                let pad = (hi - lo) * 0.02 + Vec3::splat(0.01);
                draw_box(canvas, viewport, view, &world, lo - pad, hi + pad, 0xffa030, false);
            }
        }
        self.render_sprites_2d(canvas, viewport, scene);
    }

    fn render_sprites_2d(&self, canvas: &mut Canvas, viewport: Rect, scene: &Scene) {
        let camera = scene.entities.iter().find_map(|e| {
            if !scene.is_active_in_hierarchy(e.id) {
                return None;
            }
            match e.get("Camera2D") {
                Some(Component::Camera2D(c)) if c.active => {
                    let world = scene.world_matrix(e.id);
                    let right = world.transform_vector(Vec3::X).normalized();
                    let up = world.transform_vector(Vec3::Y).normalized();
                    let scale = c.pixels_per_unit * c.zoom;
                    (scale.is_finite() && scale > 0.0).then_some((world.get_translation(), right, up, scale))
                }
                _ => None,
            }
        });
        let Some((center, right, up, scale)) = camera else { return };
        let mut sprites: Vec<(i32, usize, &grodot_core::scene::Sprite2D, Mat4)> = scene
            .entities
            .iter()
            .enumerate()
            .filter_map(|(order, e)| {
                if !scene.is_active_in_hierarchy(e.id) {
                    return None;
                }
                match e.get("Sprite2D") {
                    Some(Component::Sprite2D(sprite)) if sprite.visible => {
                        Some((sprite.z_index, order, sprite, scene.world_matrix(e.id)))
                    }
                    _ => None,
                }
            })
            .collect();
        sprites.sort_by_key(|(z, order, _, _)| (*z, *order));

        let old_clip = canvas.clip;
        canvas.set_clip(viewport.intersect(&old_clip));
        for (_, _, sprite, model) in sprites {
            let half = Vec3 { x: sprite.size.x * 0.5, y: sprite.size.y * 0.5, z: 0.0 };
            let corners = [
                Vec3 { x: -half.x, y: -half.y, z: 0.0 },
                Vec3 { x: half.x, y: -half.y, z: 0.0 },
                Vec3 { x: half.x, y: half.y, z: 0.0 },
                Vec3 { x: -half.x, y: half.y, z: 0.0 },
            ];
            let points: Vec<(f32, f32)> = corners
                .iter()
                .map(|p| {
                    let world = model.transform_point(*p);
                    let relative = world - center;
                    (
                        viewport.x as f32 + viewport.w as f32 * 0.5 + relative.dot(right) * scale,
                        viewport.y as f32 + viewport.h as f32 * 0.5 - relative.dot(up) * scale,
                    )
                })
                .collect();
            canvas.fill_polygon(
                &points,
                sprite.color.to_u32(),
                (sprite.color.a.clamp(0.0, 1.0) * 255.0).round() as u32,
            );
        }
        canvas.clip = old_clip;
    }

    /// Pick the closest entity under a viewport pixel.
    pub fn pick(&mut self, scene: &Scene, view: &View, viewport: Rect, px: f32, py: f32) -> Option<EntityId> {
        if let Some(id) = pick_sprite_2d(scene, viewport, px, py) {
            return Some(id);
        }
        let (origin, dir) = raster::screen_ray(view, viewport, px, py);
        let mut best: Option<(f32, EntityId)> = None;
        for e in &scene.entities {
            if !scene.is_active_in_hierarchy(e.id) {
                continue;
            }
            let (lo, hi) = match e.mesh().and_then(|m| self.mesh_bounds(&m.mesh)) {
                Some(b) => b,
                None if e.has("Camera") || e.has("Camera2D") || e.has("Light") => {
                    (Vec3::splat(-0.25), Vec3::splat(0.25))
                }
                None => continue,
            };
            let world = scene.world_matrix(e.id);
            let inv = world.inverse();
            let o = inv.transform_point(origin);
            let d = inv.transform_vector(dir);
            if let Some(t_local) = ray_aabb(o, d, lo, hi) {
                if t_local <= 0.0 {
                    continue; // the ray starts inside this object (e.g. the camera itself)
                }
                let hit = world.transform_point(o + d * t_local);
                let t = (hit - origin).length();
                if best.map(|(bt, _)| t < bt).unwrap_or(true) {
                    best = Some((t, e.id));
                }
            }
        }
        best.map(|(_, id)| id)
    }
}

fn pick_sprite_2d(scene: &Scene, viewport: Rect, px: f32, py: f32) -> Option<EntityId> {
    let (camera_id, camera) = scene.entities.iter().find_map(|e| {
        if !scene.is_active_in_hierarchy(e.id) {
            return None;
        }
        match e.get("Camera2D") {
            Some(Component::Camera2D(c)) if c.active => Some((e.id, c)),
            _ => None,
        }
    })?;
    let cam_world = scene.world_matrix(camera_id);
    let center = cam_world.get_translation();
    let right = cam_world.transform_vector(Vec3::X).normalized();
    let up = cam_world.transform_vector(Vec3::Y).normalized();
    let scale = camera.pixels_per_unit * camera.zoom;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let mut sprites: Vec<SpritePick> = scene
        .entities
        .iter()
        .enumerate()
        .filter_map(|(order, e)| {
            if !scene.is_active_in_hierarchy(e.id) {
                return None;
            }
            let Some(Component::Sprite2D(sprite)) = e.get("Sprite2D") else { return None };
            if !sprite.visible {
                return None;
            }
            let model = scene.world_matrix(e.id);
            let half = Vec3 { x: sprite.size.x * 0.5, y: sprite.size.y * 0.5, z: 0.0 };
            let corners = [
                Vec3 { x: -half.x, y: -half.y, z: 0.0 },
                Vec3 { x: half.x, y: -half.y, z: 0.0 },
                Vec3 { x: half.x, y: half.y, z: 0.0 },
                Vec3 { x: -half.x, y: half.y, z: 0.0 },
            ];
            let points = corners
                .iter()
                .map(|p| {
                    let relative = model.transform_point(*p) - center;
                    (
                        viewport.x as f32 + viewport.w as f32 * 0.5 + relative.dot(right) * scale,
                        viewport.y as f32 + viewport.h as f32 * 0.5 - relative.dot(up) * scale,
                    )
                })
                .collect();
            Some((sprite.z_index, order, e.id, points))
        })
        .collect();
    sprites.sort_by_key(|(z, order, _, _)| (*z, *order));
    sprites.into_iter().rev().find_map(|(_, _, id, points)| point_in_polygon(px, py, &points).then_some(id))
}

fn point_in_polygon(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut inside = false;
    for i in 0..points.len() {
        let (x0, y0) = points[i];
        let (x1, y1) = points[(i + 1) % points.len()];
        if (y0 > y) != (y1 > y) && x < (x1 - x0) * (y - y0) / (y1 - y0) + x0 {
            inside = !inside;
        }
    }
    inside
}

fn horizon(bg: Color) -> Color {
    Color { r: bg.r + (1.0 - bg.r) * 0.35, g: bg.g + (1.0 - bg.g) * 0.35, b: bg.b + (1.0 - bg.b) * 0.35, a: 1.0 }
}

/// Editor gizmo for a light: a sun with parallel rays (directional), three
/// range rings (point) or a cone out to its range (spot). Drawn in the
/// light's colour, without depth testing.
pub fn draw_light_gizmo(
    canvas: &mut Canvas,
    viewport: Rect,
    view: &View,
    world: &Mat4,
    light: &grodot_core::scene::Light,
) {
    let c = light.color;
    let col = raster::pack(0.35 + 0.65 * c.r, 0.35 + 0.65 * c.g, 0.35 + 0.65 * c.b);
    let o = world.get_translation();
    // Directions only: gizmo sizes ignore the entity's scale.
    let dir = |v: Vec3| world.transform_vector(v).normalized();
    let (fwd, right, up) = (dir(Vec3::FORWARD), dir(Vec3::X), dir(Vec3::Y));
    let line = |canvas: &mut Canvas, a: Vec3, b: Vec3| raster::draw_line_3d(canvas, viewport, view, a, b, col, false);
    let ring = |canvas: &mut Canvas, centre: Vec3, u: Vec3, v: Vec3, r: f32| {
        let n = 32;
        for i in 0..n {
            let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
            line(canvas, centre + (u * a0.cos() + v * a0.sin()) * r, centre + (u * a1.cos() + v * a1.sin()) * r);
        }
    };
    match light.kind {
        LightKind::Directional => {
            ring(canvas, o, right, up, 0.3);
            for (dx, dy) in [(0.0, 0.0), (0.3, 0.0), (-0.3, 0.0), (0.0, 0.3), (0.0, -0.3)] {
                let s = o + right * dx + up * dy;
                line(canvas, s, s + fwd * 1.5);
            }
            // Arrow head on the centre ray.
            let tip = o + fwd * 1.5;
            line(canvas, tip, tip - fwd * 0.25 + right * 0.12);
            line(canvas, tip, tip - fwd * 0.25 - right * 0.12);
        }
        LightKind::Point => {
            let r = light.range;
            ring(canvas, o, Vec3::X, Vec3::Y, r);
            ring(canvas, o, Vec3::X, Vec3::Z, r);
            ring(canvas, o, Vec3::Y, Vec3::Z, r);
            ring(canvas, o, Vec3::X, Vec3::Z, 0.15);
        }
        LightKind::Spot => {
            let half = (light.spot_angle * 0.5).to_radians();
            let len = light.range;
            let rad = len * half.tan().min(50.0);
            let end = o + fwd * len;
            ring(canvas, end, right, up, rad);
            for k in 0..4 {
                let a = k as f32 * std::f32::consts::FRAC_PI_2;
                line(canvas, o, end + (right * a.cos() + up * a.sin()) * rad);
            }
            line(canvas, o, end);
        }
    }
}

/// Fit an orthographic light camera around the part of the view frustum
/// within `distance` of the camera. Returns (world -> light clip, world size
/// of one texel, NDC depth per metre).
///
/// The fit uses the bounding sphere of the frustum slice, so its size does not
/// change as the camera turns, and the centre is snapped to whole texels so
/// shadow edges do not shimmer as the camera moves.
pub fn shadow_camera(view: &View, direction: Vec3, size: usize, distance: f32) -> (Mat4, f32, f32) {
    let dir = direction.normalized();
    let inv = view.view_proj().inverse();
    let (n, f) = (view.near.max(0.01), view.far.max(view.near + 0.1));
    let t = ((distance.min(f) - n) / (f - n)).clamp(0.0, 1.0);
    let unproject = |x: f32, y: f32, z: f32| {
        let c = inv.transform4(Vec3 { x, y, z });
        Vec3 { x: c.x / c.w, y: c.y / c.w, z: c.z / c.w }
    };
    let mut pts = Vec::with_capacity(8);
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let a = unproject(x, y, -1.0);
        let b = unproject(x, y, 1.0);
        // Linear in view depth along the ray from the near to the far plane.
        pts.push(a);
        pts.push(a + (b - a) * t);
    }
    let centre = pts.iter().fold(Vec3::ZERO, |acc, p| acc + *p) * (1.0 / pts.len() as f32);
    let radius = pts.iter().map(|p| (*p - centre).length()).fold(0.0f32, f32::max).max(1.0);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let rot = Mat4::look_at(Vec3::ZERO, dir, up);
    let texel = 2.0 * radius / size as f32;
    let mut lc = rot.transform_point(centre);
    lc.x = (lc.x / texel).round() * texel;
    lc.y = (lc.y / texel).round() * texel;
    let centre = rot.inverse().transform_point(lc);
    // Casters up to 60 m behind the slice (towards the light) still block it.
    let back = 60.0;
    let eye = centre - dir * (radius + back);
    let far = 2.0 * radius + back;
    let lv = Mat4::look_at(eye, centre, up);
    let proj = Mat4::orthographic(-radius, radius, -radius, radius, 0.0, far);
    (proj * lv, texel, 2.0 / far)
}

pub fn view_from_matrix(world: &Mat4, fov: f32, near: f32, far: f32, viewport: Rect) -> View {
    let eye = world.transform_point(Vec3::ZERO);
    let fwd = world.transform_vector(Vec3::FORWARD).normalized();
    let up = world.transform_vector(Vec3::Y).normalized();
    let aspect = viewport.w.max(1) as f32 / viewport.h.max(1) as f32;
    View {
        view: Mat4::look_at(eye, eye + fwd, up),
        proj: Mat4::perspective(fov, aspect, near.max(0.01), far.max(near + 0.1)),
        eye,
        near,
        far,
    }
}

/// Slab test; returns the entry distance along the ray.
pub fn ray_aabb(o: Vec3, d: Vec3, lo: Vec3, hi: Vec3) -> Option<f32> {
    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    for i in 0..3 {
        let (oi, di, l, h) = (o.get(i), d.get(i), lo.get(i), hi.get(i));
        if di.abs() < 1e-9 {
            if oi < l || oi > h {
                return None;
            }
        } else {
            let t1 = (l - oi) / di;
            let t2 = (h - oi) / di;
            tmin = tmin.max(t1.min(t2));
            tmax = tmax.min(t1.max(t2));
        }
    }
    if tmax >= tmin.max(0.0) {
        Some(tmin.max(0.0))
    } else {
        None
    }
}

fn draw_grid(canvas: &mut Canvas, viewport: Rect, view: &View) {
    let n = 20;
    for i in -n..=n {
        let f = i as f32;
        let (cx, cz) = if i == 0 {
            (0xd04040, 0x4060e0)
        } else if i % 5 == 0 {
            (0x8a8f99, 0x8a8f99)
        } else {
            (0x6a6f78, 0x6a6f78)
        };
        raster::draw_line_3d(
            canvas,
            viewport,
            view,
            Vec3 { x: -n as f32, y: 0.0, z: f },
            Vec3 { x: n as f32, y: 0.0, z: f },
            cx,
            true,
        );
        raster::draw_line_3d(
            canvas,
            viewport,
            view,
            Vec3 { x: f, y: 0.0, z: -n as f32 },
            Vec3 { x: f, y: 0.0, z: n as f32 },
            cz,
            true,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_box(
    canvas: &mut Canvas,
    viewport: Rect,
    view: &View,
    world: &Mat4,
    lo: Vec3,
    hi: Vec3,
    color: u32,
    depth: bool,
) {
    let c = |x: f32, y: f32, z: f32| world.transform_point(Vec3 { x, y, z });
    let p = [
        c(lo.x, lo.y, lo.z),
        c(hi.x, lo.y, lo.z),
        c(hi.x, hi.y, lo.z),
        c(lo.x, hi.y, lo.z),
        c(lo.x, lo.y, hi.z),
        c(hi.x, lo.y, hi.z),
        c(hi.x, hi.y, hi.z),
        c(lo.x, hi.y, hi.z),
    ];
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    for (a, b) in edges {
        raster::draw_line_3d(canvas, viewport, view, p[a], p[b], color, depth);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grodot_core::math::vec3;

    fn test_scene() -> Scene {
        let mut s = Scene::new("t");
        let cam = s.add_entity("Camera");
        s.entity_mut(cam).unwrap().add(Component::new_default("Camera").unwrap());
        s.entity_mut(cam).unwrap().transform_mut().position = vec3(0.0, 0.0, 5.0);
        let cube = s.add_entity("Cube");
        let mut mr = Component::new_default("MeshRenderer").unwrap();
        mr.set_field("color", grodot_core::FieldValue::Color(Color::rgb(1.0, 0.0, 0.0))).unwrap();
        s.entity_mut(cube).unwrap().add(mr);
        s
    }

    #[test]
    fn renders_cube_in_centre() {
        let s = test_scene();
        let mut canvas = Canvas::new(160, 120);
        let vp = canvas.full_rect();
        let view = SceneRenderer::camera_view(&s, vp).unwrap();
        let mut r = SceneRenderer::new(None);
        r.render(&mut canvas, vp, &s, &view, &RenderOptions::default());
        let centre = canvas.color[60 * 160 + 80];
        let (red, green) = ((centre >> 16) & 255, (centre >> 8) & 255);
        assert!(red > 60 && green < 40, "centre pixel {:06x}", centre);
        // Corner is background (not red).
        let corner = canvas.color[0];
        assert!(((corner >> 16) & 255) < 200 || ((corner >> 8) & 255) > 100);
        assert!(canvas.depth[60 * 160 + 80] < 1.0);
    }

    #[test]
    fn picking_hits_cube() {
        let s = test_scene();
        let vp = Rect::new(0, 0, 160, 120);
        let view = SceneRenderer::camera_view(&s, vp).unwrap();
        let mut r = SceneRenderer::new(None);
        let id = r.pick(&s, &view, vp, 80.0, 60.0);
        assert_eq!(id, Some(s.find_by_name("Cube").unwrap().id));
        assert_eq!(r.pick(&s, &view, vp, 2.0, 2.0), None);
    }

    #[test]
    fn camera_behind_geometry_does_not_crash() {
        let mut s = test_scene();
        s.entity_mut(1).unwrap().transform_mut().position = vec3(0.0, 0.0, 0.2);
        let mut canvas = Canvas::new(64, 48);
        let vp = canvas.full_rect();
        let view = SceneRenderer::camera_view(&s, vp).unwrap();
        SceneRenderer::new(None).render(
            &mut canvas,
            vp,
            &s,
            &view,
            &RenderOptions { grid: true, ..Default::default() },
        );
    }

    #[test]
    fn missing_mesh_reports_warning_once() {
        let mut r = SceneRenderer::new(Some(std::env::temp_dir()));
        assert!(r.mesh("nope/missing.obj").is_none());
        assert!(r.mesh("nope/missing.obj").is_none());
        assert_eq!(r.warnings.len(), 1);
    }
}
