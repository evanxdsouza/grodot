//! Software 3D rasterizer with textures and per-pixel lighting.
//!
//! The pipeline is a *visibility buffer*:
//!
//! 1. **Setup** transforms vertices, clips against the near plane, culls back
//!    faces (unless the material is double-sided) and stores each triangle's
//!    screen vertices plus its world-space attributes.
//! 2. **Raster** (parallel row bands) depth-tests every covered pixel and
//!    writes only the winning triangle's index. Alpha-cutout materials sample
//!    their texture here so cut-away pixels do not hide what is behind them.
//! 3. **Shade** (parallel row bands) visits each pixel once, rebuilds
//!    perspective-correct attributes from the triangle's plane equations,
//!    samples textures with mip selection from analytic UV derivatives, lights
//!    and fogs the result.
//! 4. **Blend** draws alpha-blended triangles back to front over the result.
//!
//! Interpolation is perspective-correct (attributes are weighted by 1/w), so
//! textures do not swim or warp the way affine mapping does.

use crate::canvas::{Canvas, Rect, Rgb};
use crate::texture::Texture;
use grodot_core::math::{Color, Mat4, Vec3};
use grodot_core::mesh::Mesh;
use grodot_core::scene::{AlphaMode, TextureFilter, TextureWrap};
use std::sync::Arc;

/// Camera matrices for one frame.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub view: Mat4,
    pub proj: Mat4,
    pub eye: Vec3,
    pub near: f32,
    pub far: f32,
}

impl View {
    pub fn view_proj(&self) -> Mat4 {
        self.proj * self.view
    }
}

/// A directional light (sun), optionally with a shadow map.
#[derive(Clone)]
pub struct DirLight {
    /// Direction the light travels (from the light towards the scene).
    pub direction: Vec3,
    pub color: Color,
    pub intensity: f32,
    /// The light asks for shadows (the renderer fills `shadow`).
    pub casts_shadows: bool,
    pub shadow: Option<Arc<ShadowMap>>,
}

/// A point light, or a spot light when `spot` is set.
#[derive(Debug, Clone, Copy)]
pub struct LocalLight {
    pub position: Vec3,
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub spot: Option<Spot>,
}

#[derive(Debug, Clone, Copy)]
pub struct Spot {
    /// Direction the cone points (normalized).
    pub direction: Vec3,
    /// Cosine of the half angle where the cone ends, and where it is at full strength.
    pub cos_outer: f32,
    pub cos_inner: f32,
}

impl Spot {
    pub fn new(direction: Vec3, full_angle_deg: f32) -> Spot {
        let half = (full_angle_deg * 0.5).to_radians();
        Spot { direction: direction.normalized(), cos_outer: half.cos(), cos_inner: (half * 0.8).cos() }
    }
}

/// Where lighting is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingMode {
    /// Every pixel is lit from its interpolated normal and position (the default).
    PerPixel,
    /// Lighting is computed at vertices and interpolated (Gouraud); the shadow
    /// lookup still happens per pixel. Faster on dense scenes, blurrier point lights.
    PerVertex,
}

/// Shadowed directional lights beyond this many are drawn without shadows.
pub const MAX_SHADOWED_SUNS: usize = 2;

#[derive(Clone)]
pub struct Lighting {
    pub suns: Vec<DirLight>,
    pub locals: Vec<LocalLight>,
    pub ambient: f32,
    pub fog_color: Color,
    pub fog_start: f32,
    pub fog_end: f32,
    pub mode: LightingMode,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            suns: vec![DirLight {
                direction: Vec3 { x: -0.4, y: -1.0, z: -0.3 }.normalized(),
                color: Color::WHITE,
                intensity: 1.0,
                casts_shadows: false,
                shadow: None,
            }],
            locals: Vec::new(),
            ambient: 0.35,
            fog_color: Color::rgb(0.6, 0.7, 0.85),
            fog_start: 60.0,
            fog_end: 200.0,
            mode: LightingMode::PerPixel,
        }
    }
}

/// A depth map rendered from a directional light (orthographic).
pub struct ShadowMap {
    pub size: usize,
    /// World -> light clip space (orthographic, so w = 1).
    pub view_proj: Mat4,
    /// Light-space NDC depth per texel; +inf where nothing was drawn.
    pub depth: Vec<f32>,
    /// World-space size of one texel (for normal-offset bias).
    pub texel_world: f32,
    /// NDC depth units per world metre along the light direction.
    pub depth_per_metre: f32,
}

impl ShadowMap {
    /// Fraction of light reaching world point `p` with normal `n`
    /// (1 = fully lit), using 3x3 bilinear-weighted PCF (16 depth taps).
    pub fn visibility(&self, p: Vec3, n: Vec3, to_light: Vec3) -> f32 {
        // Normal offset: push the lookup off the surface to avoid acne,
        // more at grazing angles where depth slopes are steep.
        let ndl = n.dot(to_light);
        let n = if ndl < 0.0 { -n } else { n };
        let slope = (1.0 - ndl.abs().min(1.0)).sqrt();
        let q = p + n * (self.texel_world * (0.8 + 1.5 * slope));
        let c = self.view_proj.transform_point(q);
        if c.x.abs() >= 1.0 || c.y.abs() >= 1.0 || c.z > 1.0 {
            return 1.0;
        }
        let size = self.size as f32;
        let u = (c.x * 0.5 + 0.5) * size - 0.5;
        let v = (1.0 - (c.y * 0.5 + 0.5)) * size - 0.5;
        let (iu, iv) = (u.floor() as i32, v.floor() as i32);
        let (fu, fv) = (u - iu as f32, v - iv as f32);
        let z = c.z - self.texel_world * self.depth_per_metre * 0.5;
        let wx = [1.0 - fu, 1.0, 1.0, fu];
        let wy = [1.0 - fv, 1.0, 1.0, fv];
        let n = self.size as i32;
        let mut lit = 0.0;
        for (dy, wyv) in wy.iter().enumerate() {
            let y = (iv - 1 + dy as i32).clamp(0, n - 1) as usize;
            let row = &self.depth[y * self.size..(y + 1) * self.size];
            for (dx, wxv) in wx.iter().enumerate() {
                let x = (iu - 1 + dx as i32).clamp(0, n - 1) as usize;
                if z <= row[x] {
                    lit += wxv * wyv;
                }
            }
        }
        lit / 9.0
    }
}

