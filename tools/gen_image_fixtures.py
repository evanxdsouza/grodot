#!/usr/bin/env python3
"""Generate PNG/BMP test fixtures and their expected RGBA bytes.

Usage: python3 tools/gen_image_fixtures.py
Writes crates/grodot-core/tests/fixtures/images/<name>.{png,bmp} and <name>.rgba
(raw RGBA8, top row first) using Pillow, an independent encoder, so the
decoder is checked against a reference implementation.
"""
import os, random, zlib, struct
from PIL import Image

out = os.path.join(os.path.dirname(__file__), "..", "crates", "grodot-core", "tests", "fixtures", "images")
os.makedirs(out, exist_ok=True)
random.seed(1234)

def base(w, h, alpha=True):
    im = Image.new("RGBA", (w, h))
    px = im.load()
    for y in range(h):
        for x in range(w):
            r = (x * 255) // max(1, w - 1)
            g = (y * 255) // max(1, h - 1)
            b = random.randrange(256)
            a = 255 if not alpha else (x * 37 + y * 11) % 256
            px[x, y] = (r, g, b, a)
    return im

def save(name, im, fmt, expect_mode="RGBA", **kw):
    path = os.path.join(out, name + "." + fmt.lower())
    im.save(path, fmt, **kw)
    ref = Image.open(path).convert("RGBA")
    open(os.path.join(out, name + ".rgba"), "wb").write(ref.tobytes())
    print(name, im.mode, im.size, os.path.getsize(path), "bytes")

rgba = base(37, 23)
save("rgba8", rgba, "PNG", compress_level=9)
save("rgb8_level0", rgba.convert("RGB"), "PNG", compress_level=0)
save("rgb8_level1", rgba.convert("RGB"), "PNG", compress_level=1)
save("gray8", rgba.convert("L"), "PNG")
save("gray_alpha8", rgba.convert("LA"), "PNG")
pal = rgba.convert("RGB").quantize(colors=16)
save("palette4", pal, "PNG", bits=4)
save("palette8", rgba.convert("RGB").quantize(colors=200), "PNG")
# Palette with transparency
p2 = Image.new("P", (16, 9))
p2.putpalette([255, 0, 0, 0, 255, 0, 0, 0, 255] + [0] * (253 * 3))
for y in range(9):
    for x in range(16):
        p2.putpixel((x, y), (x + y) % 3)
p2.info["transparency"] = bytes([255, 128, 0])
save("palette_trns", p2, "PNG", transparency=bytes([255, 128, 0]))
save("gray1", rgba.convert("1"), "PNG")
# 16-bit grayscale
im16 = Image.new("I;16", (11, 7))
for y in range(7):
    for x in range(11):
        im16.putpixel((x, y), (x * 6000 + y * 1000) % 65536)
path = os.path.join(out, "gray16.png"); im16.save(path)
ref = bytes()
for y in range(7):
    for x in range(11):
        v = ((x * 6000 + y * 1000) % 65536) >> 8
        ref += bytes([v, v, v, 255])
open(os.path.join(out, "gray16.rgba"), "wb").write(ref)
print("gray16")
# A large compressible image exercises long back-references.
big = Image.new("RGBA", (256, 128), (40, 90, 160, 255))
for y in range(0, 128, 8):
    for x in range(256):
        big.putpixel((x, y), (250, 220, 60, 255))
save("big_stripes", big, "PNG", compress_level=9)
save("bmp24", rgba.convert("RGB"), "BMP")
save("bmp8", rgba.convert("RGB").quantize(colors=64).convert("P"), "BMP")

def chunk(ty, data):
    return struct.pack(">I", len(data)) + ty + data + struct.pack(">I", zlib.crc32(ty + data) & 0xffffffff)

def write_interlaced_png(name, im):
    """Adam7 RGBA8 PNG with a mix of filter types (Pillow cannot write interlaced PNGs)."""
    w, h = im.size
    px = im.load()
    passes = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)]
    raw = b""
    for pi, (x0, y0, dx, dy) in enumerate(passes):
        xs = list(range(x0, w, dx)); ys = list(range(y0, h, dy))
        if not xs or not ys:
            continue
        prev = bytes(len(xs) * 4)
        for row_i, y in enumerate(ys):
            row = b"".join(bytes(px[x, y]) for x in xs)
            ft = (row_i + pi) % 5
            out_row = bytearray()
            for i, v in enumerate(row):
                a = row[i - 4] if i >= 4 else 0
                b = prev[i]
                c = prev[i - 4] if i >= 4 else 0
                if ft == 0: pred = 0
                elif ft == 1: pred = a
                elif ft == 2: pred = b
                elif ft == 3: pred = (a + b) // 2
                else:
                    pp = a + b - c; pa, pb, pc = abs(pp - a), abs(pp - b), abs(pp - c)
                    pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                out_row.append((v - pred) & 255)
            raw += bytes([ft]) + bytes(out_row)
            prev = row
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 1))
    data += chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    open(os.path.join(out, name + ".png"), "wb").write(data)
    open(os.path.join(out, name + ".rgba"), "wb").write(im.tobytes())
    print(name, "interlaced", im.size)

write_interlaced_png("interlaced_rgba8", base(29, 17))

def write_bmp32_alpha(name, im):
    """32-bit BITFIELDS BMP with an alpha mask (BITMAPV4HEADER), bottom-up."""
    w, h = im.size
    px = im.load()
    pixels = b""
    for y in range(h - 1, -1, -1):
        for x in range(w):
            r, g, b, a = px[x, y]
            pixels += bytes([b, g, r, a])
    hdr = struct.pack("<IiiHHIIiiII", 108, w, h, 1, 32, 3, len(pixels), 2835, 2835, 0, 0)
    hdr += struct.pack("<IIII", 0x00ff0000, 0x0000ff00, 0x000000ff, 0xff000000) + b"BGRs" + bytes(36) + bytes(12)
    off = 14 + len(hdr)
    data = b"BM" + struct.pack("<IHHI", off + len(pixels), 0, 0, off) + hdr + pixels
    open(os.path.join(out, name + ".bmp"), "wb").write(data)
    open(os.path.join(out, name + ".rgba"), "wb").write(im.tobytes())
    print(name, "bmp32 alpha", im.size)

write_bmp32_alpha("bmp32", rgba)
