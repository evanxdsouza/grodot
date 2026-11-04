//! Triangle meshes: built-in primitives and a Wavefront OBJ loader.
//!
//! Texture coordinates follow the usual image convention: (0, 0) is the
//! top-left of the texture and V grows downwards.

use crate::math::{vec3, Vec2, Vec3};
use std::f32::consts::PI;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// One UV per vertex (same length as `positions`).
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
    /// Skinning: up to four joint indices and weights per vertex (empty when not skinned).
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

impl Mesh {
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in &self.positions {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if self.positions.is_empty() {
            (Vec3::ZERO, Vec3::ZERO)
        } else {
            (lo, hi)
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Make sure every vertex has a UV (fills with zeros if missing).
    pub fn ensure_uvs(&mut self) {
        if self.uvs.len() != self.positions.len() {
            self.uvs.resize(self.positions.len(), Vec2::ZERO);
        }
    }

    fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        self.uvs.push(uv);
        self.positions.len() as u32 - 1
    }

    /// Quad a-b-c-d (counter-clockwise seen from the front) with UVs covering 0..1.
    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, n: Vec3) {
        let ia = self.vertex(a, n, Vec2::new(0.0, 1.0));
        let ib = self.vertex(b, n, Vec2::new(1.0, 1.0));
        let ic = self.vertex(c, n, Vec2::new(1.0, 0.0));
        let id = self.vertex(d, n, Vec2::new(0.0, 0.0));
        self.indices.extend_from_slice(&[ia, ib, ic, ia, ic, id]);
    }

    /// Unit cube centred on the origin (size 1). Each face shows the whole texture upright.
    pub fn cube() -> Mesh {
        let mut m = Mesh::default();
        let h = 0.5;
        let faces: [(Vec3, Vec3, Vec3); 6] = [
            (Vec3::X, Vec3::Y, -Vec3::Z),
            (-Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::Y, -Vec3::Z, Vec3::X),
            (-Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::Z, Vec3::Y, Vec3::X),
            (-Vec3::Z, Vec3::Y, -Vec3::X),
        ];
        for (n, up, right) in faces {
            let c = n * h;
            let u = up * h;
            let r = right * h;
            m.quad(c - r - u, c + r - u, c + r + u, c - r + u, n);
        }
        m
    }

    /// Flat 1x1 plane on XZ facing +Y. U follows +X, V follows +Z.
    pub fn plane() -> Mesh {
        let mut m = Mesh::default();
        let h = 0.5;
        m.quad(vec3(-h, 0.0, h), vec3(h, 0.0, h), vec3(h, 0.0, -h), vec3(-h, 0.0, -h), Vec3::Y);
        m
    }

    /// Vertical 1x1 quad on XY facing +Z (for sprites and foliage cards).
    pub fn quad_xy() -> Mesh {
        let mut m = Mesh::default();
        let h = 0.5;
        m.quad(vec3(-h, -h, 0.0), vec3(h, -h, 0.0), vec3(h, h, 0.0), vec3(-h, h, 0.0), Vec3::Z);
        m
    }