/// Render the depth of shadow-casting items as seen through `view_proj`
/// (an orthographic light camera) into a `size` x `size` shadow map. Reuses
/// the main rasterizer's setup and depth pass; faces are not culled so thin
/// and open meshes cast shadows too.
pub fn render_shadow_map(
    size: usize,
    view_proj: Mat4,
    texel_world: f32,
    depth_per_metre: f32,
    items: &[DrawItem],
) -> ShadowMap {
    let viewport = Rect::new(0, 0, size as i32, size as i32);
    let mut tris = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if item.cast_shadows && item.material.alpha_mode != AlphaMode::Blend {
            setup(item, i as u32, &view_proj, viewport, false, None, &mut tris);
        }
    }
    let mut depth = vec![f32::INFINITY; size * size];
    let mut vis = vec![u32::MAX; size * size];
    let n = threads();
    let band_h = size.div_ceil(n).max(16);
    let tris = &tris;
    std::thread::scope(|s| {
        for (i, (db, vb)) in depth.chunks_mut(band_h * size).zip(vis.chunks_mut(band_h * size)).enumerate() {
            let rows = db.len() / size;
            let band = Rect::new(0, (i * band_h) as i32, size as i32, rows as i32);
            let mut work = move || raster_band(db, vb, size, band, tris, items);
            if n == 1 {
                work();
            } else {
                s.spawn(work);
            }
        }
    });
    ShadowMap { size, view_proj, depth, texel_world, depth_per_metre }
}

/// A material ready for rendering (textures resolved).
#[derive(Clone)]
pub struct RMaterial {
    pub base_color: [f32; 4],
    pub texture: Option<Arc<Texture>>,
    pub uv_scale: [f32; 2],
    pub uv_offset: [f32; 2],
    pub filter: TextureFilter,
    pub wrap: TextureWrap,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    pub lit: bool,
    /// Added after lighting (glow). Multiplied by `emissive_texture` when set.
    pub emissive: [f32; 3],
    pub emissive_texture: Option<Arc<Texture>>,
}

impl RMaterial {
    pub fn solid(c: Color, lit: bool) -> RMaterial {
        RMaterial {
            base_color: [c.r, c.g, c.b, c.a],
            texture: None,
            uv_scale: [1.0, 1.0],
            uv_offset: [0.0, 0.0],
            filter: TextureFilter::Trilinear,
            wrap: TextureWrap::Repeat,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            lit,
            emissive: [0.0; 3],
            emissive_texture: None,
        }
    }

    fn glows(&self) -> bool {
        self.emissive != [0.0; 3]
    }
}

pub struct DrawItem<'a> {
    pub mesh: &'a Mesh,
    pub model: Mat4,
    pub material: RMaterial,
    /// Drawn into shadow maps.
    pub cast_shadows: bool,
}

