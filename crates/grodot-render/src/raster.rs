//! Software 3D rasterizer: clipping, depth testing, Gouraud lighting and fog.
//!
//! Triangles are set up once, then rasterized in parallel horizontal bands
//! (one per CPU core) so the renderer stays fast enough for small games.

use crate::canvas::{Canvas, Rect, Rgb};
use grodot_core::math::{Color, Mat4, Vec3};
use grodot_core::mesh::Mesh;

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

#[derive(Debug, Clone, Copy)]
pub struct Lighting {
    /// Direction the light travels (from the light towards the scene).
    pub direction: Vec3,
    pub color: Color,
    pub intensity: f32,
    pub ambient: f32,
    pub fog_color: Color,
    pub fog_start: f32,
    pub fog_end: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Lighting {
            direction: Vec3 { x: -0.4, y: -1.0, z: -0.3 }.normalized(),
            color: Color::WHITE,
            intensity: 1.0,
            ambient: 0.35,
            fog_color: Color::rgb(0.6, 0.7, 0.85),
            fog_start: 60.0,
            fog_end: 200.0,
        }
    }
}

pub struct DrawItem<'a> {
    pub mesh: &'a Mesh,
    pub model: Mat4,
    pub color: Color,
    pub lit: bool,
}

#[derive(Clone, Copy)]
struct SV {
    x: f32,
    y: f32,
    z: f32,
    iw: f32,
    r: f32,
    g: f32,
    b: f32,
}

#[derive(Clone, Copy)]
struct Tri {
    v: [SV; 3],
    min_y: i32,
    max_y: i32,
    min_x: i32,
    max_x: i32,
}

#[derive(Clone, Copy)]
struct CV {
    // clip-space position
    x: f32,
    y: f32,
    z: f32,
    w: f32,
    r: f32,
    g: f32,
    b: f32,
}

fn lerp_cv(a: &CV, b: &CV, t: f32) -> CV {
    CV {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
        w: a.w + (b.w - a.w) * t,
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
    }
}

const NEAR_EPS: f32 = 1e-5;

