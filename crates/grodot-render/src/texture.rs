//! Textures with a full mip chain, and the samplers used by the rasterizer.
//!
//! Colours stay in display (sRGB-ish) space, the same space the rest of the
//! renderer lights in, so textured and untextured surfaces match.

use grodot_core::image::Image;
use grodot_core::scene::{TextureFilter, TextureWrap};

/// One mip level, RGBA8 packed as `[r, g, b, a]`.
pub struct Level {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<[u8; 4]>,
}

pub struct Texture {
    pub levels: Vec<Level>,
    pub has_alpha: bool,
}

impl Texture {
    /// Build a texture and its mipmaps (2x2 box filter down to 1x1).
    pub fn from_image(img: &Image) -> Texture {
        let texels: Vec<[u8; 4]> = img.rgba.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        let mut levels = vec![Level { width: img.width, height: img.height, texels }];
        while {
            let l = levels.last().unwrap();
            l.width > 1 || l.height > 1
        } {
            let prev = levels.last().unwrap();
            let (w, h) = ((prev.width / 2).max(1), (prev.height / 2).max(1));
            let mut out = Vec::with_capacity((w * h) as usize);
            for y in 0..h {
                for x in 0..w {
                    let mut acc = [0u32; 4];
                    // Average the 2x2 footprint (clamped for odd or 1-pixel dimensions).
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(prev.width - 1);
                        let sy = (y * 2 + dy).min(prev.height - 1);
                        let t = prev.texels[(sy * prev.width + sx) as usize];
                        for c in 0..4 {
                            acc[c] += t[c] as u32;
                        }
                    }
                    out.push([
                        ((acc[0] + 2) / 4) as u8,
                        ((acc[1] + 2) / 4) as u8,
                        ((acc[2] + 2) / 4) as u8,
                        ((acc[3] + 2) / 4) as u8,
                    ]);
                }
            }
            levels.push(Level { width: w, height: h, texels: out });
        }
        Texture { has_alpha: img.has_transparency(), levels }
    }

    pub fn width(&self) -> u32 {
        self.levels[0].width
    }
    pub fn height(&self) -> u32 {
        self.levels[0].height
    }

    /// Sample at (u, v). `lod` is log2 of texels per pixel at level 0 (from the
    /// rasterizer's UV derivatives); it is only used by trilinear filtering.
    #[inline]
    pub fn sample(&self, u: f32, v: f32, lod: f32, filter: TextureFilter, wrap: TextureWrap) -> [f32; 4] {
        match filter {
            TextureFilter::Nearest => self.nearest(0, u, v, wrap),
            TextureFilter::Bilinear => self.bilinear(0, u, v, wrap),
            TextureFilter::Trilinear => {
                if lod <= 0.0 || self.levels.len() == 1 {
                    return self.bilinear(0, u, v, wrap);
                }
                let max = (self.levels.len() - 1) as f32;
                let lod = lod.min(max);
                let l0 = lod.floor() as usize;
                let t = lod - l0 as f32;
                let a = self.bilinear(l0, u, v, wrap);
                if t < 1e-3 || l0 + 1 >= self.levels.len() {
                    return a;
                }
                let b = self.bilinear(l0 + 1, u, v, wrap);
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
            }
        }
    }

    #[inline]
    fn texel(&self, level: usize, x: i32, y: i32, wrap: TextureWrap) -> [u8; 4] {
        let l = &self.levels[level];
        let (w, h) = (l.width as i32, l.height as i32);
        let (x, y) = match wrap {
            TextureWrap::Repeat => (x.rem_euclid(w), y.rem_euclid(h)),
            TextureWrap::Clamp => (x.clamp(0, w - 1), y.clamp(0, h - 1)),
        };
        l.texels[(y * w + x) as usize]
    }

    #[inline]
    fn nearest(&self, level: usize, u: f32, v: f32, wrap: TextureWrap) -> [f32; 4] {
        let l = &self.levels[level];
        let x = (u * l.width as f32).floor() as i32;
        let y = (v * l.height as f32).floor() as i32;
        let t = self.texel(level, x, y, wrap);
        [t[0] as f32 / 255.0, t[1] as f32 / 255.0, t[2] as f32 / 255.0, t[3] as f32 / 255.0]
    }

    #[inline]
    fn bilinear(&self, level: usize, u: f32, v: f32, wrap: TextureWrap) -> [f32; 4] {
        let l = &self.levels[level];
        // Texel centres sit at half-integer coordinates.
        let fx = u * l.width as f32 - 0.5;
        let fy = v * l.height as f32 - 0.5;
        if !fx.is_finite() || !fy.is_finite() {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let x0 = fx.floor();
        let y0 = fy.floor();
        let tx = fx - x0;
        let ty = fy - y0;
        let (x0, y0) = (x0 as i32, y0 as i32);
        let a = self.texel(level, x0, y0, wrap);
        let b = self.texel(level, x0 + 1, y0, wrap);
        let c = self.texel(level, x0, y0 + 1, wrap);
        let d = self.texel(level, x0 + 1, y0 + 1, wrap);
        let mut out = [0.0f32; 4];
        for i in 0..4 {
            let top = a[i] as f32 + (b[i] as f32 - a[i] as f32) * tx;
            let bot = c[i] as f32 + (d[i] as f32 - c[i] as f32) * tx;
            out[i] = (top + (bot - top) * ty) / 255.0;
        }
        out
    }
}