impl<'a> DrawItem<'a> {
    pub fn new(mesh: &'a Mesh, model: Mat4, material: RMaterial) -> DrawItem<'a> {
        DrawItem { mesh, model, material, cast_shadows: true }
    }
}

/// A triangle after setup. Attributes are stored per vertex (not divided by w).
struct Tri {
    z: [f32; 3],
    iw: [f32; 3],
    uv: [[f32; 2]; 3],
    n: [Vec3; 3],
    p: [Vec3; 3],
    draw: u32,
    /// Rasterized from its back side (double-sided materials): the normal is flipped.
    back: bool,
    /// Per-vertex lighting (LightingMode::PerVertex only): unshadowed light
    /// in [0..3], then n.l x intensity for each shadowed sun.
    vl: Option<[[f32; VL]; 3]>,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    /// Edge-function coefficients: lambda_k = (ex[k] * px + ey[k] * py + ec[k]).
    ex: [f32; 3],
    ey: [f32; 3],
    ec: [f32; 3],
}

impl Tri {
    #[inline]
    fn lambdas(&self, px: f32, py: f32) -> [f32; 3] {
        [
            self.ex[0] * px + self.ey[0] * py + self.ec[0],
            self.ex[1] * px + self.ey[1] * py + self.ec[1],
            self.ex[2] * px + self.ey[2] * py + self.ec[2],
        ]
    }
}

/// Interpolated surface data for one pixel.
pub struct Frag {
    pub uv: [f32; 2],
    /// d(uv)/dx and d(uv)/dy in UV units per pixel (for mip selection).
    pub duv_dx: [f32; 2],
    pub duv_dy: [f32; 2],
    pub normal: Vec3,
    pub pos: Vec3,
    /// Interpolated per-vertex lighting, when the triangle has it.
    pub vl: Option<[f32; VL]>,
}

/// Size of the per-vertex lighting record.
const VL: usize = 3 + MAX_SHADOWED_SUNS;

#[inline]
fn fragment(t: &Tri, px: f32, py: f32) -> Frag {
    let l = t.lambdas(px, py);
    let wl = [l[0] * t.iw[0], l[1] * t.iw[1], l[2] * t.iw[2]];
    let iw = wl[0] + wl[1] + wl[2];
    let inv = if iw.abs() > 1e-30 { 1.0 / iw } else { 0.0 };
    let b = [wl[0] * inv, wl[1] * inv, wl[2] * inv];
    // u = Nu / iw with Nu, iw linear in screen space: du/dx = (dNu*iw - Nu*diw) / iw^2.
    let dwx = [t.ex[0] * t.iw[0], t.ex[1] * t.iw[1], t.ex[2] * t.iw[2]];
    let dwy = [t.ey[0] * t.iw[0], t.ey[1] * t.iw[1], t.ey[2] * t.iw[2]];
    let diw_dx = dwx[0] + dwx[1] + dwx[2];
    let diw_dy = dwy[0] + dwy[1] + dwy[2];
    let mut uv = [0.0; 2];
    let mut duv_dx = [0.0; 2];
    let mut duv_dy = [0.0; 2];
    for c in 0..2 {
        let nu = wl[0] * t.uv[0][c] + wl[1] * t.uv[1][c] + wl[2] * t.uv[2][c];
        let dnx = dwx[0] * t.uv[0][c] + dwx[1] * t.uv[1][c] + dwx[2] * t.uv[2][c];
        let dny = dwy[0] * t.uv[0][c] + dwy[1] * t.uv[1][c] + dwy[2] * t.uv[2][c];
        uv[c] = nu * inv;
        duv_dx[c] = (dnx * iw - nu * diw_dx) * inv * inv;
        duv_dy[c] = (dny * iw - nu * diw_dy) * inv * inv;
    }
    let mut normal = t.n[0] * b[0] + t.n[1] * b[1] + t.n[2] * b[2];
    if t.back {
        normal = -normal;
    }
    let vl = t.vl.map(|v| {
        let mut o = [0.0; VL];
        for (k, ok) in o.iter_mut().enumerate() {
            *ok = v[0][k] * b[0] + v[1][k] * b[1] + v[2][k] * b[2];
        }
        o
    });
    Frag { uv, duv_dx, duv_dy, normal, pos: t.p[0] * b[0] + t.p[1] * b[1] + t.p[2] * b[2], vl }
}

/// log2 of texels per pixel, the mip level to sample.
#[inline]
fn lod(tex: &Texture, f: &Frag) -> f32 {
    let (w, h) = (tex.width() as f32, tex.height() as f32);
    let ax = (f.duv_dx[0] * w).powi(2) + (f.duv_dx[1] * h).powi(2);
    let ay = (f.duv_dy[0] * w).powi(2) + (f.duv_dy[1] * h).powi(2);
    let m = ax.max(ay);
    if m <= 1e-12 {
        return 0.0;
    }
    0.5 * m.log2()
}

/// Base colour of a fragment (texture * base_color), with alpha.
#[inline]
fn albedo(m: &RMaterial, f: &Frag) -> [f32; 4] {
    let mut c = m.base_color;
    if let Some(tex) = &m.texture {
        let s = tex.sample(f.uv[0], f.uv[1], lod(tex, f), m.filter, m.wrap);
        c = [c[0] * s[0], c[1] * s[1], c[2] * s[2], c[3] * s[3]];
    }
    c
}

struct ShadeCtx<'a> {
    eye: Vec3,
    light: &'a Lighting,
    /// (to_light, colour x intensity, shadow map) per sun.
    suns: Vec<(Vec3, [f32; 3], Option<&'a ShadowMap>)>,
}

impl<'a> ShadeCtx<'a> {
    fn new(eye: Vec3, light: &'a Lighting) -> ShadeCtx<'a> {
        let mut shadowed = 0;
        let suns = light
            .suns
            .iter()
            .map(|s| {
                let c = [s.color.r * s.intensity, s.color.g * s.intensity, s.color.b * s.intensity];
                let map = match &s.shadow {
                    Some(m) if shadowed < MAX_SHADOWED_SUNS => {
                        shadowed += 1;
                        Some(&**m)
                    }
                    _ => None,
                };
                ((-s.direction).normalized(), c, map)
            })
            .collect();
        ShadeCtx { eye, light, suns }
    }

    /// Light arriving at a surface, without shadow-mapped suns' shadows applied:
    /// returns (light from everything except shadowed suns, n.l per shadowed sun).
    #[inline]
    fn gather(&self, n: Vec3, p: Vec3) -> [f32; VL] {
        let mut o = [0.0; VL];
        let sky = 0.5 + 0.5 * n.y; // hemisphere ambient
        let amb = self.light.ambient * (0.65 + 0.35 * sky);
        o[0] = amb;
        o[1] = amb;
        o[2] = amb;
        let mut si = 0;
        for (to_light, c, map) in &self.suns {
            let d = n.dot(*to_light).max(0.0);
            if map.is_some() {
                o[3 + si] = d;
                si += 1;
            } else {
                o[0] += d * c[0];
                o[1] += d * c[1];
                o[2] += d * c[2];
            }
        }
        for l in &self.light.locals {
            let v = l.position - p;
            let d2 = v.dot(v);
            let r2 = l.range * l.range;
            if d2 >= r2 || d2 < 1e-8 {
                continue;
            }
            let dist = d2.sqrt();
            let dir = v * (1.0 / dist);
            let ndl = n.dot(dir);
            if ndl <= 0.0 {
                continue;
            }
            // Smooth window: full strength at the bulb, zero at `range`.
            let w = 1.0 - d2 / r2;
            let mut att = w * w * l.intensity * ndl;
            if let Some(s) = &l.spot {
                let cd = -dir.dot(s.direction);
                if cd <= s.cos_outer {
                    continue;
                }
                let t = ((cd - s.cos_outer) / (s.cos_inner - s.cos_outer).max(1e-4)).min(1.0);
                att *= t * t * (3.0 - 2.0 * t);
            }
            o[0] += att * l.color.r;
            o[1] += att * l.color.g;
            o[2] += att * l.color.b;
        }
        o
    }
}

#[inline]
fn shade(ctx: &ShadeCtx, m: &RMaterial, f: &Frag, base: [f32; 4]) -> [f32; 3] {
    let (mut r, mut g, mut b) = (base[0], base[1], base[2]);
    let light = ctx.light;
    if m.lit {
        let n = f.normal.normalized();
        let acc = match f.vl {
            Some(v) => v,
            None => ctx.gather(n, f.pos),
        };
        let (mut lr, mut lg, mut lb) = (acc[0], acc[1], acc[2]);
        let mut si = 0;
        for (to_light, c, map) in &ctx.suns {
            if let Some(map) = map {
                let d = acc[3 + si];
                si += 1;
                if d > 0.0 {
                    let vis = map.visibility(f.pos, n, *to_light);
                    lr += d * vis * c[0];
                    lg += d * vis * c[1];
                    lb += d * vis * c[2];
                }
            }
        }
        r *= lr;
        g *= lg;
        b *= lb;
    }
    if m.glows() {
        let mut e = m.emissive;
        if let Some(tex) = &m.emissive_texture {
            let s = tex.sample(f.uv[0], f.uv[1], lod(tex, f), m.filter, m.wrap);
            e = [e[0] * s[0], e[1] * s[1], e[2] * s[2]];
        }
        r += e[0];
        g += e[1];
        b += e[2];
    }
    let dist = (f.pos - ctx.eye).length();
    let fog = ((dist - light.fog_start) / (light.fog_end - light.fog_start).max(0.001)).clamp(0.0, 1.0);
    [r + (light.fog_color.r - r) * fog, g + (light.fog_color.g - g) * fog, b + (light.fog_color.b - b) * fog]
}

#[derive(Clone, Copy)]
struct CV {
    c: [f32; 4],
    uv: [f32; 2],
    n: Vec3,
    p: Vec3,
    vl: [f32; VL],
}

fn lerp_cv(a: &CV, b: &CV, t: f32) -> CV {
    let l = |x: f32, y: f32| x + (y - x) * t;
    CV {
        c: [l(a.c[0], b.c[0]), l(a.c[1], b.c[1]), l(a.c[2], b.c[2]), l(a.c[3], b.c[3])],
        uv: [l(a.uv[0], b.uv[0]), l(a.uv[1], b.uv[1])],
        n: a.n + (b.n - a.n) * t,
        p: a.p + (b.p - a.p) * t,
        vl: std::array::from_fn(|k| l(a.vl[k], b.vl[k])),
    }
}

const NEAR_EPS: f32 = 1e-5;

/// Clip a polygon against the near plane (z >= -w).
fn clip_near(poly: &[CV]) -> Vec<CV> {
    let mut out = Vec::with_capacity(4);
    let d = |v: &CV| v.c[2] + v.c[3] - NEAR_EPS;
    for i in 0..poly.len() {
        let a = &poly[i];
        let b = &poly[(i + 1) % poly.len()];
        let (da, db) = (d(a), d(b));
        if da >= 0.0 {
            out.push(*a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            out.push(lerp_cv(a, b, da / (da - db)));
        }
    }
    out
}

fn normal_matrix(model: &Mat4) -> Mat4 {
    let inv = model.inverse();
    let mut t = Mat4::IDENTITY;
    for c in 0..3 {
        for r in 0..3 {
            t.c[c][r] = inv.c[r][c];
        }
    }
    t
}

/// Transform, clip and cull one item into screen triangles. With `vertex_light`
/// set (per-vertex mode) lighting is gathered at each vertex. With `cull` false
/// every face is kept (shadow maps).
fn setup(
    item: &DrawItem,
    draw: u32,
    vp: &Mat4,
    viewport: Rect,
    cull: bool,
    vertex_light: Option<&ShadeCtx>,
    out: &mut Vec<Tri>,
) {
    let (vx, vy, vw, vh) = (viewport.x as f32, viewport.y as f32, viewport.w as f32, viewport.h as f32);
    let mvp = *vp * item.model;
    let nm = normal_matrix(&item.model);
    let m = &item.material;
    let mesh = item.mesh;
    let vertex_light = vertex_light.filter(|_| m.lit);
    let verts: Vec<CV> = (0..mesh.positions.len())
        .map(|i| {
            let p = mesh.positions[i];
            let c = mvp.transform4(p);
            let uv = mesh.uvs.get(i).copied().unwrap_or_default();
            let n = nm.transform_vector(mesh.normals.get(i).copied().unwrap_or(Vec3::Y));
            let wp = item.model.transform_point(p);
            CV {
                c: [c.x, c.y, c.z, c.w],
                uv: [uv.x * m.uv_scale[0] + m.uv_offset[0], uv.y * m.uv_scale[1] + m.uv_offset[1]],
                vl: match vertex_light {
                    Some(ctx) => ctx.gather(n.normalized(), wp),
                    None => [0.0; VL],
                },
                n,
                p: wp,
            }
        })
        .collect();
    for tri in mesh.indices.chunks_exact(3) {
        let (Some(a), Some(b), Some(c)) =
            (verts.get(tri[0] as usize), verts.get(tri[1] as usize), verts.get(tri[2] as usize))
        else {
            continue;
        };
        let out_all = |f: &dyn Fn(&[f32; 4]) -> bool| f(&a.c) && f(&b.c) && f(&c.c);
        if out_all(&|v| v[0] > v[3])
            || out_all(&|v| v[0] < -v[3])
            || out_all(&|v| v[1] > v[3])
            || out_all(&|v| v[1] < -v[3])
            || out_all(&|v| v[2] > v[3])
        {
            continue;
        }
        let poly = if a.c[2] + a.c[3] < NEAR_EPS || b.c[2] + b.c[3] < NEAR_EPS || c.c[2] + c.c[3] < NEAR_EPS {
            clip_near(&[*a, *b, *c])
        } else {
            vec![*a, *b, *c]
        };
        if poly.len() < 3 {
            continue;
        }
        let sv: Vec<(f32, f32, f32, f32)> = poly
            .iter()
            .map(|v| {
                let iw = 1.0 / v.c[3];
                (vx + (v.c[0] * iw * 0.5 + 0.5) * vw, vy + (1.0 - (v.c[1] * iw * 0.5 + 0.5)) * vh, v.c[2] * iw, iw)
            })
            .collect();
        for k in 1..sv.len() - 1 {
            let mut idx = [0, k, k + 1];
            let s = |i: usize| sv[idx[i]];
            let area = (s(1).0 - s(0).0) * (s(2).1 - s(0).1) - (s(2).0 - s(0).0) * (s(1).1 - s(0).1);
            if area.abs() < 1e-9 {
                continue;
            }
            // Front faces are counter-clockwise, which is negative area with y pointing down.
            let mut back = false;
            if area > 0.0 {
                if cull && !m.double_sided {
                    continue;
                }
                idx.swap(1, 2);
                back = true;
            }
            let p = [sv[idx[0]], sv[idx[1]], sv[idx[2]]];
            let x = [p[0].0, p[1].0, p[2].0];
            let y = [p[0].1, p[1].1, p[2].1];
            let min_x = x.iter().fold(f32::INFINITY, |a, &b| a.min(b)).floor().max(viewport.x as f32) as i32;
            let max_x =
                x.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b)).ceil().min((viewport.right() - 1) as f32) as i32;
            let min_y = y.iter().fold(f32::INFINITY, |a, &b| a.min(b)).floor().max(viewport.y as f32) as i32;
            let max_y =
                y.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b)).ceil().min((viewport.bottom() - 1) as f32) as i32;
            if min_x > max_x || min_y > max_y {
                continue;
            }
            // lambda_0 is the weight of vertex 0: edge (1 -> 2); positive inside after the winding fix.
            let area = (x[1] - x[0]) * (y[2] - y[0]) - (x[2] - x[0]) * (y[1] - y[0]);
            let ia = -1.0 / area;
            let edge = |i: usize, j: usize| {
                // f(p) = (p.x - xi) * (yj - yi) - (p.y - yi) * (xj - xi), scaled by ia
                let ex = (y[j] - y[i]) * ia;
                let ey = -(x[j] - x[i]) * ia;
                (ex, ey, -(x[i] * ex + y[i] * ey))
            };
            let e0 = edge(1, 2);
            let e1 = edge(2, 0);
            let e2 = edge(0, 1);
            let pv = |i: usize| poly[idx[i]];
            out.push(Tri {
                z: [p[0].2, p[1].2, p[2].2],
                iw: [p[0].3, p[1].3, p[2].3],
                uv: [pv(0).uv, pv(1).uv, pv(2).uv],
                n: [pv(0).n, pv(1).n, pv(2).n],
                p: [pv(0).p, pv(1).p, pv(2).p],
                draw,
                back,
                vl: vertex_light.map(|_| [pv(0).vl, pv(1).vl, pv(2).vl]),
                min_x,
                max_x,
                min_y,
                max_y,
                ex: [e0.0, e1.0, e2.0],
                ey: [e0.1, e1.1, e2.1],
                ec: [e0.2, e1.2, e2.2],
            });
        }
    }
}