/// Clip a polygon against the near plane (z >= -w).
fn clip_near(poly: &[CV]) -> Vec<CV> {
    let mut out = Vec::with_capacity(4);
    let d = |v: &CV| v.z + v.w - NEAR_EPS;
    for i in 0..poly.len() {
        let a = &poly[i];
        let b = &poly[(i + 1) % poly.len()];
        let (da, db) = (d(a), d(b));
        if da >= 0.0 {
            out.push(*a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(lerp_cv(a, b, t));
        }
    }
    out
}

/// Renders a list of meshes into the canvas region `viewport`.
pub fn draw_meshes(canvas: &mut Canvas, viewport: Rect, view: &View, light: &Lighting, items: &[DrawItem]) {
    let vp = view.view_proj();
    let mut tris: Vec<Tri> = Vec::new();
    let (vx, vy, vw, vh) = (viewport.x as f32, viewport.y as f32, viewport.w as f32, viewport.h as f32);
    let to_light = (-light.direction).normalized();
    for item in items {
        let mvp = vp * item.model;
        let normal_m = {
            let inv = item.model.inverse();
            // transpose of inverse (upper 3x3)
            let mut t = Mat4::IDENTITY;
            for c in 0..3 {
                for r in 0..3 {
                    t.c[c][r] = inv.c[r][c];
                }
            }
            t
        };
        let base = item.color;
        // Per-vertex lighting
        let mut cverts: Vec<CV> = Vec::with_capacity(item.mesh.positions.len());
        for (p, n) in item.mesh.positions.iter().zip(item.mesh.normals.iter()) {
            let wp = item.model.transform_point(*p);
            let (mut r, mut g, mut b) = (base.r, base.g, base.b);
            if item.lit {
                let wn = normal_m.transform_vector(*n).normalized();
                let diff = wn.dot(to_light).max(0.0) * light.intensity;
                let sky = 0.5 + 0.5 * wn.y; // hemisphere ambient
                let amb = light.ambient * (0.65 + 0.35 * sky);
                r *= amb + diff * light.color.r;
                g *= amb + diff * light.color.g;
                b *= amb + diff * light.color.b;
            }
            let dist = (wp - view.eye).length();
            let f = ((dist - light.fog_start) / (light.fog_end - light.fog_start).max(0.001)).clamp(0.0, 1.0);
            r = r + (light.fog_color.r - r) * f;
            g = g + (light.fog_color.g - g) * f;
            b = b + (light.fog_color.b - b) * f;
            let c = mvp.transform4(*p);
            cverts.push(CV { x: c.x, y: c.y, z: c.z, w: c.w, r, g, b });
        }
        for tri in item.mesh.indices.chunks_exact(3) {
            let a = cverts[tri[0] as usize];
            let b = cverts[tri[1] as usize];
            let c = cverts[tri[2] as usize];
            // Trivial reject: all outside the same frustum plane.
            let out = |f: &dyn Fn(&CV) -> bool| f(&a) && f(&b) && f(&c);
            if out(&|v| v.x > v.w) || out(&|v| v.x < -v.w) || out(&|v| v.y > v.w) || out(&|v| v.y < -v.w) || out(&|v| v.z > v.w) {
                continue;
            }
            let poly = if a.z + a.w < NEAR_EPS || b.z + b.w < NEAR_EPS || c.z + c.w < NEAR_EPS {
                clip_near(&[a, b, c])
            } else {
                vec![a, b, c]
            };
            if poly.len() < 3 {
                continue;
            }
            let sv: Vec<SV> = poly
                .iter()
                .map(|v| {
                    let iw = 1.0 / v.w;
                    SV {
                        x: vx + (v.x * iw * 0.5 + 0.5) * vw,
                        y: vy + (1.0 - (v.y * iw * 0.5 + 0.5)) * vh,
                        z: v.z * iw,
                        iw,
                        r: v.r * iw,
                        g: v.g * iw,
                        b: v.b * iw,
                    }
                })
                .collect();
            for k in 1..sv.len() - 1 {
                let t = [sv[0], sv[k], sv[k + 1]];
                // Back-face culling (counter-clockwise front faces in screen space with y down).
                let area = (t[1].x - t[0].x) * (t[2].y - t[0].y) - (t[2].x - t[0].x) * (t[1].y - t[0].y);
                if area >= 0.0 {
                    continue;
                }
                let min_x = t.iter().map(|v| v.x).fold(f32::INFINITY, f32::min).floor().max(viewport.x as f32) as i32;
                let max_x = t.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max).ceil().min((viewport.right() - 1) as f32) as i32;
                let min_y = t.iter().map(|v| v.y).fold(f32::INFINITY, f32::min).floor().max(viewport.y as f32) as i32;
                let max_y = t.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max).ceil().min((viewport.bottom() - 1) as f32) as i32;
                if min_x > max_x || min_y > max_y {
                    continue;
                }
                tris.push(Tri { v: t, min_x, max_x, min_y, max_y });
            }
        }
    }
    rasterize_parallel(canvas, viewport, &tris);
}

fn rasterize_parallel(canvas: &mut Canvas, viewport: Rect, tris: &[Tri]) {
    let width = canvas.width;
    let vp = viewport.intersect(&canvas.full_rect());
    if vp.w <= 0 || vp.h <= 0 || tris.is_empty() {
        return;
    }
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 16);
    let band_h = ((vp.h as usize + threads - 1) / threads).max(16);
    // Split the colour and depth buffers into row bands starting at vp.y.
    let start = vp.y as usize * width;
    let end = vp.bottom() as usize * width;
    let color = &mut canvas.color[start..end];
    let depth = &mut canvas.depth[start..end];
    let color_bands = color.chunks_mut(band_h * width);
    let depth_bands = depth.chunks_mut(band_h * width);
    std::thread::scope(|s| {
        for (i, (cb, db)) in color_bands.zip(depth_bands).enumerate() {
            let y0 = vp.y + (i * band_h) as i32;
            let rows = cb.len() / width;
            let band = Rect::new(vp.x, y0, vp.w, rows as i32);
            if threads == 1 {
                raster_band(cb, db, width, band, tris);
            } else {
                s.spawn(move || raster_band(cb, db, width, band, tris));
            }
        }
    });
}

