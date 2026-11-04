//! Animation math for glTF clips: keyframe sampling (LINEAR, STEP,
//! CUBICSPLINE), poses, crossfade blending, joint matrices and CPU skinning.
//!
//! Everything here is pure and deterministic so it can be unit-tested and
//! shared by the runtime (which advances clips) and the renderer (which skins).

use crate::gltf::{AnimPath, Animation, Channel, Gltf, Interpolation};
use crate::math::{Mat4, Quat, Vec3};
use crate::mesh::Mesh;

/// A node's local transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trs {
    pub t: Vec3,
    pub r: Quat,
    pub s: Vec3,
}

impl Default for Trs {
    fn default() -> Self {
        Trs { t: Vec3::ZERO, r: Quat::IDENTITY, s: Vec3::ONE }
    }
}

impl Trs {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_trs(self.t, self.r, self.s)
    }

    /// Decompose a matrix without shear (used for nodes stored as `matrix`).
    pub fn from_matrix(m: &Mat4) -> Trs {
        let col = |i: usize| Vec3 { x: m.c[i][0], y: m.c[i][1], z: m.c[i][2] };
        let (cx, cy, cz) = (col(0), col(1), col(2));
        let s = Vec3 { x: cx.length(), y: cy.length(), z: cz.length() };
        let safe = |v: Vec3, l: f32| if l > 1e-9 { v / l } else { v };
        let r = Quat::from_basis(safe(cx, s.x), safe(cy, s.y), safe(cz, s.z));
        Trs { t: m.get_translation(), r, s }
    }

    /// Blend towards `o` by `w` (0 = self, 1 = o).
    pub fn lerp(&self, o: &Trs, w: f32) -> Trs {
        Trs { t: self.t.lerp(o.t, w), r: self.r.slerp(o.r, w), s: self.s.lerp(o.s, w) }
    }
}

/// The rest (bind-time) pose of every node.
pub fn rest_pose(g: &Gltf) -> Vec<Trs> {
    g.nodes
        .iter()
        .map(|n| match n.matrix {
            Some(m) => Trs::from_matrix(&m),
            None => Trs { t: n.translation, r: n.rotation, s: n.scale },
        })
        .collect()
}

/// Index of the keyframe interval containing `t` and the fraction within it.
fn find_key(times: &[f32], t: f32) -> (usize, f32) {
    let n = times.len();
    if n == 0 {
        return (0, 0.0);
    }
    if t <= times[0] {
        return (0, 0.0);
    }
    if t >= times[n - 1] {
        return (n - 1, 0.0);
    }
    // Binary search for the last key with time <= t.
    let mut lo = 0;
    let mut hi = n - 1;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if times[mid] <= t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let span = times[lo + 1] - times[lo];
    (lo, if span > 0.0 { (t - times[lo]) / span } else { 0.0 })
}

/// Sample a channel at time `t` (clamped to the keyframe range). Returns
/// [x, y, z, w]; w is meaningful only for rotations.
pub fn sample_channel(ch: &Channel, t: f32) -> [f32; 4] {
    let comps = if ch.path == AnimPath::Rotation { 4 } else { 3 };
    let n = ch.times.len();
    if n == 0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let cubic = ch.interpolation == Interpolation::CubicSpline;
    // Value of key k (skipping the tangents for cubic splines).
    let value = |k: usize, part: usize| -> [f32; 4] {
        let base = if cubic { (k * 3 + part) * comps } else { k * comps };
        let mut v = [0.0, 0.0, 0.0, 1.0];
        v[..comps].copy_from_slice(&ch.values[base..base + comps]);
        v
    };
    let (k, f) = find_key(&ch.times, t);
    if k + 1 >= n || f == 0.0 {
        return value(k, 1);
    }
    let out = match ch.interpolation {
        Interpolation::Step => value(k, 1),
        Interpolation::Linear => {
            let (a, b) = (value(k, 1), value(k + 1, 1));
            if ch.path == AnimPath::Rotation {
                let qa = Quat { x: a[0], y: a[1], z: a[2], w: a[3] };
                let qb = Quat { x: b[0], y: b[1], z: b[2], w: b[3] };
                let q = qa.slerp(qb, f);
                [q.x, q.y, q.z, q.w]
            } else {
                [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, 1.0]
            }
        }
        Interpolation::CubicSpline => {
            let dt = ch.times[k + 1] - ch.times[k];
            let p0 = value(k, 1);
            let m0 = value(k, 2); // out-tangent of key k
            let p1 = value(k + 1, 1);
            let m1 = value(k + 1, 0); // in-tangent of key k+1
            let (t2, t3) = (f * f, f * f * f);
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + f;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            let mut v = [0.0; 4];
            for i in 0..4 {
                v[i] = h00 * p0[i] + h10 * dt * m0[i] + h01 * p1[i] + h11 * dt * m1[i];
            }
            if comps == 3 {
                v[3] = 1.0;
            }
            v
        }
    };
    if ch.path == AnimPath::Rotation {
        let q = Quat { x: out[0], y: out[1], z: out[2], w: out[3] }.normalized();
        [q.x, q.y, q.z, q.w]
    } else {
        out
    }
}