/// Number of worker bands used for parallel passes.
fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 16)
}

/// Renders a list of meshes into the canvas region `viewport`.
///
/// The background (and anything else drawn earlier) stays where no geometry lands.
pub fn draw_meshes(canvas: &mut Canvas, viewport: Rect, view: &View, light: &Lighting, items: &[DrawItem]) {
    let vpr = viewport.intersect(&canvas.full_rect());
    if vpr.w <= 0 || vpr.h <= 0 || items.is_empty() {
        return;
    }
    let vp = view.view_proj();
    let ctx = ShadeCtx::new(view.eye, light);
    let vertex_light = (light.mode == LightingMode::PerVertex).then_some(&ctx);
    let mut opaque: Vec<Tri> = Vec::new();
    let mut blended: Vec<(f32, usize)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        if item.material.alpha_mode == AlphaMode::Blend {
            let centre = item.model.transform_point({
                let (lo, hi) = item.mesh.bounds();
                (lo + hi) * 0.5
            });
            blended.push(((centre - view.eye).length(), i));
        } else {
            setup(item, i as u32, &vp, viewport, true, vertex_light, &mut opaque);
        }
    }
    // Back to front; ties keep submission order so results are deterministic.
    blended.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    let mut blend_tris: Vec<Tri> = Vec::new();
    for &(_, i) in &blended {
        setup(&items[i], i as u32, &vp, viewport, true, vertex_light, &mut blend_tris);
    }
    let ctx = &ctx;

    let width = canvas.width;
    let n = threads();
    let band_h = (vpr.h as usize).div_ceil(n).max(16);
    let start = vpr.y as usize * width;
    let end = vpr.bottom() as usize * width;
    let mut vis = vec![u32::MAX; end - start];
    let color = &mut canvas.color[start..end];
    let depth = &mut canvas.depth[start..end];
    let opaque = &opaque;
    let blend_tris = &blend_tris;
    std::thread::scope(|s| {
        let bands =
            color.chunks_mut(band_h * width).zip(depth.chunks_mut(band_h * width)).zip(vis.chunks_mut(band_h * width));
        for (i, ((cb, db), vb)) in bands.enumerate() {
            let rows = cb.len() / width;
            let band = Rect::new(vpr.x, vpr.y + (i * band_h) as i32, vpr.w, rows as i32);
            let mut work = move || {
                raster_band(db, vb, width, band, opaque, items);
                shade_band(cb, vb, width, band, opaque, items, ctx);
                blend_band(cb, db, width, band, blend_tris, items, ctx);
            };
            if n == 1 {
                work();
            } else {
                s.spawn(work);
            }
        }
    });
}

