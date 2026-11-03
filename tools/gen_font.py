#!/usr/bin/env python3
"""Pre-rasterize the UI fonts used by grodot-render into a compact binary file.

Usage: python3 tools/gen_font.py
Writes crates/grodot-render/assets/fonts.bin

Format (little endian):
  u32 font_count
  per font: u16 line_height, u16 ascent, then for chars 32..=126:
            u8 advance, u8 width, then width*line_height alpha bytes (row-major)
Fonts: Inter (SIL Open Font License 1.1), see docs/THIRD_PARTY.md.
"""
from PIL import Image, ImageDraw, ImageFont
import struct, os

FONTS = [
    ("/usr/share/fonts/opentype/inter/Inter-Regular.otf", 14),
    ("/usr/share/fonts/opentype/inter/Inter-SemiBold.otf", 14),
    ("/usr/share/fonts/opentype/inter/Inter-Bold.otf", 30),
]

out = bytearray()
out += struct.pack("<I", len(FONTS))
for path, size in FONTS:
    font = ImageFont.truetype(path, size)
    ascent, descent = font.getmetrics()
    line_h = ascent + descent
    out += struct.pack("<HH", line_h, ascent)
    for code in range(32, 127):
        ch = chr(code)
        adv = int(round(font.getlength(ch)))
        bbox = font.getbbox(ch)
        left = min(0, bbox[0])
        w = max(adv, bbox[2]) - left
        w = max(w, 1)
        img = Image.new("L", (w, line_h), 0)
        d = ImageDraw.Draw(img)
        d.text((-left, 0), ch, font=font, fill=255)
        out += struct.pack("<BB", adv, w)
        out += img.tobytes()
dst = os.path.join(os.path.dirname(__file__), "..", "crates", "grodot-render", "assets", "fonts.bin")
with open(dst, "wb") as f:
    f.write(out)
print("wrote", dst, len(out), "bytes")
