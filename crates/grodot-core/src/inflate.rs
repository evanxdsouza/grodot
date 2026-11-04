//! DEFLATE (RFC 1951) and zlib (RFC 1950) decompression, written from scratch.
//!
//! Supports stored, fixed-Huffman and dynamic-Huffman blocks. Used by the PNG
//! decoder and by `.glb`-embedded PNG textures.

#[derive(Debug, Clone, PartialEq)]
pub struct InflateError(pub String);

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn err<T>(m: &str) -> Result<T, InflateError> {
    Err(InflateError(m.to_string()))
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u64,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0, bit: 0, nbits: 0 }
    }

    #[inline]
    fn refill(&mut self) {
        while self.nbits <= 56 {
            let b = if self.pos < self.data.len() {
                self.data[self.pos]
            } else {
                // Past the end: feed zeros, the decoder checks for overrun separately.
                0
            };
            self.pos += 1;
            self.bit |= (b as u64) << self.nbits;
            self.nbits += 8;
        }
    }

    #[inline]
    fn need(&mut self, n: u32) {
        if self.nbits < n {
            self.refill();
        }
    }

    #[inline]
    fn get(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.need(n);
        let v = (self.bit & ((1u64 << n) - 1)) as u32;
        self.bit >>= n;
        self.nbits -= n;
        v
    }

    fn align_byte(&mut self) {
        let drop = self.nbits % 8;
        self.get(drop);
    }

    /// Bytes fully consumed from the input (excluding buffered bits).
    fn consumed(&self) -> usize {
        self.pos - (self.nbits / 8) as usize
    }

    fn overrun(&self) -> bool {
        self.consumed() > self.data.len()
    }
}

/// Canonical Huffman decoding table (fast lookup of up to 9 bits, then slow path).
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
    fast: Vec<u32>, // (len << 16) | symbol, 0 = not in fast table
}

const FAST_BITS: u32 = 9;

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, InflateError> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        // Check for over-subscribed code set.
        let mut left: i32 = 1;
        for &c in counts.iter().skip(1) {
            left <<= 1;
            left -= c as i32;
            if left < 0 {
                return err("invalid Huffman code lengths (over-subscribed)");
            }
        }
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + counts[i - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        let mut h = Huffman { counts, symbols, fast: vec![0; 1 << FAST_BITS] };
        // Build the fast table from canonical codes.
        let mut code: u32 = 0;
        let mut idx = 0usize;
        for len in 1..=15u32 {
            for _ in 0..h.counts[len as usize] {
                if len <= FAST_BITS {
                    // reverse code bits (deflate stores Huffman codes MSB-first)
                    let mut rev = 0u32;
                    for b in 0..len {
                        if code & (1 << b) != 0 {
                            rev |= 1 << (len - 1 - b);
                        }
                    }
                    let step = 1 << len;
                    let mut r = rev;
                    while r < (1 << FAST_BITS) {
                        h.fast[r as usize] = (len << 16) | h.symbols[idx] as u32;
                        r += step;
                    }
                }
                code += 1;
                idx += 1;
            }
            code <<= 1;
        }
        Ok(h)
    }

    #[inline]
    fn decode(&self, bits: &mut Bits) -> Result<u16, InflateError> {
        bits.need(16);
        let e = self.fast[(bits.bit & ((1 << FAST_BITS) - 1)) as usize];
        if e != 0 {
            let len = e >> 16;
            bits.bit >>= len;
            bits.nbits -= len;
            return Ok((e & 0xffff) as u16);
        }
        // Slow path: bit by bit (puff-style).
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= bits.get(1) as i32;
            let count = self.counts[len] as i32;
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        err("invalid Huffman code")
    }
}

const LEN_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Decompress a raw DEFLATE stream. `size_hint` pre-allocates the output.
pub fn inflate(data: &[u8], size_hint: usize) -> Result<Vec<u8>, InflateError> {
    inflate_limited(data, size_hint, usize::MAX)
}