#[inline]
fn row_range(t: &Tri, band: Rect) -> Option<(i32, i32)> {
    let y0 = t.min_y.max(band.y);
    let y1 = t.max_y.min(band.bottom() - 1);
    if y0 > y1 {
        None
    } else {
        Some((y0, y1))
    }
}

fn raster_band(depth: &mut [f32], vis: &mut [u32], width: usize, band: Rect, tris: &[Tri], items: &[DrawItem]) {
    for (ti, t) in tris.iter().enumerate() {
        let Some((y0, y1)) = row_range(t, band) else { continue };
        let mat = &items[t.draw as usize].material;
        let cutout = mat.alpha_mode == AlphaMode::Cutout;
        for y in y0..=y1 {
            let py = y as f32 + 0.5;
            let px0 = t.min_x as f32 + 0.5;
            let mut l = t.lambdas(px0, py);
            let row = (y - band.y) as usize * width;
            for x in t.min_x..=t.max_x {
                if l[0] >= 0.0 && l[1] >= 0.0 && l[2] >= 0.0 {
                    let z = t.z[0] * l[0] + t.z[1] * l[1] + t.z[2] * l[2];
                    let i = row + x as usize;
                    if z < depth[i] && z >= -1.0 {
                        let keep = !cutout || {
                            let f = fragment(t, x as f32 + 0.5, py);
                            albedo(mat, &f)[3] >= mat.alpha_cutoff
                        };
                        if keep {
                            depth[i] = z;
                            vis[i] = ti as u32;
                        }
                    }
                }
                l[0] += t.ex[0];
                l[1] += t.ex[1];
                l[2] += t.ex[2];
            }
        }
    }
}