/// Write an animation's values at `time` into `pose` (untouched nodes keep their value).
pub fn apply(anim: &Animation, time: f32, pose: &mut [Trs]) {
    for ch in &anim.channels {
        let Some(node) = pose.get_mut(ch.node) else { continue };
        let v = sample_channel(ch, time);
        match ch.path {
            AnimPath::Translation => node.t = Vec3 { x: v[0], y: v[1], z: v[2] },
            AnimPath::Rotation => node.r = Quat { x: v[0], y: v[1], z: v[2], w: v[3] },
            AnimPath::Scale => node.s = Vec3 { x: v[0], y: v[1], z: v[2] },
        }
    }
}

/// Blend two poses node by node.
pub fn blend(a: &[Trs], b: &[Trs], w: f32) -> Vec<Trs> {
    a.iter().zip(b.iter()).map(|(x, y)| x.lerp(y, w.clamp(0.0, 1.0))).collect()
}

pub fn local_matrices(pose: &[Trs]) -> Vec<Mat4> {
    pose.iter().map(|p| p.matrix()).collect()
}

/// Skinning matrices for a skin: joint world transform times inverse bind matrix.
/// The result maps bind-pose vertices to their animated position in model space.
pub fn joint_matrices(g: &Gltf, skin: usize, world: &[Mat4]) -> Vec<Mat4> {
    let Some(s) = g.skins.get(skin) else { return Vec::new() };
    s.joints
        .iter()
        .zip(s.inverse_bind.iter())
        .map(|(&j, ibm)| world.get(j).copied().unwrap_or(Mat4::IDENTITY) * *ibm)
        .collect()
}

