//! Renders a [`Scene`] (shared by the editor viewport and the game runtime).

use crate::canvas::{blend, Canvas, Rect};
use crate::raster::{self, DrawItem, Lighting, View};
use grodot_core::math::{Color, Mat4, Vec3};
use grodot_core::mesh::Mesh;
use grodot_core::scene::{Component, EntityId, Scene, Shape};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    pub grid: bool,
    pub selection: Option<EntityId>,
    pub show_colliders: bool,
    pub show_camera_icons: bool,
}

pub struct SceneRenderer {
    meshes: HashMap<String, Option<Rc<Mesh>>>,
    /// Project root used to resolve `.obj` mesh paths.
    pub root: Option<PathBuf>,
    /// Asset problems found while rendering (each reported once).
    pub warnings: Vec<String>,
}

impl SceneRenderer {
    pub fn new(root: Option<PathBuf>) -> Self {
        SceneRenderer { meshes: HashMap::new(), root, warnings: Vec::new() }
    }

    /// Forget cached meshes (e.g. after files changed on disk).
    pub fn clear_cache(&mut self) {
        self.meshes.clear();
    }

    pub fn mesh(&mut self, name: &str) -> Option<Rc<Mesh>> {
        if let Some(m) = self.meshes.get(name) {
            return m.clone();
        }
        let loaded = if let Some(p) = Mesh::primitive(name) {
            Some(Rc::new(p))
        } else if name.is_empty() {
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
        let cam_ent = scene
            .entities
            .iter()
            .find(|e| e.camera().map(|c| c.active).unwrap_or(false) && scene.is_active_in_hierarchy(e.id))?;
        let cam = cam_ent.camera()?;
        let world = scene.world_matrix(cam_ent.id);
        Some(view_from_matrix(&world, cam.fov, cam.near, cam.far, viewport))
    }

    pub fn lighting(scene: &Scene, view: &View) -> Lighting {
        let mut l = Lighting { ambient: scene.environment.ambient, ..Default::default() };
        if let Some(e) = scene.entities.iter().find(|e| e.has("Light") && scene.is_active_in_hierarchy(e.id)) {
            if let Some(Component::Light(light)) = e.get("Light") {
                let world = scene.world_matrix(e.id);
                l.direction = world.transform_vector(Vec3::FORWARD).normalized();
                l.color = light.color;
                l.intensity = light.intensity;
            }
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

    pub fn render(&mut self, canvas: &mut Canvas, viewport: Rect, scene: &Scene, view: &View, opts: &RenderOptions) {
        Self::draw_background(canvas, viewport, scene.environment.background);
        let light = Self::lighting(scene, view);
        let mut meshes: Vec<(Rc<Mesh>, Mat4, Color, bool)> = Vec::new();
        for e in &scene.entities {
            let Some(mr) = e.mesh() else { continue };
            if !mr.visible || !scene.is_active_in_hierarchy(e.id) {
                continue;
            }
            if let Some(mesh) = self.mesh(&mr.mesh) {
                meshes.push((mesh, scene.world_matrix(e.id), mr.color, mr.lit));
            }
        }
        // Blob shadows: soft dark discs under lit meshes, drawn as flat geometry.
        let disc = shadow_disc();
        let mut shadows = Vec::new();
        for (mesh, model, _, lit) in &meshes {
            if !*lit {
                continue;
            }
            let (lo, hi) = mesh.bounds();
            let size = model.transform_vector(hi - lo).abs();
            let centre = model.transform_point((lo + hi) * 0.5);
            let bottom = centre.y - size.y * 0.5;
            if size.y < 0.05 || size.x > 6.0 || size.z > 6.0 {
                continue; // floors and walls do not cast blobs
            }
            if let Some(ground) = ground_height(scene, &meshes, centre, bottom) {
                let h = bottom - ground;
                if (0.0..8.0).contains(&h) {
                    let r = (size.x.max(size.z) * 0.6) * (1.0 - h / 10.0);
                    let strength = (1.0 - h / 8.0) * 0.5;
                    shadows.push((
                        Mat4::from_trs(
                            Vec3 { x: centre.x, y: ground + 0.012, z: centre.z },
                            grodot_core::math::Quat::IDENTITY,
                            Vec3 { x: r, y: 1.0, z: r },
                        ),
                        strength,
                    ));
                }
            }
        }
        let mut items: Vec<DrawItem> =
            meshes.iter().map(|(m, model, c, lit)| DrawItem { mesh: m, model: *model, color: *c, lit: *lit }).collect();
        raster::draw_meshes(canvas, viewport, view, &light, &items);
        items.clear();
        // Shadows are drawn as darkened unlit discs blended by depth test.
        for (model, strength) in &shadows {
            draw_shadow(canvas, viewport, view, &disc, *model, *strength);
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
                if e.has("Camera") || e.has("Light") {
                    let world = scene.world_matrix(e.id);
                    let col = if e.has("Camera") { 0xffffff } else { 0xffe060 };
                    draw_box(canvas, viewport, view, &world, Vec3::splat(-0.2), Vec3::splat(0.2), col, false);
                    let tip = world.transform_point(Vec3::FORWARD * 0.8);
                    raster::draw_line_3d(canvas, viewport, view, world.transform_point(Vec3::ZERO), tip, col, false);
                }
            }
        }
        if let Some(sel) = opts.selection {
            if let Some(e) = scene.entity(sel) {
                let world = scene.world_matrix(sel);
                let (lo, hi) = e
                    .mesh()
                    .and_then(|m| self.mesh(&m.mesh))
                    .map(|m| m.bounds())
                    .unwrap_or((Vec3::splat(-0.25), Vec3::splat(0.25)));
                let pad = (hi - lo) * 0.02 + Vec3::splat(0.01);
                draw_box(canvas, viewport, view, &world, lo - pad, hi + pad, 0xffa030, false);
            }
        }
    }

    /// Pick the closest entity under a viewport pixel.
    pub fn pick(&mut self, scene: &Scene, view: &View, viewport: Rect, px: f32, py: f32) -> Option<EntityId> {
        let (origin, dir) = raster::screen_ray(view, viewport, px, py);
        let mut best: Option<(f32, EntityId)> = None;
        for e in &scene.entities {
            if !scene.is_active_in_hierarchy(e.id) {
                continue;
            }
            let (lo, hi) = match e.mesh().and_then(|m| self.mesh(&m.mesh)) {
                Some(m) => m.bounds(),
                None if e.has("Camera") || e.has("Light") => (Vec3::splat(-0.25), Vec3::splat(0.25)),
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

fn horizon(bg: Color) -> Color {
    Color { r: bg.r + (1.0 - bg.r) * 0.35, g: bg.g + (1.0 - bg.g) * 0.35, b: bg.b + (1.0 - bg.b) * 0.35, a: 1.0 }
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

fn ground_height(_scene: &Scene, meshes: &[(Rc<Mesh>, Mat4, Color, bool)], p: Vec3, below: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    for (m, model, _, _) in meshes {
        let (lo, hi) = m.bounds();
        let wlo = model.transform_point(lo);
        let whi = model.transform_point(hi);
        let (mn, mx) = (wlo.min(whi), wlo.max(whi));
        if p.x >= mn.x
            && p.x <= mx.x
            && p.z >= mn.z
            && p.z <= mx.z
            && mx.y <= below + 0.01
            && (mx.x - mn.x) > 1.5
            && best.map(|b| mx.y > b).unwrap_or(true)
        {
            best = Some(mx.y);
        }
    }
    best
}

fn shadow_disc() -> Vec<Vec3> {
    (0..20)
        .map(|i| {
            let a = i as f32 / 20.0 * std::f32::consts::TAU;
            Vec3 { x: a.cos() * 0.5, y: 0.0, z: a.sin() * 0.5 }
        })
        .collect()
}

fn draw_shadow(canvas: &mut Canvas, viewport: Rect, view: &View, disc: &[Vec3], model: Mat4, strength: f32) {
    // Project the disc outline and fill it with a darkening blend (scanline polygon).
    let pd: Vec<(f32, f32, f32)> =
        disc.iter().filter_map(|p| raster::project_depth(view, viewport, model.transform_point(*p))).collect();
    if pd.len() < disc.len() {
        return;
    }
    let pts: Vec<(f32, f32)> = pd.iter().map(|p| (p.0, p.1)).collect();
    // Only darken pixels that are not in front of the shadow plane (occluders keep their colour).
    let min_lin = pd.iter().map(|p| raster::linear_depth(view, p.2)).fold(f32::INFINITY, f32::min) - 0.15;
    let min_y = pts.iter().map(|p| p.1).fold(f32::INFINITY, f32::min).max(viewport.y as f32) as i32;
    let max_y = pts.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max).min(viewport.bottom() as f32 - 1.0) as i32;
    let alpha = (strength.clamp(0.0, 1.0) * 255.0) as u32;
    let cx = pts.iter().map(|p| p.0).sum::<f32>() / pts.len() as f32;
    let cy = pts.iter().map(|p| p.1).sum::<f32>() / pts.len() as f32;
    let rx = pts.iter().map(|p| (p.0 - cx).abs()).fold(0.0, f32::max).max(1.0);
    let ry = pts.iter().map(|p| (p.1 - cy).abs()).fold(0.0, f32::max).max(1.0);
    for y in min_y..=max_y {
        let mut xs = Vec::new();
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            let yf = y as f32 + 0.5;
            if (a.1 <= yf && b.1 > yf) || (b.1 <= yf && a.1 > yf) {
                xs.push(a.0 + (yf - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let x0 = (xs[0].max(viewport.x as f32)) as i32;
        let x1 = (xs[xs.len() - 1].min(viewport.right() as f32 - 1.0)) as i32;
        for x in x0..=x1 {
            let dx = (x as f32 + 0.5 - cx) / rx;
            let dy = (y as f32 + 0.5 - cy) / ry;
            let fall = (1.0 - (dx * dx + dy * dy)).clamp(0.0, 1.0);
            let i = y as usize * canvas.width + x as usize;
            if raster::linear_depth(view, canvas.depth[i]) < min_lin {
                continue;
            }
            canvas.color[i] = blend(canvas.color[i], 0x000000, (alpha as f32 * fall) as u32);
        }
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
