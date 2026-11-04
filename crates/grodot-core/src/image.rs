//! Image decoding: PNG (all colour types, 1-16 bit, Adam7) and BMP (24/32-bit,
//! 8-bit palettes). Output is always 8-bit RGBA, top row first.

use crate::inflate;

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, top row first.
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Image {
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
        Image { width, height, rgba }
    }

    pub fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Image {
        let n = width as usize * height as usize;
        Image { width, height, rgba: rgba.iter().copied().cycle().take(n * 4).collect() }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    pub fn has_transparency(&self) -> bool {
        self.rgba.chunks_exact(4).any(|p| p[3] < 255)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageError(pub String);

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn bad<T>(m: impl Into<String>) -> Result<T, ImageError> {
    Err(ImageError(m.into()))
}

/// Largest image we accept (keeps a corrupt header from allocating gigabytes).
pub const MAX_DIMENSION: u32 = 16384;
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Decode PNG or BMP by sniffing the signature.
pub fn decode(bytes: &[u8]) -> Result<Image, ImageError> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        decode_png(bytes)
    } else if bytes.starts_with(b"BM") {
        decode_bmp(bytes)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        bad("JPEG images are not supported yet; convert the texture to PNG")
    } else {
        bad("unknown image format (Grodot reads PNG and BMP)")
    }
}

fn check_size(w: u32, h: u32) -> Result<(), ImageError> {
    if w == 0 || h == 0 {
        return bad("image has zero width or height");
    }
    if w > MAX_DIMENSION || h > MAX_DIMENSION || w as u64 * h as u64 > MAX_PIXELS {
        return bad(format!("image is too large ({}x{}); the limit is {} pixels per side", w, h, MAX_DIMENSION));
    }
    Ok(())
}

// ------------------------------------------------------------------ PNG

