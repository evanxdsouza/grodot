//! Triangle meshes: built-in primitives and a Wavefront OBJ loader.

use crate::math::{vec3, Vec3};
use std::f32::consts::PI;

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub indices: Vec<u32>,
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

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, n: Vec3) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&[a, b, c, d]);
        self.normals.extend_from_slice(&[n, n, n, n]);
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Unit cube centred on the origin (size 1).
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

    /// Flat 1x1 plane on XZ facing +Y.
    pub fn plane() -> Mesh {
        let mut m = Mesh::default();
        let h = 0.5;
        m.quad(vec3(-h, 0.0, h), vec3(h, 0.0, h), vec3(h, 0.0, -h), vec3(-h, 0.0, -h), Vec3::Y);
        m
    }

    /// UV sphere of diameter 1.
    pub fn sphere(segments: u32, rings: u32) -> Mesh {
        let mut m = Mesh::default();
        for r in 0..=rings {
            let v = r as f32 / rings as f32;
            let phi = v * PI;
            for s in 0..=segments {
                let u = s as f32 / segments as f32;
                let th = u * 2.0 * PI;
                let n = vec3(phi.sin() * th.cos(), phi.cos(), phi.sin() * th.sin());
                m.positions.push(n * 0.5);
                m.normals.push(n);
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

    /// Cylinder of diameter 1 and height 1 along Y.
    pub fn cylinder(segments: u32) -> Mesh {
        let mut m = Mesh::default();
        let h = 0.5;
        for i in 0..segments {
            let a0 = i as f32 / segments as f32 * 2.0 * PI;
            let a1 = (i + 1) as f32 / segments as f32 * 2.0 * PI;
            let p0 = vec3(a0.cos() * 0.5, 0.0, a0.sin() * 0.5);
            let p1 = vec3(a1.cos() * 0.5, 0.0, a1.sin() * 0.5);
            let base = m.positions.len() as u32;
            let n0 = vec3(a0.cos(), 0.0, a0.sin());
            let n1 = vec3(a1.cos(), 0.0, a1.sin());
            m.positions.extend_from_slice(&[
                p0 + vec3(0.0, -h, 0.0),
                p1 + vec3(0.0, -h, 0.0),
                p1 + vec3(0.0, h, 0.0),
                p0 + vec3(0.0, h, 0.0),
            ]);
            m.normals.extend_from_slice(&[n0, n1, n1, n0]);
            m.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            // caps
            let t = m.positions.len() as u32;
            m.positions.extend_from_slice(&[vec3(0.0, h, 0.0), p1 + vec3(0.0, h, 0.0), p0 + vec3(0.0, h, 0.0)]);
            m.normals.extend_from_slice(&[Vec3::Y; 3]);
            m.indices.extend_from_slice(&[t, t + 1, t + 2]);
            let b = m.positions.len() as u32;
            m.positions.extend_from_slice(&[vec3(0.0, -h, 0.0), p0 + vec3(0.0, -h, 0.0), p1 + vec3(0.0, -h, 0.0)]);
            m.normals.extend_from_slice(&[-Vec3::Y; 3]);
            m.indices.extend_from_slice(&[b, b + 1, b + 2]);
        }
        m
    }

    /// Capsule of diameter 1 and total height 2 along Y.
    pub fn capsule(segments: u32, rings: u32) -> Mesh {
        let mut m = Mesh::sphere(segments, rings * 2);
        for (p, n) in m.positions.iter_mut().zip(m.normals.iter()) {
            p.y += if n.y >= 0.0 { 0.5 } else { -0.5 };
        }
        m
    }

    pub fn primitive(name: &str) -> Option<Mesh> {
        Some(match name {
            "cube" => Mesh::cube(),
            "sphere" => Mesh::sphere(24, 16),
            "plane" => Mesh::plane(),
            "cylinder" => Mesh::cylinder(24),
            "capsule" => Mesh::capsule(20, 8),
            _ => return None,
        })
    }

    /// Parse a Wavefront OBJ (positions, optional normals, polygon faces).
    pub fn from_obj(src: &str) -> Result<Mesh, String> {
        let mut pos = Vec::new();
        let mut nrm = Vec::new();
        let mut m = Mesh::default();
        let mut need_normals = false;
        for (ln, line) in src.lines().enumerate() {
            let mut it = line.split_whitespace();
            let parse3 = |it: &mut std::str::SplitWhitespace| -> Result<Vec3, String> {
                let mut v = [0f32; 3];
                for x in v.iter_mut() {
                    *x = it
                        .next()
                        .and_then(|s| s.parse().ok())
                        .ok_or_else(|| format!("line {}: expected 3 numbers", ln + 1))?;
                }
                Ok(Vec3::from_array(v))
            };
            match it.next() {
                Some("v") => pos.push(parse3(&mut it)?),
                Some("vn") => nrm.push(parse3(&mut it)?.normalized()),
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
                        let _vt = parts.next();
                        let ni = resolve(parts.next(), nrm.len())?;
                        verts.push((vi, ni));
                    }
                    if verts.len() < 3 {
                        return Err(format!("line {}: face needs at least 3 vertices", ln + 1));
                    }
                    let base = m.positions.len() as u32;
                    for (vi, ni) in &verts {
                        m.positions.push(pos[*vi]);
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
                    if m.normals[i as usize] == Vec3::ZERO || m.normals[i as usize].length_sq() < 0.5 {
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
        for name in crate::scene::PRIMITIVE_MESHES {
            let m = Mesh::primitive(name).unwrap();
            assert_eq!(m.positions.len(), m.normals.len());
            assert_eq!(m.indices.len() % 3, 0);
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
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
    fn obj_loader() {
        let src = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n";
        let m = Mesh::from_obj(src).unwrap();
        assert_eq!(m.indices.len(), 6);
        assert!((m.normals[0] - Vec3::Z).length() < 1e-5);
        assert!(Mesh::from_obj("f 1 2 3").is_err());
    }
}