fn shade_band(
    color: &mut [Rgb],
    vis: &[u32],
    width: usize,
    band: Rect,
    tris: &[Tri],
    items: &[DrawItem],
    ctx: &ShadeCtx,
) {
    for y in band.y..band.bottom() {
        let row = (y - band.y) as usize * width;
        for x in band.x..band.right() {
            let i = row + x as usize;
            let id = vis[i];
            if id == u32::MAX {
                continue;
            }
            let t = &tris[id as usize];
            let mat = &items[t.draw as usize].material;
            let f = fragment(t, x as f32 + 0.5, y as f32 + 0.5);
            let base = albedo(mat, &f);
            let c = shade(ctx, mat, &f, base);
            color[i] = pack(c[0], c[1], c[2]);
        }
    }
}

fn blend_band(
    color: &mut [Rgb],
    depth: &[f32],
    width: usize,
    band: Rect,
    tris: &[Tri],
    items: &[DrawItem],
    ctx: &ShadeCtx,
) {
    for t in tris {
        let Some((y0, y1)) = row_range(t, band) else { continue };
        let mat = &items[t.draw as usize].material;
        for y in y0..=y1 {
            let py = y as f32 + 0.5;
            let mut l = t.lambdas(t.min_x as f32 + 0.5, py);
            let row = (y - band.y) as usize * width;
            for x in t.min_x..=t.max_x {
                if l[0] >= 0.0 && l[1] >= 0.0 && l[2] >= 0.0 {
                    let z = t.z[0] * l[0] + t.z[1] * l[1] + t.z[2] * l[2];
                    let i = row + x as usize;
                    if z < depth[i] && z >= -1.0 {
                        let f = fragment(t, x as f32 + 0.5, py);
                        let base = albedo(mat, &f);
                        let a = base[3].clamp(0.0, 1.0);
                        if a > 0.002 {
                            let c = shade(ctx, mat, &f, base);
                            let src = pack(c[0], c[1], c[2]);
                            color[i] = crate::canvas::blend(color[i], src, (a * 255.0 + 0.5) as u32);
                        }
                    }
                }
                l[0] += t.ex[0];
                l[1] += t.ex[1];
                l[2] += t.ex[2];
            }
        }
    }
}

#[inline]
pub fn pack(r: f32, g: f32, b: f32) -> Rgb {
    // Simple filmic-ish tone curve keeps bright highlights from clipping harshly.
    let tc = |v: f32| {
        let v = v.max(0.0);
        let v = v / (1.0 + 0.3 * v) * 1.15;
        (v.min(1.0) * 255.0 + 0.5) as u32
    };
    (tc(r) << 16) | (tc(g) << 8) | tc(b)
}

pub fn color_to_rgb(c: Color) -> Rgb {
    c.to_u32()
}

