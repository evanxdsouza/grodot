//! Small 3D math library: vectors, quaternions and 4x4 matrices.
//!
//! Conventions (used everywhere in Grodot):
//! * Right-handed coordinates, **+Y is up**, **-Z is forward**, +X is right.
//! * 1 world unit = 1 metre.
//! * Rotations are stored as unit quaternions. Euler angles are in **degrees**
//!   and applied in Y (yaw), then X (pitch), then Z (roll) order.
//! * Matrices are column-major and multiply column vectors (`M * v`).

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[inline]
pub const fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = vec3(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = vec3(1.0, 1.0, 1.0);
    pub const X: Vec3 = vec3(1.0, 0.0, 0.0);
    pub const Y: Vec3 = vec3(0.0, 1.0, 0.0);
    pub const Z: Vec3 = vec3(0.0, 0.0, 1.0);
    pub const FORWARD: Vec3 = vec3(0.0, 0.0, -1.0);

    pub fn splat(v: f32) -> Vec3 {
        vec3(v, v, v)
    }
    pub fn dot(self, o: Vec3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: Vec3) -> Vec3 {
        vec3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn length_sq(self) -> f32 {
        self.dot(self)
    }
    pub fn normalized(self) -> Vec3 {
        let l = self.length();
        if l > 1e-9 {
            self / l
        } else {
            Vec3::ZERO
        }
    }
    pub fn mul_elem(self, o: Vec3) -> Vec3 {
        vec3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
    pub fn min(self, o: Vec3) -> Vec3 {
        vec3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: Vec3) -> Vec3 {
        vec3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    pub fn abs(self) -> Vec3 {
        vec3(self.x.abs(), self.y.abs(), self.z.abs())
    }
    pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
        self + (o - self) * t
    }
    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
    pub fn from_array(a: [f32; 3]) -> Vec3 {
        vec3(a[0], a[1], a[2])
    }
    pub fn get(self, i: usize) -> f32 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    pub fn set(&mut self, i: usize, v: f32) {
        match i {
            0 => self.x = v,
            1 => self.y = v,
            _ => self.z = v,
        }
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        vec3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, o: Vec3) {
        *self = *self + o;
    }
}
impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        vec3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl SubAssign for Vec3 {
    fn sub_assign(&mut self, o: Vec3) {
        *self = *self - o;
    }
}
impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: f32) -> Vec3 {
        vec3(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f32> for Vec3 {
    type Output = Vec3;
    fn div(self, s: f32) -> Vec3 {
        vec3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        vec3(-self.x, -self.y, -self.z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// A rotation quaternion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Default for Quat {
    fn default() -> Self {
        Quat::IDENTITY
    }
}

impl Quat {
    pub const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    pub fn from_axis_angle(axis: Vec3, radians: f32) -> Quat {
        let a = axis.normalized();
        let (s, c) = (radians * 0.5).sin_cos();
        Quat { x: a.x * s, y: a.y * s, z: a.z * s, w: c }
    }

    /// Euler angles in degrees, applied yaw (Y) then pitch (X) then roll (Z).
    pub fn from_euler_deg(e: Vec3) -> Quat {
        let qy = Quat::from_axis_angle(Vec3::Y, e.y.to_radians());
        let qx = Quat::from_axis_angle(Vec3::X, e.x.to_radians());
        let qz = Quat::from_axis_angle(Vec3::Z, e.z.to_radians());
        (qy * qx * qz).normalized()
    }

    /// Inverse of [`Quat::from_euler_deg`].
    pub fn to_euler_deg(self) -> Vec3 {
        let m = Mat4::from_quat(self);
        // R = Ry * Rx * Rz ; m[col][row]
        let r12 = m.c[2][1]; // row 1, col 2
        let pitch = (-r12).clamp(-1.0, 1.0).asin();
        let (yaw, roll);
        if r12.abs() < 0.9999 {
            yaw = m.c[2][0].atan2(m.c[2][2]);
            roll = m.c[0][1].atan2(m.c[1][1]);
        } else {
            yaw = (-m.c[0][2]).atan2(m.c[0][0]);
            roll = 0.0;
        }
        let clean = |v: f32| {
            let d = v.to_degrees();
            if d.abs() < 1e-4 {
                0.0
            } else {
                (d * 1000.0).round() / 1000.0
            }
        };
        vec3(clean(pitch), clean(yaw), clean(roll))
    }

    pub fn normalized(self) -> Quat {
        let l = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if l < 1e-9 {
            return Quat::IDENTITY;
        }
        Quat { x: self.x / l, y: self.y / l, z: self.z / l, w: self.w / l }
    }

    pub fn conjugate(self) -> Quat {
        Quat { x: -self.x, y: -self.y, z: -self.z, w: self.w }
    }

    pub fn rotate(self, v: Vec3) -> Vec3 {
        let q = vec3(self.x, self.y, self.z);
        let t = q.cross(v) * 2.0;
        v + t * self.w + q.cross(t)
    }

    /// Rotation that makes -Z point along `dir` with +Y as close to `up` as possible.
    pub fn look_rotation(dir: Vec3, up: Vec3) -> Quat {
        let f = dir.normalized();
        if f.length_sq() < 1e-9 {
            return Quat::IDENTITY;
        }
        let mut r = f.cross(up).normalized();
        if r.length_sq() < 1e-9 {
            r = f.cross(Vec3::Z).normalized();
        }
        let u = r.cross(f);
        // Columns: right, up, back(-f)
        Quat::from_basis(r, u, -f)
    }

    pub fn from_basis(x: Vec3, y: Vec3, z: Vec3) -> Quat {
        let (m00, m01, m02) = (x.x, y.x, z.x);
        let (m10, m11, m12) = (x.y, y.y, z.y);
        let (m20, m21, m22) = (x.z, y.z, z.z);
        let tr = m00 + m11 + m22;
        let q = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            Quat { w: 0.25 * s, x: (m21 - m12) / s, y: (m02 - m20) / s, z: (m10 - m01) / s }
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Quat { w: (m21 - m12) / s, x: 0.25 * s, y: (m01 + m10) / s, z: (m02 + m20) / s }
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Quat { w: (m02 - m20) / s, x: (m01 + m10) / s, y: 0.25 * s, z: (m12 + m21) / s }
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Quat { w: (m10 - m01) / s, x: (m02 + m20) / s, y: (m12 + m21) / s, z: 0.25 * s }
        };
        q.normalized()
    }

    pub fn slerp(self, o: Quat, t: f32) -> Quat {
        let mut d = self.x * o.x + self.y * o.y + self.z * o.z + self.w * o.w;
        let mut o = o;
        if d < 0.0 {
            d = -d;
            o = Quat { x: -o.x, y: -o.y, z: -o.z, w: -o.w };
        }
        if d > 0.9995 {
            return Quat {
                x: self.x + (o.x - self.x) * t,
                y: self.y + (o.y - self.y) * t,
                z: self.z + (o.z - self.z) * t,
                w: self.w + (o.w - self.w) * t,
            }
            .normalized();
        }
        let th = d.acos();
        let s = th.sin();
        let a = ((1.0 - t) * th).sin() / s;
        let b = (t * th).sin() / s;
        Quat {
            x: self.x * a + o.x * b,
            y: self.y * a + o.y * b,
            z: self.z * a + o.z * b,
            w: self.w * a + o.w * b,
        }
    }

    pub fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

impl Mul for Quat {
    type Output = Quat;
    fn mul(self, o: Quat) -> Quat {
        Quat {
            w: self.w * o.w - self.x * o.x - self.y * o.y - self.z * o.z,
            x: self.w * o.x + self.x * o.w + self.y * o.z - self.z * o.y,
            y: self.w * o.y - self.x * o.z + self.y * o.w + self.z * o.x,
            z: self.w * o.z + self.x * o.y - self.y * o.x + self.z * o.w,
        }
    }
}

/// Column-major 4x4 matrix: `c[col][row]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub c: [[f32; 4]; 4],
}

impl Default for Mat4 {
    fn default() -> Self {
        Mat4::IDENTITY
    }
}

impl Mat4 {
    pub const IDENTITY: Mat4 = Mat4 {
        c: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
    };

    pub fn from_quat(q: Quat) -> Mat4 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        Mat4 {
            c: [
                [1.0 - (yy + zz), xy + wz, xz - wy, 0.0],
                [xy - wz, 1.0 - (xx + zz), yz + wx, 0.0],
                [xz + wy, yz - wx, 1.0 - (xx + yy), 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    pub fn from_trs(t: Vec3, r: Quat, s: Vec3) -> Mat4 {
        let mut m = Mat4::from_quat(r);
        for i in 0..3 {
            m.c[0][i] *= s.x;
            m.c[1][i] *= s.y;
            m.c[2][i] *= s.z;
        }
        m.c[3] = [t.x, t.y, t.z, 1.0];
        m
    }

    pub fn translation(t: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.c[3] = [t.x, t.y, t.z, 1.0];
        m
    }

    /// OpenGL-style perspective projection (clip z in -1..1), fov in degrees.
    pub fn perspective(fov_y_deg: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f = 1.0 / (fov_y_deg.to_radians() * 0.5).tan();
        let nf = 1.0 / (near - far);
        Mat4 {
            c: [
                [f / aspect, 0.0, 0.0, 0.0],
                [0.0, f, 0.0, 0.0],
                [0.0, 0.0, (far + near) * nf, -1.0],
                [0.0, 0.0, 2.0 * far * near * nf, 0.0],
            ],
        }
    }

    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
        let f = (target - eye).normalized();
        let s = f.cross(up).normalized();
        let u = s.cross(f);
        Mat4 {
            c: [
                [s.x, u.x, -f.x, 0.0],
                [s.y, u.y, -f.y, 0.0],
                [s.z, u.z, -f.z, 0.0],
                [-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0],
            ],
        }
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let c = &self.c;
        vec3(
            c[0][0] * p.x + c[1][0] * p.y + c[2][0] * p.z + c[3][0],
            c[0][1] * p.x + c[1][1] * p.y + c[2][1] * p.z + c[3][1],
            c[0][2] * p.x + c[1][2] * p.y + c[2][2] * p.z + c[3][2],
        )
    }

    pub fn transform_vector(&self, p: Vec3) -> Vec3 {
        let c = &self.c;
        vec3(
            c[0][0] * p.x + c[1][0] * p.y + c[2][0] * p.z,
            c[0][1] * p.x + c[1][1] * p.y + c[2][1] * p.z,
            c[0][2] * p.x + c[1][2] * p.y + c[2][2] * p.z,
        )
    }

    pub fn transform4(&self, p: Vec3) -> Vec4 {
        let c = &self.c;
        Vec4 {
            x: c[0][0] * p.x + c[1][0] * p.y + c[2][0] * p.z + c[3][0],
            y: c[0][1] * p.x + c[1][1] * p.y + c[2][1] * p.z + c[3][1],
            z: c[0][2] * p.x + c[1][2] * p.y + c[2][2] * p.z + c[3][2],
            w: c[0][3] * p.x + c[1][3] * p.y + c[2][3] * p.z + c[3][3],
        }
    }

    pub fn get_translation(&self) -> Vec3 {
        vec3(self.c[3][0], self.c[3][1], self.c[3][2])
    }

    /// General 4x4 inverse. Returns identity if the matrix is singular.
    pub fn inverse(&self) -> Mat4 {
        let m: Vec<f32> = self.c.iter().flat_map(|c| c.iter().copied()).collect();
        let mut inv = [0f32; 16];
        inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
        inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
        inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
        inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
        inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
        inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
        inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
        inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
        inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
        inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
        inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
        inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
        inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
        inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
        inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
        inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
        let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
        if det.abs() < 1e-12 {
            return Mat4::IDENTITY;
        }
        let inv_det = 1.0 / det;
        let mut out = Mat4::IDENTITY;
        for col in 0..4 {
            for row in 0..4 {
                out.c[col][row] = inv[col * 4 + row] * inv_det;
            }
        }
        out
    }
}

impl Mul for Mat4 {
    type Output = Mat4;
    fn mul(self, o: Mat4) -> Mat4 {
        let mut r = [[0f32; 4]; 4];
        for (col, rc) in r.iter_mut().enumerate() {
            for (row, v) in rc.iter_mut().enumerate() {
                *v = (0..4).map(|k| self.c[k][row] * o.c[col][k]).sum();
            }
        }
        Mat4 { c: r }
    }
}

/// An RGBA colour with components in 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Color::WHITE
    }
}

impl Color {
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    pub fn from_hex(s: &str) -> Option<Color> {
        let h = s.trim_start_matches('#');
        let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|v| v as f32 / 255.0);
        match h.len() {
            6 => Some(Color { r: p(0)?, g: p(2)?, b: p(4)?, a: 1.0 }),
            8 => Some(Color { r: p(0)?, g: p(2)?, b: p(4)?, a: p(6)? }),
            _ => None,
        }
    }

    pub fn to_hex(&self) -> String {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        if self.a < 1.0 {
            format!("#{:02x}{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b), c(self.a))
        } else {
            format!("#{:02x}{:02x}{:02x}", c(self.r), c(self.g), c(self.b))
        }
    }

    /// Pack as 0x00RRGGBB.
    pub fn to_u32(&self) -> u32 {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        (c(self.r) << 16) | (c(self.g) << 8) | c(self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn quat_rotates_vectors() {
        let q = Quat::from_euler_deg(vec3(0.0, 90.0, 0.0));
        // Yaw 90 degrees left turns forward (-Z) into -X.
        assert!(close(q.rotate(Vec3::FORWARD), vec3(-1.0, 0.0, 0.0)));
    }

    #[test]
    fn euler_roundtrip() {
        for e in [vec3(10.0, 20.0, 30.0), vec3(-45.0, 170.0, 0.0), vec3(0.0, -90.0, 15.0)] {
            let back = Quat::from_euler_deg(e).to_euler_deg();
            let q1 = Quat::from_euler_deg(e);
            let q2 = Quat::from_euler_deg(back);
            assert!(close(q1.rotate(Vec3::X), q2.rotate(Vec3::X)), "{:?} -> {:?}", e, back);
            assert!(close(q1.rotate(Vec3::Y), q2.rotate(Vec3::Y)));
        }
    }

    #[test]
    fn trs_and_inverse() {
        let m = Mat4::from_trs(vec3(1.0, 2.0, 3.0), Quat::from_euler_deg(vec3(0.0, 45.0, 0.0)), vec3(2.0, 1.0, 1.0));
        let p = vec3(0.5, -1.0, 4.0);
        let q = m.transform_point(p);
        assert!(close(m.inverse().transform_point(q), p));
    }

    #[test]
    fn nested_transforms_compose() {
        let parent = Mat4::from_trs(vec3(10.0, 0.0, 0.0), Quat::IDENTITY, vec3(2.0, 2.0, 2.0));
        let child = Mat4::from_trs(vec3(1.0, 0.0, 0.0), Quat::IDENTITY, Vec3::ONE);
        let world = parent * child;
        assert!(close(world.transform_point(Vec3::ZERO), vec3(12.0, 0.0, 0.0)));
    }

    #[test]
    fn look_rotation_points_forward() {
        let q = Quat::look_rotation(vec3(1.0, 0.0, 0.0), Vec3::Y);
        assert!(close(q.rotate(Vec3::FORWARD), vec3(1.0, 0.0, 0.0)));
    }

    #[test]
    fn hex_colors() {
        let c = Color::from_hex("#ff8000").unwrap();
        assert_eq!(c.to_hex(), "#ff8000");
    }
}
