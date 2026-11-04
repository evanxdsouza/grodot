#!/usr/bin/env python3
"""Generate examples/particles-demo: original CC0 textures and the scene.

    python3 tools/gen_particles_demo.py

Writes flame.png and spark.png (soft sprites with alpha) and copies the grass
texture from examples/textured-demo. Standard library only.
"""
import json
import os
import shutil
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "particles-demo")


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


def lerp(a, b, t):
    return a + (b - a) * t


def flame(x, y):
    # 64x64 soft flame: bright yellow core fading to orange and transparent at the rim.
    u, v = (x + 0.5) / 32.0 - 1.0, (y + 0.5) / 32.0 - 1.0
    r = min(1.0, (u * u + v * v) ** 0.5)
    t = r
    red = 255
    green = int(lerp(245, 110, t))
    blue = int(lerp(190, 30, t))
    alpha = max(0.0, min(1.0, (1.0 - r) ** 1.6 * 1.3))
    return (red, green, blue, int(alpha * 255))


def spark(x, y):
    # 16x16 hot dot: white centre, yellow rim, fully transparent outside.
    u, v = (x + 0.5) / 8.0 - 1.0, (y + 0.5) / 8.0 - 1.0
    r = min(1.0, (u * u + v * v) ** 0.5)
    alpha = max(0.0, min(1.0, 1.0 - r * 1.4))
    return (255, int(lerp(255, 200, r)), int(lerp(255, 80, r)), int(alpha * 255))


def main():
    write_png(os.path.join(ROOT, "assets/textures/flame.png"), 64, 64, flame)
    write_png(os.path.join(ROOT, "assets/textures/spark.png"), 16, 16, spark)
    src = os.path.join(ROOT, "..", "textured-demo", "assets", "textures", "grass.png")
    os.makedirs(os.path.join(ROOT, "assets/textures"), exist_ok=True)
    shutil.copy(src, os.path.join(ROOT, "assets/textures/grass.png"))

    ents = []
    nid = [0]

    def T(p, r=(0, 0, 0), s=(1, 1, 1)):
        return {"type": "Transform", "position": list(p), "rotation_degrees": list(r), "scale": list(s)}

    def add(name, comps, tag=""):
        nid[0] += 1
        ents.append({"id": nid[0], "name": name, "parent": None, "active": True, "tag": tag, "components": comps})

    def mr(mesh, color, texture=None):
        m = {"base_color": color}
        if texture:
            m["texture"] = texture
        return {"type": "MeshRenderer", "mesh": mesh, "lit": True, "visible": True, "material": m}

    def emitter(**kw):
        base = {"type": "ParticleEmitter", "playing": True, "rate": 20, "max_particles": 128, "lifetime": 1.0,
                "lifetime_random": 0.2, "speed": 1.0, "spread": 15, "gravity": [0, 0, 0],
                "size_start": 0.3, "size_end": 0.05, "color_start": "#ffffff", "color_end": "#ffffff",
                "texture": None}
        base.update(kw)
        if base["texture"] is None:
            del base["texture"]
        return base

    add("Camera", [T((0, 2.6, 9), (-10, 0, 0)), {"type": "Camera", "fov": 55, "near": 0.1, "far": 150, "active": True}])
    add("Sun", [T((0, 10, 0), (-55, 30, 0)), {"type": "Light", "light_type": "directional", "color": "#ffe7c8",
                                             "intensity": 0.55, "shadows": True}])
    add("Ground", [T((0, 0, 0), s=(40, 1, 40)),
                   mr("plane", "#ffffff", "assets/textures/grass.png") | {}])
    ents[-1]["components"][1]["material"]["uv_scale"] = [12, 12]

    for i, x in enumerate([-2.2, 2.2]):
        add("Torch %d" % (i + 1), [T((x, 0.9, -1.0), s=(0.35, 1.8, 0.35)), mr("cube", "#6b4a2b")])
        add("Flame %d" % (i + 1), [
            T((x, 2.0, -1.0)),
            emitter(rate=45, lifetime=0.8, lifetime_random=0.4, speed=0.9, spread=12, gravity=[0, 0.8, 0],
                    size_start=0.9, size_end=0.15, color_start="#ffe070", color_end="#ff3a00",
                    texture="assets/textures/flame.png", max_particles=96),
        ])
        add("Sparks %d" % (i + 1), [
            T((x, 2.0, -1.0)),
            emitter(rate=6, lifetime=1.3, speed=2.0, spread=35, gravity=[0, -3.5, 0],
                    size_start=0.12, size_end=0.02, color_start="#ffd060", color_end="#ff6a10",
                    texture="assets/textures/spark.png", max_particles=40),
        ])
        add("Torch %d Light" % (i + 1), [T((x, 2.2, -0.6)),
                                         {"type": "Light", "light_type": "point", "color": "#ff9a40",
                                          "intensity": 0.9, "range": 6}])

    add("Bomb Crate", [T((0, 0.5, 2.5), s=(1, 1, 1)), mr("cube", "#a87444"),
                       {"type": "Collider", "shape": "box", "size": [1, 1, 1]}])
    add("Explosion", [
        T((0, 0.8, 2.5)),
        emitter(playing=False, rate=0, max_particles=300, lifetime=1.2, lifetime_random=0.3, speed=6.0, spread=180,
                gravity=[0, -6, 0], size_start=0.9, size_end=0.2, color_start="#ffe080", color_end="#ff4000",
                texture="assets/textures/flame.png"),
        {"type": "Script", "path": "scripts/bomb.gro", "enabled": True},
    ])

    scene = {"scene_version": 1, "name": "Particles",
             "environment": {"background": "#16182a", "ambient": 0.3}, "entities": ents}
    os.makedirs(os.path.join(ROOT, "scenes"), exist_ok=True)
    with open(os.path.join(ROOT, "scenes/main.gscene"), "w") as f:
        json.dump(scene, f, indent=1)
        f.write("\n")
    print("wrote", ROOT, len(ents), "entities")


if __name__ == "__main__":
    main()