/// Draw a texture scaled into `r` (alpha-blended, picks a mip close to the size).
pub fn draw_texture(canvas: &mut crate::canvas::Canvas, r: crate::canvas::Rect, tex: &Texture) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let scale = (tex.width() as f32 / r.w as f32).max(tex.height() as f32 / r.h as f32);
    let lod = scale.max(1.0).log2();
    for y in 0..r.h {
        for x in 0..r.w {
            let u = (x as f32 + 0.5) / r.w as f32;
            let v = (y as f32 + 0.5) / r.h as f32;
            let c = tex.sample(u, v, lod, TextureFilter::Trilinear, TextureWrap::Clamp);
            let rgb = ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32;
            canvas.put(r.x + x, r.y + y, rgb, (c[3] * 255.0) as u32);
        }
    }
}

/// Draw the part of a texture between `uv0` and `uv1` (0..1, `[u, v]`) scaled into `r`.
pub fn draw_texture_uv(
    canvas: &mut crate::canvas::Canvas,
    r: crate::canvas::Rect,
    tex: &Texture,
    uv0: [f32; 2],
    uv1: [f32; 2],
) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let (du, dv) = (uv1[0] - uv0[0], uv1[1] - uv0[1]);
    let scale = ((tex.width() as f32 * du) / r.w as f32).max((tex.height() as f32 * dv) / r.h as f32);
    let lod = scale.max(1.0).log2();
    for y in 0..r.h {
        for x in 0..r.w {
            let u = uv0[0] + du * (x as f32 + 0.5) / r.w as f32;
            let v = uv0[1] + dv * (y as f32 + 0.5) / r.h as f32;
            let c = tex.sample(u, v, lod, TextureFilter::Bilinear, TextureWrap::Clamp);
            let rgb = ((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32;
            canvas.put(r.x + x, r.y + y, rgb, (c[3] * 255.0) as u32);
        }
    }
}

/// Draw a texture stretched into `r` as a nine-slice: the corners keep their
/// size (`border` texels, or pixels on screen) and the edges and middle stretch.
pub fn draw_nine_slice(canvas: &mut crate::canvas::Canvas, r: crate::canvas::Rect, tex: &Texture, border: i32) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let (tw, th) = (tex.width() as f32, tex.height() as f32);
    let b = border.max(0) as f32;
    // Corners cannot overlap, even in a small panel.
    let bx = b.min(r.w as f32 / 2.0);
    let by = b.min(r.h as f32 / 2.0);
    let bu = b.min(tw / 2.0);
    let bv = b.min(th / 2.0);
    let xs_src = [0.0, bu, tw - bu, tw];
    let ys_src = [0.0, bv, th - bv, th];
    let xs_dst = [0.0, bx, r.w as f32 - bx, r.w as f32];
    let ys_dst = [0.0, by, r.h as f32 - by, r.h as f32];
    for row in 0..3 {
        for col in 0..3 {
            let x0 = xs_dst[col].round() as i32;
            let x1 = xs_dst[col + 1].round() as i32;
            let y0 = ys_dst[row].round() as i32;
            let y1 = ys_dst[row + 1].round() as i32;
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let dest = crate::canvas::Rect::new(r.x + x0, r.y + y0, x1 - x0, y1 - y0);
            let uv0 = [xs_src[col] / tw, ys_src[row] / th];
            let uv1 = [xs_src[col + 1] / tw, ys_src[row + 1] / th];
            draw_texture_uv(canvas, dest, tex, uv0, uv1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(n: u32) -> Texture {
        let mut rgba = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Texture::from_image(&Image::new(n, n, rgba))
    }

    #[test]
    fn mip_chain_goes_to_one_pixel_and_averages() {
        let t = checker(8);
        assert_eq!(t.levels.len(), 4);
        assert_eq!((t.levels[3].width, t.levels[3].height), (1, 1));
        // A checkerboard averages to mid grey at every mip level.
        assert!((t.levels[1].texels[0][0] as i32 - 128).abs() <= 1);
        let odd = Texture::from_image(&Image::solid(5, 3, [10, 20, 30, 255]));
        assert_eq!(odd.levels.last().unwrap().texels[0], [10, 20, 30, 255]);
    }

    #[test]
    fn nearest_and_wrap_modes() {
        let t = checker(2); // white at (0,0), black at (1,0)
        let f = TextureFilter::Nearest;
        assert_eq!(t.sample(0.25, 0.25, 0.0, f, TextureWrap::Repeat)[0], 1.0);
        assert_eq!(t.sample(0.75, 0.25, 0.0, f, TextureWrap::Repeat)[0], 0.0);
        assert_eq!(t.sample(1.25, 0.25, 0.0, f, TextureWrap::Repeat)[0], 1.0, "repeat wraps to the left texel");
        assert_eq!(t.sample(1.25, 0.25, 0.0, f, TextureWrap::Clamp)[0], 0.0, "clamp sticks to the edge texel");
    }

    #[test]
    fn bilinear_interpolates_between_texel_centres() {
        let t = Texture::from_image(&Image::new(2, 1, vec![0, 0, 0, 255, 255, 255, 255, 255]));
        let mid = t.sample(0.5, 0.5, 0.0, TextureFilter::Bilinear, TextureWrap::Clamp)[0];
        assert!((mid - 0.5).abs() < 1e-5);
        let at_centre = t.sample(0.25, 0.5, 0.0, TextureFilter::Bilinear, TextureWrap::Clamp)[0];
        assert!(at_centre.abs() < 1e-5);
    }

    #[test]
    fn trilinear_uses_smaller_mips_when_minified() {
        let t = checker(64);
        let f = TextureFilter::Trilinear;
        // Far away (lod 6 = whole texture per pixel) the checker blurs to grey.
        let far = t.sample(0.1, 0.1, 6.0, f, TextureWrap::Repeat)[0];
        assert!((far - 0.5).abs() < 0.02, "far = {}", far);
        let half = t.sample(0.1, 0.1, 5.5, f, TextureWrap::Repeat)[0];
        assert!((half - 0.5).abs() < 0.02);
    }
}
