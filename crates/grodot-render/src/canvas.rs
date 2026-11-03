//! A CPU framebuffer with 2D drawing helpers, anti-aliased text and PNG export.

use crate::font::{Font, FontId, FONTS};

/// Packed 0x00RRGGBB colour.
pub type Rgb = u32;

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[inline]
pub fn blend(dst: Rgb, src: Rgb, alpha: u32) -> Rgb {
    if alpha >= 255 {
        return src;
    }
    if alpha == 0 {
        return dst;
    }
    let inv = 255 - alpha;
    let r = (((src >> 16) & 255) * alpha + ((dst >> 16) & 255) * inv) / 255;
    let g = (((src >> 8) & 255) * alpha + ((dst >> 8) & 255) * inv) / 255;
    let b = ((src & 255) * alpha + (dst & 255) * inv) / 255;
    (r << 16) | (g << 8) | b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn shrink(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2 * d).max(0), (self.h - 2 * d).max(0))
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w).min(o.x + o.w);
        let y1 = (self.y + self.h).min(o.y + o.h);
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub color: Vec<Rgb>,
    pub depth: Vec<f32>,
    /// Current clip rectangle for 2D drawing.
    pub clip: Rect,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Canvas {
        let (w, h) = (width.max(1), height.max(1));
        Canvas {
            width: w,
            height: h,
            color: vec![0; w * h],
            depth: vec![f32::INFINITY; w * h],
            clip: Rect::new(0, 0, w as i32, h as i32),
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        let (w, h) = (width.max(1), height.max(1));
        if w == self.width && h == self.height {
            return;
        }
        self.width = w;
        self.height = h;
        self.color = vec![0; w * h];
        self.depth = vec![f32::INFINITY; w * h];
        self.clip = self.full_rect();
    }

    pub fn full_rect(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    pub fn set_clip(&mut self, r: Rect) {
        self.clip = r.intersect(&self.full_rect());
    }

    pub fn reset_clip(&mut self) {
        self.clip = self.full_rect();
    }

    pub fn clear(&mut self, c: Rgb) {
        self.color.fill(c);
        self.depth.fill(f32::INFINITY);
    }

    pub fn clear_depth_rect(&mut self, r: Rect) {
        let r = r.intersect(&self.full_rect());
        for y in r.y..r.bottom() {
            let row = y as usize * self.width;
            self.depth[row + r.x as usize..row + r.right() as usize].fill(f32::INFINITY);
        }
    }

    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: Rgb, alpha: u32) {
        if self.clip.contains(x, y) {
            let i = y as usize * self.width + x as usize;
            self.color[i] = blend(self.color[i], c, alpha);
        }
    }

    pub fn fill_rect(&mut self, r: Rect, c: Rgb) {
        self.fill_rect_alpha(r, c, 255);
    }

    pub fn fill_rect_alpha(&mut self, r: Rect, c: Rgb, alpha: u32) {
        let r = r.intersect(&self.clip);
        for y in r.y..r.bottom() {
            let row = y as usize * self.width;
            for x in r.x..r.right() {
                let i = row + x as usize;
                self.color[i] = blend(self.color[i], c, alpha);
            }
        }
    }

    /// Rectangle with slightly rounded corners.
    pub fn fill_round_rect(&mut self, r: Rect, c: Rgb, alpha: u32) {
        if r.w < 4 || r.h < 4 {
            return self.fill_rect_alpha(r, c, alpha);
        }
        self.fill_rect_alpha(Rect::new(r.x + 1, r.y, r.w - 2, 1), c, alpha);
        self.fill_rect_alpha(Rect::new(r.x, r.y + 1, r.w, r.h - 2), c, alpha);
        self.fill_rect_alpha(Rect::new(r.x + 1, r.bottom() - 1, r.w - 2, 1), c, alpha);
    }

    pub fn stroke_rect(&mut self, r: Rect, c: Rgb) {
        self.fill_rect(Rect::new(r.x, r.y, r.w, 1), c);
        self.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), c);
        self.fill_rect(Rect::new(r.x, r.y, 1, r.h), c);
        self.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), c);
    }

    pub fn vgradient(&mut self, r: Rect, top: Rgb, bottom: Rgb) {
        let r2 = r.intersect(&self.clip);
        for y in r2.y..r2.bottom() {
            let t = if r.h <= 1 { 0 } else { ((y - r.y) * 255 / (r.h - 1)).clamp(0, 255) as u32 };
            let c = blend(top, bottom, t);
            let row = y as usize * self.width;
            self.color[row + r2.x as usize..row + r2.right() as usize].fill(c);
        }
    }

    /// Anti-aliased-ish 2D line (Wu-style without the endpoints nicety).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
        if steps > 20000 {
            return;
        }
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let x = x0 + dx * t;
            let y = y0 + dy * t;
            self.put(x.round() as i32, y.round() as i32, c, 255);
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgb, alpha: u32) {
        let x0 = (cx - r - 1.0).floor() as i32;
        let x1 = (cx + r + 1.0).ceil() as i32;
        let y0 = (cy - r - 1.0).floor() as i32;
        let y1 = (cy + r + 1.0).ceil() as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let cov = (r - d + 0.5).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.put(x, y, c, (cov * alpha as f32) as u32);
                }
            }
        }
    }

    pub fn font(&self, id: FontId) -> &'static Font {
        &FONTS[id as usize]
    }

    /// Draw text with its top-left at (x, y). Returns the x after the last glyph.
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: Rgb, font: FontId) -> i32 {
        let f = &FONTS[font as usize];
        let mut cx = x;
        for ch in s.chars() {
            let g = f.glyph(ch);
            if cx > self.clip.right() {
                break;
            }
            for gy in 0..f.line_height as i32 {
                let py = y + gy;
                if py < self.clip.y || py >= self.clip.bottom() {
                    continue;
                }
                let row = &g.alpha[(gy as usize) * g.width as usize..(gy as usize + 1) * g.width as usize];
                for (gx, &a) in row.iter().enumerate() {
                    if a > 0 {
                        self.put(cx + gx as i32, py, c, a as u32);
                    }
                }
            }
            cx += g.advance as i32;
        }
        cx
    }

    /// Text with a dark drop shadow (for HUD readability over 3D scenes).
    pub fn text_shadow(&mut self, x: i32, y: i32, s: &str, c: Rgb, font: FontId) -> i32 {
        self.text(x + 1, y + 1, s, 0x101010, font);
        self.text(x, y, s, c, font)
    }

    pub fn measure(&self, s: &str, font: FontId) -> i32 {
        FONTS[font as usize].measure(s)
    }

    /// Encode the framebuffer as a PNG (RGB, uncompressed deflate blocks).
    pub fn to_png(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity((self.width * 3 + 1) * self.height);
        for y in 0..self.height {
            raw.push(0);
            for x in 0..self.width {
                let c = self.color[y * self.width + x];
                raw.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8]);
            }
        }
        encode_png(self.width as u32, self.height as u32, &raw)
    }

    pub fn save_png(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_png())
    }
}