fn crc32(data: &[&[u8]]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
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
    let mut crc = 0xFFFF_FFFFu32;
    for part in data {
        for &b in *part {
            crc = t[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
    }
    crc ^ 0xFFFF_FFFF
}

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
}

impl Header {
    fn channels(&self) -> usize {
        match self.color {
            0 => 1,
            2 => 3,
            3 => 1,
            4 => 2,
            6 => 4,
            _ => 0,
        }
    }
    fn bits_per_pixel(&self) -> usize {
        self.channels() * self.depth as usize
    }
    fn row_bytes(&self, w: u32) -> usize {
        (w as usize * self.bits_per_pixel()).div_ceil(8)
    }
}

pub fn decode_png(bytes: &[u8]) -> Result<Image, ImageError> {
    if bytes.len() < 8 || bytes[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return bad("not a PNG file");
    }
    let mut pos = 8;
    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 4]> = Vec::new();
    let mut trns_key: Option<[u16; 3]> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_end = false;
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        let ty = &bytes[pos + 4..pos + 8];
        if pos + 12 + len > bytes.len() {
            return bad(format!("PNG chunk {} is truncated", String::from_utf8_lossy(ty)));
        }
        let body = &bytes[pos + 8..pos + 8 + len];
        let crc = u32::from_be_bytes(bytes[pos + 8 + len..pos + 12 + len].try_into().unwrap());
        if crc32(&[ty, body]) != crc {
            return bad(format!("PNG chunk {} has a bad checksum (corrupt file)", String::from_utf8_lossy(ty)));
        }
        match ty {
            b"IHDR" => {
                if body.len() != 13 {
                    return bad("bad IHDR chunk");
                }
                let h = Header {
                    width: u32::from_be_bytes(body[0..4].try_into().unwrap()),
                    height: u32::from_be_bytes(body[4..8].try_into().unwrap()),
                    depth: body[8],
                    color: body[9],
                    interlace: body[12],
                };
                if body[10] != 0 || body[11] != 0 {
                    return bad("unsupported PNG compression or filter method");
                }
                let ok_depth = match h.color {
                    0 => matches!(h.depth, 1 | 2 | 4 | 8 | 16),
                    3 => matches!(h.depth, 1 | 2 | 4 | 8),
                    2 | 4 | 6 => matches!(h.depth, 8 | 16),
                    _ => false,
                };
                if !ok_depth {
                    return bad(format!("invalid PNG colour type {} with bit depth {}", h.color, h.depth));
                }
                if h.interlace > 1 {
                    return bad("unknown PNG interlace method");
                }
                check_size(h.width, h.height)?;
                header = Some(h);
            }
            b"PLTE" => {
                if !len.is_multiple_of(3) || len / 3 > 256 {
                    return bad("bad PLTE chunk");
                }
                palette = body.chunks(3).map(|c| [c[0], c[1], c[2], 255]).collect();
            }
            b"tRNS" => {
                let h = header.as_ref().ok_or_else(|| ImageError("tRNS before IHDR".into()))?;
                match h.color {
                    3 => {
                        for (i, &a) in body.iter().enumerate() {
                            if let Some(p) = palette.get_mut(i) {
                                p[3] = a;
                            }
                        }
                    }
                    0 if body.len() >= 2 => {
                        let g = u16::from_be_bytes([body[0], body[1]]);
                        trns_key = Some([g, g, g]);
                    }
                    2 if body.len() >= 6 => {
                        trns_key = Some([
                            u16::from_be_bytes([body[0], body[1]]),
                            u16::from_be_bytes([body[2], body[3]]),
                            u16::from_be_bytes([body[4], body[5]]),
                        ]);
                    }
                    _ => {}
                }
            }
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => {
                seen_end = true;
                break;
            }
            _ => {
                // Ancillary chunks (lowercase first letter) are skipped; unknown critical ones are errors.
                if ty[0].is_ascii_uppercase() {
                    return bad(format!("unsupported critical PNG chunk {}", String::from_utf8_lossy(ty)));
                }
            }
        }
        pos += 12 + len;
    }
    let h = header.ok_or_else(|| ImageError("PNG has no IHDR chunk".into()))?;
    if !seen_end && idat.is_empty() {
        return bad("PNG is truncated (no image data)");
    }
    if h.color == 3 && palette.is_empty() {
        return bad("palette PNG has no PLTE chunk");
    }
    // Expected size of the filtered data, for the bomb limit.
    let passes: Vec<(u32, u32, u32, u32, u32, u32)> = if h.interlace == 1 {
        // (x0, y0, dx, dy, w, h)
        let p = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
        p.iter()
            .map(|&(x0, y0, dx, dy)| {
                let w = if h.width > x0 { (h.width - x0).div_ceil(dx) } else { 0 };
                let hh = if h.height > y0 { (h.height - y0).div_ceil(dy) } else { 0 };
                (x0, y0, dx, dy, w, hh)
            })
            .collect()
    } else {
        vec![(0, 0, 1, 1, h.width, h.height)]
    };
    let expected: usize =
        passes.iter().filter(|p| p.4 > 0 && p.5 > 0).map(|p| (h.row_bytes(p.4) + 1) * p.5 as usize).sum();
    let raw = inflate::zlib_decompress(&idat, expected, expected + 1024)
        .map_err(|e| ImageError(format!("PNG image data: {}", e)))?;
    if raw.len() < expected {
        return bad("PNG image data is shorter than the image size");
    }
    let mut out = vec![0u8; h.width as usize * h.height as usize * 4];
    let mut off = 0;
    for &(x0, y0, dx, dy, pw, ph) in &passes {
        if pw == 0 || ph == 0 {
            continue;
        }
        let rb = h.row_bytes(pw);
        let size = (rb + 1) * ph as usize;
        let rows = unfilter(&raw[off..off + size], rb, ph as usize, h.bits_per_pixel().div_ceil(8))?;
        off += size;
        for py in 0..ph as usize {
            let row = &rows[py * rb..(py + 1) * rb];
            for px in 0..pw as usize {
                let rgba = sample(&h, row, px, &palette, trns_key);
                let x = x0 as usize + px * dx as usize;
                let y = y0 as usize + py * dy as usize;
                let i = (y * h.width as usize + x) * 4;
                out[i..i + 4].copy_from_slice(&rgba);
            }
        }
    }
    Ok(Image { width: h.width, height: h.height, rgba: out })
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Undo PNG scanline filters. `bpp` is bytes per complete pixel (at least 1).
fn unfilter(data: &[u8], row_bytes: usize, rows: usize, bpp: usize) -> Result<Vec<u8>, ImageError> {
    let mut out = vec![0u8; row_bytes * rows];
    for y in 0..rows {
        let ft = data[y * (row_bytes + 1)];
        let src = &data[y * (row_bytes + 1) + 1..(y + 1) * (row_bytes + 1)];
        let (prev_rows, cur_rows) = out.split_at_mut(y * row_bytes);
        let prev = if y > 0 { &prev_rows[(y - 1) * row_bytes..] } else { &[][..] };
        let cur = &mut cur_rows[..row_bytes];
        for x in 0..row_bytes {
            let a = if x >= bpp { cur[x - bpp] } else { 0 };
            let b = if y > 0 { prev[x] } else { 0 };
            let c = if y > 0 && x >= bpp { prev[x - bpp] } else { 0 };
            cur[x] = match ft {
                0 => src[x],
                1 => src[x].wrapping_add(a),
                2 => src[x].wrapping_add(b),
                3 => src[x].wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => src[x].wrapping_add(paeth(a, b, c)),
                _ => return bad(format!("invalid PNG filter type {}", ft)),
            };
        }
    }
    Ok(out)
}