fn raster_band(color: &mut [Rgb], depth: &mut [f32], width: usize, band: Rect, tris: &[Tri]) {
    let by0 = band.y;
    let by1 = band.bottom() - 1;
    for t in tris {
        if t.max_y < by0 || t.min_y > by1 {
            continue;
        }
        let [a, b, c] = t.v;
        let area = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
        if area.abs() < 1e-8 {
            continue;
        }
        let inv_area = -1.0 / area; // front faces have negative screen area
        let y_start = t.min_y.max(by0);
        let y_end = t.max_y.min(by1);
        // Edge function coefficients: w0 is weight of a, etc.
        let e0 = (c.y - b.y, b.x - c.x); // edge bc
        let e1 = (a.y - c.y, c.x - a.x); // edge ca
        let e2 = (b.y - a.y, a.x - b.x); // edge ab
        for y in y_start..=y_end {
            let py = y as f32 + 0.5;
            let px0 = t.min_x as f32 + 0.5;
            let mut w0 = (px0 - b.x) * e0.0 + (py - b.y) * e0.1;
            let mut w1 = (px0 - c.x) * e1.0 + (py - c.y) * e1.1;
            let mut w2 = (px0 - a.x) * e2.0 + (py - a.y) * e2.1;
            let row = (y - band.y) as usize * width;
            for x in t.min_x..=t.max_x {
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    let l0 = w0 * inv_area;
                    let l1 = w1 * inv_area;
                    let l2 = w2 * inv_area;
                    let z = a.z * l0 + b.z * l1 + c.z * l2;
                    let i = row + x as usize;
                    if z < depth[i] && z >= -1.0 {
                        depth[i] = z;
                        let iw = a.iw * l0 + b.iw * l1 + c.iw * l2;
                        let w = 1.0 / iw;
                        let r = (a.r * l0 + b.r * l1 + c.r * l2) * w;
                        let g = (a.g * l0 + b.g * l1 + c.g * l2) * w;
                        let bl = (a.b * l0 + b.b * l1 + c.b * l2) * w;
                        color[i] = pack(r, g, bl);
                    }
                }
                w0 += e0.0;
                w1 += e1.0;
                w2 += e2.0;
            }
        }
    }
}

#[inline]
pub fn pack(r: f32, g: f32, b: f32) -> Rgb {
    // Simple filmic-ish tone curve keeps bright highlights from clipping harshly.
    let tc = |v: f32| {
        let v = v.max(0.0);
        let v = v / (1.0 + 0.15 * v);
        ((v * 1.15).min(1.0) * 255.0 + 0.5) as u32
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
    // clip against near plane
    let da = ca.z + ca.w - NEAR_EPS;
    let db = cb.z + cb.w - NEAR_EPS;
    if da < 0.0 && db < 0.0 {
        return;
    }
    if da < 0.0 {
        let t = da / (da - db);
        ca = grodot_core::math::Vec4 {
            x: ca.x + (cb.x - ca.x) * t,
            y: ca.y + (cb.y - ca.y) * t,
            z: ca.z + (cb.z - ca.z) * t,
            w: ca.w + (cb.w - ca.w) * t,
        };
    } else if db < 0.0 {
        let t = db / (db - da);
        cb = grodot_core::math::Vec4 {
            x: cb.x + (ca.x - cb.x) * t,
            y: cb.y + (ca.y - cb.y) * t,
            z: cb.z + (ca.z - cb.z) * t,
            w: cb.w + (ca.w - cb.w) * t,
        };
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

/// Project a world point to viewport pixel coordinates (None if behind the camera).
pub fn project(view: &View, viewport: Rect, p: Vec3) -> Option<(f32, f32)> {
    let c = view.view_proj().transform4(p);
    if c.w <= 1e-4 {
        return None;
    }
    let iw = 1.0 / c.w;
    Some((
        viewport.x as f32 + (c.x * iw * 0.5 + 0.5) * viewport.w as f32,
        viewport.y as f32 + (1.0 - (c.y * iw * 0.5 + 0.5)) * viewport.h as f32,
    ))
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