/// Draw a 3D line segment with optional depth testing.
pub fn draw_line_3d(canvas: &mut Canvas, viewport: Rect, view: &View, a: Vec3, b: Vec3, color: Rgb, depth_test: bool) {
    let vp = view.view_proj();
    let mut ca = vp.transform4(a);
    let mut cb = vp.transform4(b);
    let da = ca.z + ca.w - NEAR_EPS;
    let db = cb.z + cb.w - NEAR_EPS;
    if da < 0.0 && db < 0.0 {
        return;
    }
    let lerp4 = |p: grodot_core::math::Vec4, q: grodot_core::math::Vec4, t: f32| grodot_core::math::Vec4 {
        x: p.x + (q.x - p.x) * t,
        y: p.y + (q.y - p.y) * t,
        z: p.z + (q.z - p.z) * t,
        w: p.w + (q.w - p.w) * t,
    };
    if da < 0.0 {
        ca = lerp4(ca, cb, da / (da - db));
    } else if db < 0.0 {
        cb = lerp4(cb, ca, db / (db - da));
    }
    let proj = |c: grodot_core::math::Vec4| {
        let iw = 1.0 / c.w;
        (
            viewport.x as f32 + (c.x * iw * 0.5 + 0.5) * viewport.w as f32,
            viewport.y as f32 + (1.0 - (c.y * iw * 0.5 + 0.5)) * viewport.h as f32,
            c.z * iw,
        )
    };
    let (x0, y0, z0) = proj(ca);
    let (x1, y1, z1) = proj(cb);
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil() as i32;
    if steps <= 0 || steps > 8000 {
        return;
    }
    let old_clip = canvas.clip;
    canvas.set_clip(viewport.intersect(&old_clip));
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = (x0 + (x1 - x0) * t) as i32;
        let y = (y0 + (y1 - y0) * t) as i32;
        let z = z0 + (z1 - z0) * t;
        if !canvas.clip.contains(x, y) {
            continue;
        }
        let idx = y as usize * canvas.width + x as usize;
        if !depth_test || z <= canvas.depth[idx] + 0.0005 {
            canvas.color[idx] = color;
        }
    }
    canvas.clip = old_clip;
}

/// Project a world point; returns pixel coordinates and NDC depth.
pub fn project_depth(view: &View, viewport: Rect, p: Vec3) -> Option<(f32, f32, f32)> {
    let c = view.view_proj().transform4(p);
    if c.w <= 1e-4 {
        return None;
    }
    let iw = 1.0 / c.w;
    Some((
        viewport.x as f32 + (c.x * iw * 0.5 + 0.5) * viewport.w as f32,
        viewport.y as f32 + (1.0 - (c.y * iw * 0.5 + 0.5)) * viewport.h as f32,
        c.z * iw,
    ))
}

/// Convert NDC depth (-1..1) to linear view distance.
pub fn linear_depth(view: &View, z: f32) -> f32 {
    let (n, f) = (view.near.max(0.01), view.far.max(view.near + 0.1));
    2.0 * n * f / (f + n - z * (f - n))
}

/// Project a world point to viewport pixel coordinates (None if behind the camera).
pub fn project(view: &View, viewport: Rect, p: Vec3) -> Option<(f32, f32)> {
    project_depth(view, viewport, p).map(|(x, y, _)| (x, y))
}

