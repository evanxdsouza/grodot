#!/usr/bin/env python3
"""Generate the original (CC0) textures used by the example projects.

Usage: python3 tools/gen_textures.py <output-dir>
Every texture is painted procedurally here with Pillow, so the examples ship
no third-party art. All results are released under CC0.
"""
import math
import os
import random
import sys

from PIL import Image, ImageDraw, ImageFilter

out = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(out, exist_ok=True)


def tileable_noise(size, scale, seed, octaves=4):
    """Value noise that tiles seamlessly (periodic lattice)."""
    rnd = random.Random(seed)
    result = [[0.0] * size for _ in range(size)]
    amp, total = 1.0, 0.0
    for o in range(octaves):
        cells = max(1, scale * (2 ** o))
        lattice = [[rnd.random() for _ in range(cells)] for _ in range(cells)]
        for y in range(size):
            fy = y / size * cells
            y0 = int(fy) % cells
            y1 = (y0 + 1) % cells
            ty = fy - int(fy)
            ty = ty * ty * (3 - 2 * ty)
            for x in range(size):
                fx = x / size * cells
                x0 = int(fx) % cells
                x1 = (x0 + 1) % cells
                tx = fx - int(fx)
                tx = tx * tx * (3 - 2 * tx)
                a = lattice[y0][x0] + (lattice[y0][x1] - lattice[y0][x0]) * tx
                b = lattice[y1][x0] + (lattice[y1][x1] - lattice[y1][x0]) * tx
                result[y][x] += (a + (b - a) * ty) * amp
        total += amp
        amp *= 0.5
    return [[v / total for v in row] for row in result]


def save(img, name):
    path = os.path.join(out, name)
    img.save(path, optimize=True)
    print("wrote", path, img.size)


def grass():
    n = 128
    noise = tileable_noise(n, 4, 11)
    img = Image.new("RGB", (n, n))
    px = img.load()
    rnd = random.Random(5)
    for y in range(n):
        for x in range(n):
            v = noise[y][x]
            px[x, y] = (int(60 + 40 * v), int(120 + 60 * v), int(45 + 25 * v))
    d = ImageDraw.Draw(img)
    for _ in range(900):  # blades
        x, y = rnd.randrange(n), rnd.randrange(n)
        length = rnd.randrange(3, 8)
        lean = rnd.uniform(-1.5, 1.5)
        shade = rnd.randrange(-25, 35)
        color = (max(0, 70 + shade), min(255, 150 + shade), max(0, 50 + shade // 2))
        for dx in (-n, 0, n):
            for dy in (-n, 0, n):
                d.line([(x + dx, y + dy), (x + dx + lean, y + dy - length)], fill=color, width=1)
    save(img, "grass.png")


def crate():
    n = 128
    img = Image.new("RGB", (n, n), (150, 104, 58))
    d = ImageDraw.Draw(img)
    rnd = random.Random(8)
    # planks
    for i in range(4):
        y0 = i * 32
        base = 140 + rnd.randrange(-15, 15)
        for y in range(y0, y0 + 32):
            for x in range(n):
                grain = math.sin((x * 0.15) + math.sin(y * 0.4 + i) * 2.0) * 10
                v = int(base + grain + rnd.randrange(-6, 6))
                img.putpixel((x, y), (v, int(v * 0.7), int(v * 0.4)))
        d.line([(0, y0), (n, y0)], fill=(70, 45, 22), width=2)
    # frame and diagonal brace
    d.rectangle([0, 0, n - 1, n - 1], outline=(78, 50, 25), width=10)
    d.line([(8, n - 9), (n - 9, 8)], fill=(110, 74, 38), width=14)
    d.line([(8, n - 9), (n - 9, 8)], fill=(85, 56, 28), width=2)
    for cx, cy in [(10, 10), (n - 11, 10), (10, n - 11), (n - 11, n - 11)]:  # nails
        d.ellipse([cx - 3, cy - 3, cx + 3, cy + 3], fill=(60, 60, 64))
        d.ellipse([cx - 1, cy - 2, cx + 1, cy], fill=(150, 150, 155))
    save(img, "crate.png")


def fern():
    """A fern frond with a transparent background (for alpha-cutout cards)."""
    n = 128
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    rnd = random.Random(3)
    for stem in range(5):
        angle = math.radians(-90 + (stem - 2) * 22)
        length = 105 - abs(stem - 2) * 12
        x0, y0 = n / 2, n - 2
        pts = []
        for i in range(30):
            t = i / 29
            bend = math.radians((stem - 2) * 14) * t * t
            x = x0 + math.cos(angle + bend) * length * t
            y = y0 + math.sin(angle + bend) * length * t
            pts.append((x, y))
        d.line(pts, fill=(52, 96, 34, 255), width=3)
        for i in range(3, 29, 2):
            t = i / 29
            (x, y) = pts[i]
            leaf = 18 * (1 - t) + 4
            for side in (-1, 1):
                a = angle + side * math.radians(70) + rnd.uniform(-0.2, 0.2)
                tip = (x + math.cos(a) * leaf, y + math.sin(a) * leaf)
                mid = (x + math.cos(a) * leaf * 0.5 + math.cos(a + 1.57) * 2.5,
                       y + math.sin(a) * leaf * 0.5 + math.sin(a + 1.57) * 2.5)
                g = 120 + rnd.randrange(-20, 40)
                d.polygon([(x, y), mid, tip], fill=(40, g, 30, 255))
    save(img, "fern.png")


def water():
    n = 64
    noise = tileable_noise(n, 3, 21, octaves=3)
    img = Image.new("RGBA", (n, n))
    px = img.load()
    for y in range(n):
        for x in range(n):
            v = noise[y][x]
            wave = 0.5 + 0.5 * math.sin((x / n) * 2 * math.pi * 2 + v * 4)
            px[x, y] = (int(40 + 60 * wave), int(110 + 70 * wave), int(170 + 60 * wave), 170)
    save(img, "water.png")


def stone():
    n = 128
    noise = tileable_noise(n, 6, 31)
    img = Image.new("RGB", (n, n))
    px = img.load()
    for y in range(n):
        for x in range(n):
            v = noise[y][x]
            g = int(95 + 80 * v)
            px[x, y] = (g, g - 4, g - 10)
    d = ImageDraw.Draw(img)
    for row in range(4):  # mortar lines for a brick pattern
        y = row * 32
        d.line([(0, y), (n, y)], fill=(60, 58, 54), width=3)
        off = 0 if row % 2 == 0 else 32
        for x in range(off, n + 64, 64):
            d.line([(x % n, y), (x % n, y + 32)], fill=(60, 58, 54), width=3)
    save(img.filter(ImageFilter.SMOOTH), "stone.png")


def checker():
    """High-frequency checker that shows off mipmapping on distant surfaces."""
    n = 64
    img = Image.new("RGB", (n, n))
    px = img.load()
    for y in range(n):
        for x in range(n):
            v = 235 if ((x // 4) + (y // 4)) % 2 == 0 else 35
            px[x, y] = (v, v, v)
    save(img, "checker.png")


grass()
crate()
fern()
water()
stone()
checker()