    /// UV sphere of diameter 1. U wraps around the equator, V runs from the top pole down.
    pub fn sphere(segments: u32, rings: u32) -> Mesh {
        let mut m = Mesh::default();
        for r in 0..=rings {
            let v = r as f32 / rings as f32;
            let phi = v * PI;
            for s in 0..=segments {
                let u = s as f32 / segments as f32;
                let th = u * 2.0 * PI;
                let n = vec3(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
                m.vertex(n * 0.5, n, Vec2::new(1.0 - u, v));
            }
        }
        let stride = segments + 1;
        for r in 0..rings {
            for s in 0..segments {
                let a = r * stride + s;
                let b = a + stride;
                m.indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
            }
        }
        m
    }

    /// Cylinder of diameter 1 and height 1 along Y. The side wraps the texture once
    /// around; the caps map it flat.
    pub fn cylinder(segments: u32) -> Mesh {
        let mut m = Mesh::default();
        let h = 0.5;
        let cap_uv = |p: Vec3| Vec2::new(p.x + 0.5, p.z + 0.5);
        for i in 0..segments {
            let t0 = i as f32 / segments as f32;
            let t1 = (i + 1) as f32 / segments as f32;
            let (a0, a1) = (t0 * 2.0 * PI, t1 * 2.0 * PI);
            let p0 = vec3(a0.cos() * 0.5, 0.0, a0.sin() * 0.5);
            let p1 = vec3(a1.cos() * 0.5, 0.0, a1.sin() * 0.5);
            let n0 = vec3(a0.cos(), 0.0, a0.sin());
            let n1 = vec3(a1.cos(), 0.0, a1.sin());
            let up = vec3(0.0, h, 0.0);
            let i0 = m.vertex(p0 - up, n0, Vec2::new(1.0 - t0, 1.0));
            let i1 = m.vertex(p1 - up, n1, Vec2::new(1.0 - t1, 1.0));
            let i2 = m.vertex(p1 + up, n1, Vec2::new(1.0 - t1, 0.0));
            let i3 = m.vertex(p0 + up, n0, Vec2::new(1.0 - t0, 0.0));
            m.indices.extend_from_slice(&[i0, i2, i1, i0, i3, i2]);
            let c = m.vertex(up, Vec3::Y, Vec2::new(0.5, 0.5));
            let c1 = m.vertex(p1 + up, Vec3::Y, cap_uv(p1));
            let c0 = m.vertex(p0 + up, Vec3::Y, cap_uv(p0));
            m.indices.extend_from_slice(&[c, c1, c0]);
            let b = m.vertex(-up, -Vec3::Y, Vec2::new(0.5, 0.5));
            let b0 = m.vertex(p0 - up, -Vec3::Y, cap_uv(p0));
            let b1 = m.vertex(p1 - up, -Vec3::Y, cap_uv(p1));
            m.indices.extend_from_slice(&[b, b0, b1]);
        }
        m
    }

    /// Capsule of diameter 1 and total height 2 along Y (sphere UVs stretched over the body).
    pub fn capsule(segments: u32, rings: u32) -> Mesh {
        let mut m = Mesh::sphere(segments, rings * 2);
        for (p, n) in m.positions.iter_mut().zip(m.normals.iter()) {
            p.y += if n.y >= 0.0 { 0.5 } else { -0.5 };
        }
        // V follows height so a texture wraps the whole body evenly.
        for (uv, p) in m.uvs.iter_mut().zip(m.positions.iter()) {
            uv.y = (1.0 - p.y) / 2.0;
        }
        m
    }

    pub fn primitive(name: &str) -> Option<Mesh> {
        Some(match name {
            "cube" => Mesh::cube(),
            "sphere" => Mesh::sphere(24, 16),
            "plane" => Mesh::plane(),
            "quad" => Mesh::quad_xy(),
            "cylinder" => Mesh::cylinder(24),
            "capsule" => Mesh::capsule(20, 8),
            _ => return None,
        })
    }

    /// Parse a Wavefront OBJ (positions, optional normals and texture coordinates, polygon faces).
    pub fn from_obj(src: &str) -> Result<Mesh, String> {
        let mut pos = Vec::new();
        let mut nrm = Vec::new();
        let mut tex = Vec::new();
        let mut m = Mesh::default();
        let mut need_normals = false;
        for (ln, line) in src.lines().enumerate() {
            let mut it = line.split_whitespace();
            let parse_n = |it: &mut std::str::SplitWhitespace, n: usize, min: usize| -> Result<[f32; 3], String> {
                let mut v = [0f32; 3];
                for (i, x) in v.iter_mut().enumerate().take(n) {
                    match it.next() {
                        Some(s) => *x = s.parse().map_err(|_| format!("line {}: bad number '{}'", ln + 1, s))?,
                        None if i >= min => break,
                        None => return Err(format!("line {}: expected {} numbers", ln + 1, min)),
                    }
                }
                Ok(v)
            };
            match it.next() {
                Some("v") => pos.push(Vec3::from_array(parse_n(&mut it, 3, 3)?)),
                Some("vn") => nrm.push(Vec3::from_array(parse_n(&mut it, 3, 3)?).normalized()),
                Some("vt") => {
                    let v = parse_n(&mut it, 2, 1)?;
                    // OBJ puts V=0 at the bottom of the image.
                    tex.push(Vec2::new(v[0], 1.0 - v[1]));
                }
                Some("f") => {
                    let mut verts = Vec::new();
                    for tok in it {
                        let mut parts = tok.split('/');
                        let resolve = |s: Option<&str>, len: usize| -> Result<Option<usize>, String> {
                            match s {
                                None | Some("") => Ok(None),
                                Some(t) => {
                                    let i: i64 =
                                        t.parse().map_err(|_| format!("line {}: bad index '{}'", ln + 1, t))?;
                                    let idx = if i < 0 { len as i64 + i } else { i - 1 };
                                    if idx < 0 || idx as usize >= len {
                                        return Err(format!("line {}: index {} out of range", ln + 1, i));
                                    }
                                    Ok(Some(idx as usize))
                                }
                            }
                        };
                        let vi = resolve(parts.next(), pos.len())?
                            .ok_or_else(|| format!("line {}: face needs a vertex index", ln + 1))?;
                        let ti = resolve(parts.next(), tex.len())?;
                        let ni = resolve(parts.next(), nrm.len())?;
                        verts.push((vi, ti, ni));
                    }
                    if verts.len() < 3 {
                        return Err(format!("line {}: face needs at least 3 vertices", ln + 1));
                    }
                    let base = m.positions.len() as u32;
                    for (vi, ti, ni) in &verts {
                        m.positions.push(pos[*vi]);
                        m.uvs.push(ti.map(|t| tex[t]).unwrap_or(Vec2::ZERO));
                        match ni {
                            Some(n) => m.normals.push(nrm[*n]),
                            None => {
                                need_normals = true;
                                m.normals.push(Vec3::ZERO);
                            }
                        }
                    }
                    for k in 1..verts.len() as u32 - 1 {
                        m.indices.extend_from_slice(&[base, base + k, base + k + 1]);
                    }
                }
                _ => {}
            }
        }
        if m.indices.is_empty() {
            return Err("OBJ file contains no faces".into());
        }
        if need_normals {
            for tri in m.indices.chunks(3) {
                let (a, b, c) =
                    (m.positions[tri[0] as usize], m.positions[tri[1] as usize], m.positions[tri[2] as usize]);
                let n = (b - a).cross(c - a).normalized();
                for &i in tri {
                    if m.normals[i as usize].length_sq() < 0.5 {
                        m.normals[i as usize] = n;
                    }
                }
            }
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_have_consistent_buffers() {
        for name in crate::scene::PRIMITIVE_MESHES.iter().chain(["quad"].iter()) {
            let m = Mesh::primitive(name).unwrap();
            assert_eq!(m.positions.len(), m.normals.len(), "{}", name);
            assert_eq!(m.positions.len(), m.uvs.len(), "{} needs a UV per vertex", name);
            assert_eq!(m.indices.len() % 3, 0);
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
            assert!(
                m.uvs.iter().all(|uv| (-1e-5..=1.0 + 1e-5).contains(&uv.x) && (-1e-5..=1.0 + 1e-5).contains(&uv.y)),
                "{}",
                name
            );
        }
    }

    #[test]
    fn cube_winding_faces_outward() {
        let m = Mesh::cube();
        for tri in m.indices.chunks(3) {
            let (a, b, c) = (m.positions[tri[0] as usize], m.positions[tri[1] as usize], m.positions[tri[2] as usize]);
            let n = (b - a).cross(c - a);
            assert!(n.dot(m.normals[tri[0] as usize]) > 0.0);
        }
    }

    #[test]
    fn cube_faces_show_the_whole_texture_upright() {
        let m = Mesh::cube();
        // +Z face is the 5th quad (vertices 16..20): top vertices have V = 0.
        for i in 16..20 {
            let expect_v = if m.positions[i].y > 0.0 { 0.0 } else { 1.0 };
            assert_eq!(m.uvs[i].y, expect_v);
        }
    }

    #[test]
    fn obj_loader_with_texture_coordinates() {
        let src = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nf 1/1 2/2 3/3 4/4\n";
        let m = Mesh::from_obj(src).unwrap();
        assert_eq!(m.indices.len(), 6);
        assert!((m.normals[0] - Vec3::Z).length() < 1e-5);
        assert_eq!(m.uvs[0], Vec2::new(0.0, 1.0), "OBJ V is flipped to image space");
        assert_eq!(m.uvs[2], Vec2::new(1.0, 0.0));
        let no_vt = Mesh::from_obj("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1//1 2//1 3//1\nvn 0 0 1\n");
        assert!(no_vt.is_err(), "normal index 1 is referenced before it exists");
        assert!(Mesh::from_obj("f 1 2 3").is_err());
    }
}