fn sample(h: &Header, row: &[u8], x: usize, palette: &[[u8; 4]], key: Option<[u16; 3]>) -> [u8; 4] {
    let d = h.depth as usize;
    let small = |x: usize| -> u16 {
        // packed sample for depths < 8
        let bit = x * d;
        let byte = row[bit / 8];
        let shift = 8 - d - (bit % 8);
        ((byte >> shift) & ((1 << d) - 1) as u8) as u16
    };
    let s16 = |i: usize| -> u16 { u16::from_be_bytes([row[i * 2], row[i * 2 + 1]]) };
    let to8 = |v: u16| -> u8 {
        match d {
            16 => (v >> 8) as u8,
            8 => v as u8,
            _ => (v * 255 / ((1 << d) - 1)) as u8,
        }
    };
    match h.color {
        0 => {
            let g = if d < 8 {
                small(x)
            } else if d == 8 {
                row[x] as u16
            } else {
                s16(x)
            };
            let a = if key.map(|k| k[0] == g).unwrap_or(false) { 0 } else { 255 };
            let v = to8(g);
            [v, v, v, a]
        }
        2 => {
            let (r, g, b) = if d == 8 {
                (row[x * 3] as u16, row[x * 3 + 1] as u16, row[x * 3 + 2] as u16)
            } else {
                (s16(x * 3), s16(x * 3 + 1), s16(x * 3 + 2))
            };
            let a = if key == Some([r, g, b]) { 0 } else { 255 };
            [to8(r), to8(g), to8(b), a]
        }
        3 => {
            let i = if d < 8 { small(x) as usize } else { row[x] as usize };
            palette.get(i).copied().unwrap_or([0, 0, 0, 255])
        }
        4 => {
            if d == 8 {
                let g = row[x * 2];
                [g, g, g, row[x * 2 + 1]]
            } else {
                let g = to8(s16(x * 2));
                [g, g, g, to8(s16(x * 2 + 1))]
            }
        }
        _ => {
            if d == 8 {
                [row[x * 4], row[x * 4 + 1], row[x * 4 + 2], row[x * 4 + 3]]
            } else {
                [to8(s16(x * 4)), to8(s16(x * 4 + 1)), to8(s16(x * 4 + 2)), to8(s16(x * 4 + 3))]
            }
        }
    }
}

// ------------------------------------------------------------------ BMP