/// Skin a mesh on the CPU (linear blend skinning, up to four joints per vertex).
/// Vertices whose joint indices are out of range keep their bind position.
pub fn skin_mesh(mesh: &Mesh, joints: &[Mat4]) -> Mesh {
    let mut out = mesh.clone();
    if mesh.joints.is_empty() || joints.is_empty() {
        return out;
    }
    for i in 0..mesh.positions.len() {
        let (js, ws) = (mesh.joints[i], mesh.weights[i]);
        let mut p = Vec3::ZERO;
        let mut n = Vec3::ZERO;
        let mut total = 0.0;
        for k in 0..4 {
            let w = ws[k];
            if w <= 0.0 {
                continue;
            }
            let Some(m) = joints.get(js[k] as usize) else { continue };
            p += m.transform_point(mesh.positions[i]) * w;
            n += m.transform_vector(mesh.normals[i]) * w;
            total += w;
        }
        if total > 1e-6 {
            out.positions[i] = p / total;
            out.normals[i] = n.normalized();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::vec3;

    fn ch(path: AnimPath, interp: Interpolation, times: &[f32], values: &[f32]) -> Channel {
        Channel { node: 0, path, interpolation: interp, times: times.to_vec(), values: values.to_vec() }
    }

    #[test]
    fn linear_translation_and_clamping() {
        let c = ch(AnimPath::Translation, Interpolation::Linear, &[0.0, 2.0], &[0.0, 0.0, 0.0, 10.0, 4.0, -2.0]);
        assert_eq!(sample_channel(&c, 1.0), [5.0, 2.0, -1.0, 1.0]);
        assert_eq!(sample_channel(&c, -1.0)[0], 0.0, "before the first key holds the first value");
        assert_eq!(sample_channel(&c, 9.0)[0], 10.0, "after the last key holds the last value");
        assert_eq!(sample_channel(&c, 0.5), [2.5, 1.0, -0.5, 1.0]);
    }

    #[test]
    fn step_holds_until_the_next_key() {
        let c =
            ch(AnimPath::Scale, Interpolation::Step, &[0.0, 1.0, 2.0], &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0]);
        assert_eq!(sample_channel(&c, 0.99)[0], 1.0);
        assert_eq!(sample_channel(&c, 1.0)[0], 2.0);
        assert_eq!(sample_channel(&c, 1.5)[0], 2.0);
        assert_eq!(sample_channel(&c, 2.5)[0], 3.0);
    }

    #[test]
    fn rotation_uses_slerp() {
        let q0 = Quat::IDENTITY;
        let q1 = Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
        let c = ch(
            AnimPath::Rotation,
            Interpolation::Linear,
            &[0.0, 1.0],
            &[q0.x, q0.y, q0.z, q0.w, q1.x, q1.y, q1.z, q1.w],
        );
        let v = sample_channel(&c, 0.5);
        let q = Quat { x: v[0], y: v[1], z: v[2], w: v[3] };
        // Halfway is a 45 degree turn: forward (-Z) rotates to (-sin45, 0, -cos45).
        let f = q.rotate(Vec3::FORWARD);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((f - vec3(-h, 0.0, -h)).length() < 1e-5, "{:?}", f);
    }

    #[test]
    fn cubic_spline_hermite() {
        // Keys at 0 and 2 with values 0 and 1. Zero tangents give smoothstep.
        let zero = ch(
            AnimPath::Translation,
            Interpolation::CubicSpline,
            &[0.0, 2.0],
            &[
                0.0, 0.0, 0.0, /* in */ 0.0, 0.0, 0.0, /* value */ 0.0, 0.0, 0.0, /* out */
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ],
        );
        assert!((sample_channel(&zero, 1.0)[0] - 0.5).abs() < 1e-6);
        assert!((sample_channel(&zero, 0.5)[0] - 0.15625).abs() < 1e-6, "3t^2 - 2t^3 at t = 0.25");
        // Out-tangent of 1 unit/s on the first key, with dt = 2: h10 * dt * m0 adds 2*(0.25^3 - 2*0.25^2 + 0.25).
        let slope = ch(
            AnimPath::Translation,
            Interpolation::CubicSpline,
            &[0.0, 2.0],
            &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        );
        let expect = 0.15625 + 2.0 * (0.015625 - 0.125 + 0.25);
        assert!((sample_channel(&slope, 0.5)[0] - expect).abs() < 1e-6);
        assert_eq!(sample_channel(&slope, 2.0)[0], 1.0, "the last key value, not its tangent");
    }

    #[test]
    fn crossfade_blends_poses() {
        let a = vec![Trs { t: vec3(0.0, 0.0, 0.0), ..Default::default() }];
        let b = vec![Trs { t: vec3(4.0, 0.0, 0.0), s: vec3(3.0, 3.0, 3.0), ..Default::default() }];
        let m = blend(&a, &b, 0.25);
        assert_eq!(m[0].t.x, 1.0);
        assert_eq!(m[0].s.x, 1.5);
    }

    #[test]
    fn skinning_follows_the_joint() {
        let mesh = Mesh {
            positions: vec![vec3(0.0, 1.0, 0.0), vec3(0.0, 1.0, 0.0)],
            normals: vec![Vec3::Y, Vec3::Y],
            uvs: vec![Default::default(); 2],
            joints: vec![[0, 0, 0, 0], [0, 1, 0, 0]],
            weights: vec![[1.0, 0.0, 0.0, 0.0], [0.5, 0.5, 0.0, 0.0]],
            ..Default::default()
        };
        let moved = Mat4::translation(vec3(2.0, 0.0, 0.0));
        let skinned = skin_mesh(&mesh, &[Mat4::IDENTITY, moved]);
        assert_eq!(skinned.positions[0], vec3(0.0, 1.0, 0.0), "fully bound to the still joint");
        assert!((skinned.positions[1] - vec3(1.0, 1.0, 0.0)).length() < 1e-6, "half way between the two joints");
    }

    #[test]
    fn matrix_decomposition_roundtrip() {
        let t =
            Trs { t: vec3(1.0, 2.0, 3.0), r: Quat::from_euler_deg(vec3(10.0, 40.0, -20.0)), s: vec3(2.0, 1.0, 0.5) };
        let back = Trs::from_matrix(&t.matrix());
        let p = vec3(0.3, -0.7, 1.1);
        assert!((back.matrix().transform_point(p) - t.matrix().transform_point(p)).length() < 1e-4);
    }
}
