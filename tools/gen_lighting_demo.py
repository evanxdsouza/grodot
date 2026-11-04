#!/usr/bin/env python3
"""Generate examples/lighting-demo: textures (original, CC0) and the scene.

    python3 tools/gen_lighting_demo.py

Uses only the standard library (zlib PNG writer), so it runs anywhere.
"""
import json
import math
import os
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "lighting-demo")


def write_png(path, w, h, pixel):
    rows = bytearray()
    for y in range(h):
        rows.append(0)
        for x in range(w):
            rows.extend(bytes(pixel(x, y)))

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b"")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(png)


def hash2(x, y):
    n = (x * 374761393 + y * 668265263) & 0xFFFFFFFF
    n = ((n ^ (n >> 13)) * 1274126177) & 0xFFFFFFFF
    return (n ^ (n >> 16)) / 0xFFFFFFFF


def tiles(x, y):
    # 64x64: 4x4 stone tiles with dark grout and per-tile tint.
    tx, ty = x // 16, y // 16
    gx, gy = x % 16, y % 16
    if gx == 0 or gy == 0:
        return (70, 66, 62, 255)
    t = 0.85 + 0.15 * hash2(tx, ty)
    n = 0.9 + 0.1 * hash2(x, y)
    v = 175 * t * n
    return (int(v), int(v * 0.96), int(v * 0.9), 255)


def runes(x, y):
    # 64x64 emissive mask: glowing rune strokes on black.
    u, v = (x + 0.5) / 64.0, (y + 0.5) / 64.0
    d = 1e9
    # Circle
    d = min(d, abs(math.hypot(u - 0.5, v - 0.5) - 0.36))
    # Three strokes inside
    for (ax, ay, bx, by) in [(0.5, 0.22, 0.5, 0.78), (0.3, 0.4, 0.7, 0.62), (0.3, 0.62, 0.7, 0.4)]:
        px, py = u - ax, v - ay
        dx, dy = bx - ax, by - ay
        t = max(0.0, min(1.0, (px * dx + py * dy) / (dx * dx + dy * dy)))
        d = min(d, math.hypot(px - dx * t, py - dy * t))
    g = max(0.0, 1.0 - d / 0.035)
    g = g * g
    c = int(255 * g)
    return (c, c, c, 255)


def T(p, r=(0, 0, 0), s=(1, 1, 1)):
    return {"type": "Transform", "position": list(p), "rotation_degrees": list(r), "scale": list(s)}


def ent(i, name, comps, tag=""):
    return {"id": i, "name": name, "parent": None, "active": True, "tag": tag, "components": comps}


def mr(mesh, material, lit=True, cast=True):
    return {"type": "MeshRenderer", "mesh": mesh, "lit": lit, "visible": True, "cast_shadows": cast, "material": material}


def light(kind, color, intensity, rng=10, angle=45, shadows=True):
    return {"type": "Light", "light_type": kind, "color": color, "intensity": intensity, "range": rng,
            "spot_angle": angle, "shadows": shadows}


def script(path):
    return {"type": "Script", "path": path, "enabled": True}


def main():
    write_png(os.path.join(ROOT, "assets/textures/tiles.png"), 64, 64, tiles)
    write_png(os.path.join(ROOT, "assets/textures/runes.png"), 64, 64, runes)
    e = []
    nid = [0]

    def add(name, comps, tag=""):
        nid[0] += 1
        e.append(ent(nid[0], name, comps, tag))

    add("Camera", [T((0, 6.5, 11), (-27, 0, 0)), {"type": "Camera", "fov": 55, "near": 0.1, "far": 120, "active": True}])
    add("Sun", [T((0, 10, 0), (-42, 35, 0)), light("directional", "#ffd9b0", 0.7), script("scripts/demo.gro")])
    add("Floor", [T((0, 0, 0), s=(22, 1, 22)),
                  mr("plane", {"base_color": "#ffffff", "texture": "assets/textures/tiles.png", "uv_scale": [11, 11]})])
    # Shadow casters
    for i, (x, z, h) in enumerate([(-5, -4, 3.0), (-1.5, -5, 4.0), (2.5, -4.5, 2.5), (5.5, -3, 3.5)]):
        add("Pillar %d" % (i + 1), [T((x, h / 2, z), s=(0.8, h, 0.8)), mr("cube", {"base_color": "#b9b2a6"})])
    add("Crate", [T((-2.5, 0.5, 1.5), (0, 20, 0)), mr("cube", {"base_color": "#a87444"})])
    add("Ball", [T((1.0, 0.6, 2.2), s=(1.2, 1.2, 1.2)), mr("sphere", {"base_color": "#d8d8e0"})])
    add("Statue", [T((4, 1.0, 1.5), s=(0.7, 1.0, 0.7)), mr("capsule", {"base_color": "#e8e2d0"})])
    # Point lights with glowing bulbs (emissive, unshadowed so they do not block the sun)
    bulb = lambda c: mr("sphere", {"base_color": "#202020", "emissive": c}, cast=False)
    add("Warm Lamp", [T((-4, 1.4, 0.5), s=(0.25, 0.25, 0.25)), light("point", "#ff9a40", 1.8, rng=6),
                      bulb("#ffb060"), script("scripts/flicker.gro")])
    add("Cool Lamp", [T((3, 1.2, -1.5), s=(0.25, 0.25, 0.25)), light("point", "#4f8cff", 1.6, rng=6), bulb("#80b0ff")])
    add("Orbiter", [T((0, 1.2, 2), s=(0.2, 0.2, 0.2)), light("point", "#50ff7a", 1.4, rng=4.5), bulb("#90ffb0"),
                    script("scripts/orbit.gro")])
    # Spot light aimed down at the statue
    add("Spot", [T((4, 5, 4), (-60, 0, 0)), light("spot", "#fff6e0", 2.0, rng=9, angle=38)])
    # Emissive objects
    add("Crystal", [T((-1, 0.9, -1.5), (0, 0, 15), s=(0.5, 0.9, 0.5)),
                    mr("capsule", {"base_color": "#103028", "emissive": "#40ffd0"}), script("scripts/pulse.gro")])
    add("Rune Slab", [T((0, 0.08, 4.2), s=(2.2, 0.16, 2.2)),
                      mr("cube", {"base_color": "#5a5550", "texture": "assets/textures/tiles.png",
                                  "emissive": "#ff6a20", "emissive_texture": "assets/textures/runes.png"})])
    scene = {"scene_version": 1, "name": "Lighting", "environment": {"background": "#1c2340", "ambient": 0.22},
             "entities": e}
    os.makedirs(os.path.join(ROOT, "scenes"), exist_ok=True)
    with open(os.path.join(ROOT, "scenes/main.gscene"), "w") as f:
        json.dump(scene, f, indent=1)
        f.write("\n")
    print("wrote", ROOT, len(e), "entities")


if __name__ == "__main__":
    main()
