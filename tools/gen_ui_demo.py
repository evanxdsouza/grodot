#!/usr/bin/env python3
"""Generate examples/ui-demo: an original CC0 nine-slice panel skin and the scenes.

    python3 tools/gen_ui_demo.py

Standard library only. The skin is a 24x24 bevelled panel; a border of 6 keeps
its corners and bevel at any size.
"""
import json
import os
import struct
import zlib

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "examples", "ui-demo")
SIZE = 24
BORDER = 6


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


def panel(x, y):
    d = min(x, y, SIZE - 1 - x, SIZE - 1 - y)
    if d == 0:
        return (20, 24, 34, 255)  # outline
    if d < BORDER:
        # Bevel: light on the top-left, dark on the bottom-right.
        return (98, 116, 146, 255) if x + y < SIZE else (34, 42, 58, 255)
    return (43, 52, 66, 240)  # fill


def t(d, i):
    return d[i]


def main():
    write_png(os.path.join(ROOT, "assets/ui/panel.png"), SIZE, SIZE, panel)

    def T(p, r=(0, 0, 0), s=(1, 1, 1)):
        return {"type": "Transform", "position": list(p), "rotation_degrees": list(r), "scale": list(s)}

    def ent(i, name, comps):
        return {"id": i, "name": name, "parent": None, "active": True, "tag": "", "components": comps}

    camera = {"type": "Camera", "fov": 55, "near": 0.1, "far": 100, "active": True}
    sun = {"type": "Light", "light_type": "directional", "color": "#fff1d6", "intensity": 1.0}
    spinner = {"type": "Script", "path": "scripts/spinner.gro", "enabled": True}
    crate = {"type": "MeshRenderer", "mesh": "cube", "lit": True, "visible": True,
             "material": {"base_color": "#b07a45"}}

    menu = {
        "scene_version": 1, "name": "Menu",
        "environment": {"background": "#1c2a44", "ambient": 0.45},
        "entities": [
            ent(1, "Camera", [T((0, 1.2, 5), ), camera]),
            ent(2, "Sun", [T((0, 8, 0)), sun]),
            ent(3, "Crate", [T((2.6, 0.8, 0), (0, 0, 0), (1.5, 1.5, 1.5)), crate, spinner]),
            ent(4, "Menu", [T((0, 0, 0)), {"type": "Script", "path": "scripts/menu.gro", "enabled": True}]),
        ],
    }
    game = {
        "scene_version": 1, "name": "Game",
        "environment": {"background": "#8db7e6", "ambient": 0.5},
        "entities": [
            ent(1, "Camera", [T((0, 3, 6), (-20, 0, 0)), camera]),
            ent(2, "Sun", [T((0, 10, 0), (-50, 30, 0)), sun]),
            ent(3, "Ground", [T((0, 0, 0), s=(30, 1, 30)),
                              {"type": "MeshRenderer", "mesh": "plane", "lit": True, "visible": True,
                               "material": {"base_color": "#6fae5a"}}]),
            ent(4, "Crate", [T((0, 0.75, 0), (0, 0, 0), (1.5, 1.5, 1.5)), crate, spinner]),
            ent(5, "Level", [T((0, 0, 0)), {"type": "Script", "path": "scripts/level.gro", "enabled": True}]),
        ],
    }
    os.makedirs(os.path.join(ROOT, "scenes"), exist_ok=True)
    for name, scene in [("menu", menu), ("game", game)]:
        with open(os.path.join(ROOT, "scenes", name + ".gscene"), "w") as f:
            json.dump(scene, f, indent=1)
            f.write("\n")
    print("wrote", ROOT)


if __name__ == "__main__":
    main()