static CRC_TABLE: std::sync::LazyLock<[u32; 256]> = std::sync::LazyLock::new(|| {
    let mut t = [0u32; 256];
    for (i, slot) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    t
});

fn crc32(data: &[u8]) -> u32 {
    let t = &*CRC_TABLE;
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = t[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn encode_png(w: u32, h: u32, raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let chunk = |out: &mut Vec<u8>, ty: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = ty.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc32(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    // zlib stream with stored blocks
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = c.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn png_header() {
        let c = Canvas::new(4, 3);
        let png = c.to_png();
        assert_eq!(&png[1..4], b"PNG");
        assert!(png.len() > 50);
    }

    #[test]
    fn blending() {
        assert_eq!(blend(0x000000, 0xffffff, 255), 0xffffff);
        assert_eq!(blend(0x000000, 0xffffff, 0), 0x000000);
        let mid = blend(0x000000, 0xfefefe, 128);
        assert!((mid >> 16) > 100 && (mid >> 16) < 160);
    }

    #[test]
    fn text_draws_pixels() {
        let mut c = Canvas::new(100, 30);
        c.clear(0);
        let end = c.text(2, 2, "Hi", 0xffffff, FontId::Ui);
        assert!(end > 2);
        assert!(c.color.iter().any(|&p| p != 0));
    }
}