/// World-space ray through a viewport pixel.
pub fn screen_ray(view: &View, viewport: Rect, px: f32, py: f32) -> (Vec3, Vec3) {
    let nx = ((px - viewport.x as f32) / viewport.w as f32) * 2.0 - 1.0;
    let ny = 1.0 - ((py - viewport.y as f32) / viewport.h as f32) * 2.0;
    let inv = view.view_proj().inverse();
    let unproject = |z: f32| {
        let c = inv.transform4(Vec3 { x: nx, y: ny, z });
        Vec3 { x: c.x / c.w, y: c.y / c.w, z: c.z / c.w }
    };
    let a = unproject(-1.0);
    let b = unproject(1.0);
    (a, (b - a).normalized())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grodot_core::image::Image;
    use grodot_core::math::{vec3, Quat};

    fn view_looking_down_z(w: i32, h: i32) -> (View, Rect) {
        let vpr = Rect::new(0, 0, w, h);
        let world = Mat4::from_trs(vec3(0.0, 0.0, 3.0), Quat::IDENTITY, Vec3::ONE);
        (crate::scene_render::view_from_matrix(&world, 60.0, 0.1, 100.0, vpr), vpr)
    }

    fn textured(tex: Image) -> RMaterial {
        let mut m = RMaterial::solid(Color::WHITE, false);
        m.texture = Some(Arc::new(Texture::from_image(&tex)));
        m
    }

    #[test]
    fn perspective_correct_uvs_on_a_receding_plane() {
        // A plane tilted away from the camera: with perspective-correct mapping the
        // texture's halfway line (u = 0.5) lands where the plane's middle projects,
        // not halfway across the screen-space triangle.
        let mut rgba = Vec::new();
        for _y in 0..2 {
            rgba.extend_from_slice(&[255, 0, 0, 255, 0, 0, 255, 255]); // left red, right blue
        }
        let mut mat = textured(Image::new(2, 2, rgba));
        mat.filter = TextureFilter::Nearest;
        mat.double_sided = true;
        let mesh = Mesh::plane();
        // Rotate the plane so +X recedes into the distance.
        let model = Mat4::from_trs(
            vec3(0.0, 0.0, 0.0),
            Quat::from_euler_deg(vec3(0.0, 70.0, 0.0)) * Quat::from_euler_deg(vec3(90.0, 0.0, 0.0)),
            vec3(4.0, 1.0, 4.0),
        );
        let (view, vpr) = view_looking_down_z(200, 100);
        let mut canvas = Canvas::new(200, 100);
        canvas.clear(0);
        draw_meshes(&mut canvas, vpr, &view, &Lighting::default(), &[DrawItem::new(&mesh, model, mat)]);
        // Where does the plane's middle (u = 0.5) project?
        let mid = raster_mid_x(&view, vpr, &model);
        let row = 50;
        // Pixels just left of the projected middle are red, just right are blue.
        let left = canvas.color[row * 200 + (mid - 3.0) as usize];
        let right = canvas.color[row * 200 + (mid + 3.0) as usize];
        assert!(left >> 16 > 100 && left & 0xff < 60, "left of centre {:06x}", left);
        assert!(right & 0xff > 100 && right >> 16 < 60, "right of centre {:06x}", right);
    }

    fn raster_mid_x(view: &View, vpr: Rect, model: &Mat4) -> f32 {
        project(view, vpr, model.transform_point(Vec3::ZERO)).unwrap().0
    }

    #[test]
    fn cutout_discards_transparent_pixels() {
        let mut rgba = vec![0u8; 4 * 4 * 4];
        for (i, p) in rgba.chunks_exact_mut(4).enumerate() {
            let x = i % 4;
            p.copy_from_slice(&[0, 255, 0, if x < 2 { 255 } else { 0 }]);
        }
        let mut mat = textured(Image::new(4, 4, rgba));
        mat.alpha_mode = AlphaMode::Cutout;
        mat.filter = TextureFilter::Nearest;
        let mesh = Mesh::quad_xy();
        let (view, vpr) = view_looking_down_z(100, 100);
        let mut canvas = Canvas::new(100, 100);
        canvas.clear(0x112233);
        let model = Mat4::from_trs(Vec3::ZERO, Quat::IDENTITY, vec3(2.0, 2.0, 1.0));
        draw_meshes(&mut canvas, vpr, &view, &Lighting::default(), &[DrawItem::new(&mesh, model, mat)]);
        let (cx, _) = project(&view, vpr, Vec3::ZERO).unwrap();
        let l = canvas.color[50 * 100 + (cx - 10.0) as usize];
        let r = canvas.color[50 * 100 + (cx + 10.0) as usize];
        assert!((l >> 8) & 0xff > 150, "opaque half is drawn: {:06x}", l);
        assert_eq!(r, 0x112233, "transparent half is cut away");
        assert!(canvas.depth[50 * 100 + (cx + 10.0) as usize].is_infinite(), "and does not write depth");
    }

    #[test]
    fn blend_mixes_with_background() {
        let mut mat = RMaterial::solid(Color { r: 1.0, g: 1.0, b: 1.0, a: 0.5 }, false);
        mat.alpha_mode = AlphaMode::Blend;
        let mesh = Mesh::quad_xy();
        let (view, vpr) = view_looking_down_z(40, 40);
        let mut canvas = Canvas::new(40, 40);
        canvas.clear(0x000000);
        let model = Mat4::from_trs(Vec3::ZERO, Quat::IDENTITY, Vec3::ONE);
        draw_meshes(&mut canvas, vpr, &view, &Lighting::default(), &[DrawItem::new(&mesh, model, mat)]);
        let c = canvas.color[20 * 40 + 20];
        let r = (c >> 16) & 0xff;
        assert!((110..=150).contains(&r), "half-transparent white over black is grey, got {}", r);
    }

    #[test]
    fn back_faces_culled_unless_double_sided() {
        let mesh = Mesh::quad_xy();
        let (view, vpr) = view_looking_down_z(40, 40);
        let model = Mat4::from_trs(Vec3::ZERO, Quat::from_euler_deg(vec3(0.0, 180.0, 0.0)), Vec3::ONE);
        for double in [false, true] {
            let mut mat = RMaterial::solid(Color::WHITE, false);
            mat.double_sided = double;
            let mut canvas = Canvas::new(40, 40);
            canvas.clear(0);
            draw_meshes(&mut canvas, vpr, &view, &Lighting::default(), &[DrawItem::new(&mesh, model, mat)]);
            assert_eq!(canvas.color[20 * 40 + 20] != 0, double);
        }
    }

    #[test]
    fn sun_shadow_blocks_light_behind_a_caster() {
        // A cube hangs over the origin; the sun comes in tilted towards +X.
        let floor = Mesh::plane();
        let cube = Mesh::primitive("cube").unwrap();
        let floor_model = Mat4::from_trs(Vec3::ZERO, Quat::IDENTITY, vec3(20.0, 1.0, 20.0));
        let cube_model = Mat4::from_trs(vec3(0.0, 1.0, 0.0), Quat::IDENTITY, Vec3::ONE);
        let mut floor_item = DrawItem::new(&floor, floor_model, RMaterial::solid(Color::WHITE, true));
        floor_item.cast_shadows = false;
        let items = [floor_item, DrawItem::new(&cube, cube_model, RMaterial::solid(Color::WHITE, true))];
        let (view, _) = view_looking_down_z(100, 100);
        let dir = vec3(0.3, -1.0, 0.0).normalized();
        let (vp, texel, dpm) = crate::scene_render::shadow_camera(&view, dir, 512, 40.0);
        let map = render_shadow_map(512, vp, texel, dpm, &items);
        let to_light = (-dir).normalized();
        let up = Vec3::Y;
        // Directly under the cube: the light path crosses it, so the point is in shadow.
        let shadowed = map.visibility(Vec3::ZERO, up, to_light);
        assert!(shadowed < 0.5, "under the cube should be shadowed, got {}", shadowed);
        // Well away from the cube's shadow: fully lit.
        let lit = map.visibility(vec3(6.0, 0.0, 0.0), up, to_light);
        assert!(lit > 0.99, "clear floor should be lit, got {}", lit);
    }

    #[test]
    fn point_light_falls_off_to_zero_at_its_range() {
        let mut lighting = Lighting { suns: Vec::new(), ambient: 0.0, ..Default::default() };
        lighting.locals.push(LocalLight {
            position: vec3(0.0, 1.0, 0.0),
            color: Color::WHITE,
            intensity: 1.0,
            range: 2.0,
            spot: None,
        });
        let ctx = ShadeCtx::new(vec3(0.0, 0.0, 10.0), &lighting);
        let near = ctx.gather(Vec3::Y, vec3(0.0, 0.0, 0.0))[0];
        let far = ctx.gather(Vec3::Y, vec3(0.0, 2.5, 0.0))[0];
        assert!(near > 0.5, "a point near the bulb is bright, got {}", near);
        assert_eq!(far, 0.0, "beyond the range there is no light");
    }
}