pub fn decode_bmp(bytes: &[u8]) -> Result<Image, ImageError> {
    let rd16 = |o: usize| -> Result<u16, ImageError> {
        bytes
            .get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .ok_or_else(|| ImageError("BMP is truncated".into()))
    };
    let rd32 = |o: usize| -> Result<u32, ImageError> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| ImageError("BMP is truncated".into()))
    };
    if !bytes.starts_with(b"BM") {
        return bad("not a BMP file");
    }
    let data_off = rd32(10)? as usize;
    let hsize = rd32(14)? as usize;
    if hsize < 40 {
        return bad("old OS/2 BMP headers are not supported");
    }
    let w = rd32(18)? as i32;
    let hraw = rd32(22)? as i32;
    let bpp = rd16(28)?;
    let compression = rd32(30)?;
    if w <= 0 || hraw == 0 {
        return bad("BMP has invalid dimensions");
    }
    let top_down = hraw < 0;
    let hgt = hraw.unsigned_abs();
    check_size(w as u32, hgt)?;
    // BI_RGB = 0, BI_BITFIELDS = 3 (accepted for 32-bit with standard masks)
    let masks = if compression == 3 && bpp == 32 {
        Some((rd32(54)?, rd32(58)?, rd32(62)?, if hsize >= 56 { rd32(66)? } else { 0 }))
    } else if compression == 0 {
        None
    } else {
        return bad("compressed BMP files are not supported; save as uncompressed 24 or 32-bit");
    };
    let palette: Vec<[u8; 4]> = if bpp <= 8 {
        let n = match rd32(46)? {
            0 => 1usize << bpp,
            n => n as usize,
        };
        (0..n.min(256))
            .map(|i| {
                let o = 14 + hsize + i * 4;
                bytes.get(o..o + 4).map(|p| [p[2], p[1], p[0], 255]).unwrap_or([0, 0, 0, 255])
            })
            .collect()
    } else {
        Vec::new()
    };
    let stride = (w as usize * bpp as usize).div_ceil(32) * 4;
    if data_off + stride * hgt as usize > bytes.len() {
        return bad("BMP pixel data is truncated");
    }
    let mut out = vec![0u8; w as usize * hgt as usize * 4];
    let channel = |v: u32, mask: u32| -> u8 {
        if mask == 0 {
            return 255;
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).count_ones();
        let raw = (v & mask) >> shift;
        if bits >= 8 {
            (raw >> (bits - 8)) as u8
        } else {
            (raw * 255 / ((1 << bits) - 1)) as u8
        }
    };
    for row in 0..hgt as usize {
        let src_row = if top_down { row } else { hgt as usize - 1 - row };
        let line = &bytes[data_off + src_row * stride..data_off + (src_row + 1) * stride];
        for x in 0..w as usize {
            let px: [u8; 4] = match bpp {
                32 => {
                    let p = &line[x * 4..x * 4 + 4];
                    match masks {
                        Some((rm, gm, bm, am)) => {
                            let v = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                            [channel(v, rm), channel(v, gm), channel(v, bm), channel(v, am)]
                        }
                        // BI_RGB 32-bit: the fourth byte is usually unused; treat 0 alpha everywhere as opaque.
                        None => [p[2], p[1], p[0], p[3]],
                    }
                }
                24 => {
                    let p = &line[x * 3..x * 3 + 3];
                    [p[2], p[1], p[0], 255]
                }
                8 => palette.get(line[x] as usize).copied().unwrap_or([0, 0, 0, 255]),
                4 => {
                    let b = line[x / 2];
                    let i = if x % 2 == 0 { b >> 4 } else { b & 15 };
                    palette.get(i as usize).copied().unwrap_or([0, 0, 0, 255])
                }
                1 => {
                    let b = line[x / 8];
                    let i = (b >> (7 - x % 8)) & 1;
                    palette.get(i as usize).copied().unwrap_or([0, 0, 0, 255])
                }
                _ => return bad(format!("unsupported BMP bit depth {}", bpp)),
            };
            let i = (row * w as usize + x) * 4;
            out[i..i + 4].copy_from_slice(&px);
        }
    }
    if bpp == 32 && masks.is_none() && out.chunks_exact(4).all(|p| p[3] == 0) {
        for p in out.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }
    Ok(Image { width: w as u32, height: hgt, rgba: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_formats() {
        assert!(decode(b"GIF89a....").is_err());
        assert!(decode(&[0xff, 0xd8, 0xff]).unwrap_err().0.contains("JPEG"));
    }

    #[test]
    fn paeth_predictor() {
        assert_eq!(paeth(10, 20, 30), 10);
        assert_eq!(paeth(30, 20, 10), 30);
    }
}