/// Like [`inflate`], but fails if the output would exceed `max_out` bytes
/// (protects against decompression bombs in untrusted files).
pub fn inflate_limited(data: &[u8], size_hint: usize, max_out: usize) -> Result<Vec<u8>, InflateError> {
    let mut out: Vec<u8> = Vec::with_capacity(size_hint.min(max_out).min(1 << 28));
    let mut bits = Bits::new(data);
    let fixed_lit = {
        let mut l = [0u8; 288];
        l[..144].fill(8);
        l[144..256].fill(9);
        l[256..280].fill(7);
        l[280..].fill(8);
        Huffman::new(&l)?
    };
    let fixed_dist = Huffman::new(&[5u8; 30])?;
    loop {
        let last = bits.get(1);
        let kind = bits.get(2);
        match kind {
            0 => {
                bits.align_byte();
                let len = bits.get(16) as usize;
                let nlen = bits.get(16) as usize;
                if len != (!nlen & 0xffff) {
                    return err("stored block length check failed");
                }
                // Buffered whole bytes come out of the bit buffer first.
                for _ in 0..len {
                    if bits.overrun() {
                        return err("unexpected end of compressed data");
                    }
                    out.push(bits.get(8) as u8);
                }
                if out.len() > max_out {
                    return err("decompressed data is too large");
                }
            }
            1 => block(&mut bits, &mut out, &fixed_lit, &fixed_dist, max_out)?,
            2 => {
                let hlit = bits.get(5) as usize + 257;
                let hdist = bits.get(5) as usize + 1;
                let hclen = bits.get(4) as usize + 4;
                if hlit > 286 || hdist > 30 {
                    return err("invalid dynamic block header");
                }
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl = [0u8; 19];
                for &o in ORDER.iter().take(hclen) {
                    cl[o] = bits.get(3) as u8;
                }
                let clh = Huffman::new(&cl)?;
                let mut lens = vec![0u8; hlit + hdist];
                let mut i = 0;
                while i < hlit + hdist {
                    let sym = clh.decode(&mut bits)?;
                    match sym {
                        0..=15 => {
                            lens[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return err("repeat with no previous length");
                            }
                            let prev = lens[i - 1];
                            let n = 3 + bits.get(2) as usize;
                            if i + n > lens.len() {
                                return err("code lengths overflow");
                            }
                            lens[i..i + n].fill(prev);
                            i += n;
                        }
                        17 | 18 => {
                            let n = if sym == 17 { 3 + bits.get(3) as usize } else { 11 + bits.get(7) as usize };
                            if i + n > lens.len() {
                                return err("code lengths overflow");
                            }
                            i += n; // already zero
                        }
                        _ => return err("invalid code length symbol"),
                    }
                }
                if lens[256] == 0 {
                    return err("dynamic block has no end-of-block code");
                }
                let lit = Huffman::new(&lens[..hlit])?;
                let dist = Huffman::new(&lens[hlit..])?;
                block(&mut bits, &mut out, &lit, &dist, max_out)?;
            }
            _ => return err("invalid block type"),
        }
        if bits.overrun() {
            return err("unexpected end of compressed data");
        }
        if last == 1 {
            break;
        }
    }
    Ok(out)
}

fn block(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    max_out: usize,
) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(bits)? as usize;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let li = sym - 257;
            if li >= 29 {
                return err("invalid length symbol");
            }
            let len = LEN_BASE[li] as usize + bits.get(LEN_EXTRA[li] as u32) as usize;
            let di = dist.decode(bits)? as usize;
            if di >= 30 {
                return err("invalid distance symbol");
            }
            let d = DIST_BASE[di] as usize + bits.get(DIST_EXTRA[di] as u32) as usize;
            if d > out.len() {
                return err("distance too far back");
            }
            let start = out.len() - d;
            if d >= len {
                out.extend_from_within(start..start + len);
            } else {
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            if out.len() > max_out {
                return err("decompressed data is too large");
            }
        }
        if bits.overrun() {
            return err("unexpected end of compressed data");
        }
    }
}

/// Decompress a zlib stream (2-byte header, DEFLATE data, Adler-32 trailer).
pub fn zlib_decompress(data: &[u8], size_hint: usize, max_out: usize) -> Result<Vec<u8>, InflateError> {
    if data.len() < 2 {
        return err("zlib stream too short");
    }
    let (cmf, flg) = (data[0], data[1]);
    if cmf & 0x0f != 8 || !((cmf as u16) << 8 | flg as u16).is_multiple_of(31) {
        return err("invalid zlib header");
    }
    if flg & 0x20 != 0 {
        return err("zlib preset dictionaries are not supported");
    }
    let out = inflate_limited(&data[2..], size_hint, max_out)?;
    // Verify Adler-32 when the trailer is present.
    if data.len() >= 6 {
        let t = &data[data.len() - 4..];
        let expect = u32::from_be_bytes([t[0], t[1], t[2], t[3]]);
        if adler32(&out) != expect {
            return err("zlib checksum mismatch (corrupt data)");
        }
    }
    Ok(out)
}

pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_block() {
        // zlib header + stored block "abc" + adler
        let mut z = vec![0x78, 0x01, 0x01, 3, 0, 0xfc, 0xff, b'a', b'b', b'c'];
        z.extend_from_slice(&adler32(b"abc").to_be_bytes());
        assert_eq!(zlib_decompress(&z, 0, 100).unwrap(), b"abc");
    }

    #[test]
    fn fixed_huffman_with_back_reference() {
        // Python: zlib.compress(b"hello hello hello hello", 9)
        let z = [0x78, 0xda, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xb1];
        assert_eq!(zlib_decompress(&z, 0, 1000).unwrap(), b"hello hello hello hello");
    }

    #[test]
    fn detects_corruption_and_bombs() {
        let z = [0x78, 0xda, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xb2];
        assert!(zlib_decompress(&z, 0, 1000).is_err());
        let ok = [0x78, 0xda, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01, 0x68, 0x03, 0x08, 0xb1];
        assert!(zlib_decompress(&ok, 0, 5).is_err());
        assert!(zlib_decompress(&[0x78, 0xda, 0xff], 0, 100).is_err());
    }
}
