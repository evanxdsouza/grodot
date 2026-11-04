#!/usr/bin/env python3
"""Generate Grodot's original low-poly environment kit (CC0) as glTF 2.0 files.

Usage: python3 tools/gen_gltf_kit.py <output-dir>

The kit exercises every container format the importer supports:
  * .glb with embedded buffers and an embedded PNG texture (house, crate)
  * .gltf + external .bin (lamp post)
  * .gltf with a base64 data: URI buffer (bush)
  * node hierarchies with translation / rotation / scale and a matrix node
  * a LINES primitive (the fence's wire)
All geometry is made here, flat-shaded, and released under CC0.
"""
import base64
import io
import json
import math
import os
import random
import struct
import sys

from PIL import Image, ImageDraw

OUT = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(OUT, exist_ok=True)


class Builder:
    def __init__(self):
        self.bin = bytearray()
        self.doc = {
            "asset": {"version": "2.0", "generator": "grodot tools/gen_gltf_kit.py", "copyright": "CC0"},
            "scene": 0, "scenes": [{"nodes": []}], "nodes": [], "meshes": [], "materials": [],
            "accessors": [], "bufferViews": [], "buffers": [],
        }

    def _view(self, data, target=None):
        while len(self.bin) % 4:
            self.bin.append(0)
        off = len(self.bin)
        self.bin += data
        bv = {"buffer": 0, "byteOffset": off, "byteLength": len(data)}
        if target:
            bv["target"] = target
        self.doc["bufferViews"].append(bv)
        return len(self.doc["bufferViews"]) - 1

    def _accessor(self, floats, kind, comps):
        data = struct.pack("<%df" % len(floats), *floats)
        bv = self._view(data, 34962)
        count = len(floats) // comps
        acc = {"bufferView": bv, "componentType": 5126, "count": count, "type": kind}
        if kind == "VEC3":
            xs, ys, zs = floats[0::3], floats[1::3], floats[2::3]
            acc["min"] = [min(xs), min(ys), min(zs)]
            acc["max"] = [max(xs), max(ys), max(zs)]
        self.doc["accessors"].append(acc)
        return len(self.doc["accessors"]) - 1

    def _indices(self, idx):
        data = struct.pack("<%dH" % len(idx), *idx)
        bv = self._view(data, 34963)
        self.doc["accessors"].append({"bufferView": bv, "componentType": 5123, "count": len(idx), "type": "SCALAR"})
        return len(self.doc["accessors"]) - 1

    def material(self, name, color, texture=None, double_sided=False, alpha=None, emissive=None):
        pbr = {"baseColorFactor": list(color) + ([1.0] if len(color) == 3 else []), "metallicFactor": 0.0, "roughnessFactor": 0.9}
        if texture is not None:
            pbr["baseColorTexture"] = {"index": texture}
        m = {"name": name, "pbrMetallicRoughness": pbr}
        if double_sided:
            m["doubleSided"] = True
        if alpha:
            m["alphaMode"] = alpha
        if emissive:
            m["emissiveFactor"] = list(emissive)
        self.doc["materials"].append(m)
        return len(self.doc["materials"]) - 1

    def png_texture(self, img, nearest=False):
        buf = io.BytesIO()
        img.save(buf, "PNG")
        bv = self._view(buf.getvalue())
        self.doc.setdefault("images", []).append({"bufferView": bv, "mimeType": "image/png"})
        self.doc.setdefault("samplers", []).append(
            {"magFilter": 9728 if nearest else 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497})
        self.doc.setdefault("textures", []).append(
            {"source": len(self.doc["images"]) - 1, "sampler": len(self.doc["samplers"]) - 1})
        return len(self.doc["textures"]) - 1

    def mesh(self, name, parts):
        """parts: list of (faces, material, uv_mode); faces are lists of 3D points (CCW from outside)."""
        prims = []
        for faces, mat, uv_mode in parts:
            pos, nrm, uv, idx = [], [], [], []
            for f in faces:
                a, b, c = f[0], f[1], f[2]
                n = norm(cross(sub(b, a), sub(c, a)))
                base = len(pos) // 3
                for p in f:
                    pos += p
                    nrm += n
                    uv += planar_uv(p, n, uv_mode)
                for k in range(1, len(f) - 1):
                    idx += [base, base + k, base + k + 1]
            prim = {"attributes": {"POSITION": self._accessor(pos, "VEC3", 3), "NORMAL": self._accessor(nrm, "VEC3", 3),
                                   "TEXCOORD_0": self._accessor(uv, "VEC2", 2)},
                    "indices": self._indices(idx), "material": mat}
            prims.append(prim)
        self.doc["meshes"].append({"name": name, "primitives": prims})
        return len(self.doc["meshes"]) - 1

    def lines(self, name, points, mat):
        pos = [c for p in points for c in p]
        prim = {"attributes": {"POSITION": self._accessor(pos, "VEC3", 3)},
                "indices": self._indices(list(range(len(points)))), "material": mat, "mode": 3}
        self.doc["meshes"].append({"name": name, "primitives": [prim]})
        return len(self.doc["meshes"]) - 1

    def node(self, name, mesh=None, t=None, r=None, s=None, children=None, matrix=None, root=False):
        n = {"name": name}
        if mesh is not None:
            n["mesh"] = mesh
        if t:
            n["translation"] = list(t)
        if r:
            n["rotation"] = list(r)
        if s:
            n["scale"] = list(s)
        if matrix:
            n["matrix"] = matrix
        if children:
            n["children"] = children
        self.doc["nodes"].append(n)
        i = len(self.doc["nodes"]) - 1
        if root:
            self.doc["scenes"][0]["nodes"].append(i)
        return i

    def write(self, name, mode):
        """mode: 'glb', 'external' (.gltf + .bin) or 'embedded' (.gltf with a data: URI)."""
        while len(self.bin) % 4:
            self.bin.append(0)
        buf = {"byteLength": len(self.bin)}
        if mode == "external":
            buf["uri"] = name + ".bin"
            open(os.path.join(OUT, name + ".bin"), "wb").write(self.bin)
        elif mode == "embedded":
            buf["uri"] = "data:application/octet-stream;base64," + base64.b64encode(bytes(self.bin)).decode()
        self.doc["buffers"] = [buf]
        if mode == "glb":
            js = json.dumps(self.doc, separators=(",", ":")).encode()
            while len(js) % 4:
                js += b" "
            total = 12 + 8 + len(js) + 8 + len(self.bin)
            out = b"glTF" + struct.pack("<II", 2, total)
            out += struct.pack("<II", len(js), 0x4E4F534A) + js
            out += struct.pack("<II", len(self.bin), 0x004E4942) + bytes(self.bin)
            path = os.path.join(OUT, name + ".glb")
            open(path, "wb").write(out)
        else:
            path = os.path.join(OUT, name + ".gltf")
            open(path, "w").write(json.dumps(self.doc, indent=1))
        print("wrote", path)


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def norm(v):
    l = math.sqrt(sum(x * x for x in v)) or 1.0
    return [x / l for x in v]


def planar_uv(p, n, mode):
    if mode == "box":
        ax = max(range(3), key=lambda i: abs(n[i]))
        if ax == 0:
            return [p[2], 1 - p[1]]
        if ax == 1:
            return [p[0], p[2]]
        return [p[0], 1 - p[1]]
    return [0.0, 0.0]


def box(sx, sy, sz, y0=0.0):
    """Faces of a box from y0 to y0+sy, centred on x/z (CCW seen from outside)."""
    x, z = sx / 2, sz / 2
    y1 = y0 + sy
    v = lambda a, b, c: [a, b, c]
    return [
        [v(-x, y0, z), v(x, y0, z), v(x, y1, z), v(-x, y1, z)],      # +Z
        [v(x, y0, -z), v(-x, y0, -z), v(-x, y1, -z), v(x, y1, -z)],  # -Z
        [v(x, y0, z), v(x, y0, -z), v(x, y1, -z), v(x, y1, z)],      # +X
        [v(-x, y0, -z), v(-x, y0, z), v(-x, y1, z), v(-x, y1, -z)],  # -X
        [v(-x, y1, z), v(x, y1, z), v(x, y1, -z), v(-x, y1, -z)],    # +Y
        [v(-x, y0, -z), v(x, y0, -z), v(x, y0, z), v(-x, y0, z)],    # -Y
    ]


def cone(radius, height, segments, y0=0.0, jitter=0.0, seed=0):
    rnd = random.Random(seed)
    ring = []
    for i in range(segments):
        a = 2 * math.pi * i / segments
        r = radius * (1 + rnd.uniform(-jitter, jitter))
        ring.append([math.cos(a) * r, y0, -math.sin(a) * r])
    tip = [0, y0 + height, 0]
    faces = []
    for i in range(segments):
        a, b = ring[i], ring[(i + 1) % segments]
        faces.append([a, b, tip])
    faces.append(list(reversed(ring)))
    return faces


def prism_roof(sx, sz, h, y0):
    x, z = sx / 2, sz / 2
    a, b, c, d = [-x, y0, z], [x, y0, z], [x, y0, -z], [-x, y0, -z]
    e, f = [-x, y0 + h, 0], [x, y0 + h, 0]
    return [[a, b, f, e], [c, d, e, f], [b, c, f], [d, a, e], [d, c, b, a]]


def cylinder(r, h, segments, y0=0.0):
    faces = []
    ring0 = [[math.cos(2 * math.pi * i / segments) * r, y0, -math.sin(2 * math.pi * i / segments) * r] for i in range(segments)]
    ring1 = [[p[0], y0 + h, p[2]] for p in ring0]
    for i in range(segments):
        j = (i + 1) % segments
        faces.append([ring0[i], ring0[j], ring1[j], ring1[i]])
    faces.append(ring1)
    faces.append(list(reversed(ring0)))
    return faces


def rock_faces(seed, radius=0.6):
    """A jittered octahedron subdivided once: chunky low-poly rock."""
    rnd = random.Random(seed)
    verts = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]]
    tris = [(0, 2, 4), (4, 2, 1), (1, 2, 5), (5, 2, 0), (4, 3, 0), (1, 3, 4), (5, 3, 1), (0, 3, 5)]
    cache = {}

    def mid(a, b):
        key = tuple(sorted((a, b)))
        if key not in cache:
            p = norm([(verts[a][i] + verts[b][i]) / 2 for i in range(3)])
            verts.append(p)
            cache[key] = len(verts) - 1
        return cache[key]
    new = []
    for a, b, c in tris:
        ab, bc, ca = mid(a, b), mid(b, c), mid(c, a)
        new += [(a, ab, ca), (ab, b, bc), (ca, bc, c), (ab, bc, ca)]
    pts = [[v[0] * radius * rnd.uniform(0.8, 1.15), max(v[1], -0.3) * radius * 0.75 * rnd.uniform(0.85, 1.1) + 0.2,
            v[2] * radius * rnd.uniform(0.8, 1.15)] for v in verts]
    return [[pts[a], pts[b], pts[c]] for a, b, c in new]


def quat_y(deg):
    a = math.radians(deg) / 2
    return [0, math.sin(a), 0, math.cos(a)]


def plank_texture():
    img = Image.new("RGB", (64, 64), (176, 132, 86))
    d = ImageDraw.Draw(img)
    rnd = random.Random(2)
    for i in range(0, 64, 8):
        shade = rnd.randrange(-18, 18)
        d.rectangle([0, i, 63, i + 7], fill=(176 + shade, 132 + shade, 86 + shade // 2))
        d.line([(0, i), (63, i)], fill=(110, 78, 46))
        x = rnd.randrange(8, 56)
        d.line([(x, i), (x, i + 7)], fill=(120, 86, 52))
    return img


def window_texture():
    img = Image.new("RGB", (32, 32), (60, 90, 120))
    d = ImageDraw.Draw(img)
    d.rectangle([0, 0, 31, 31], outline=(240, 236, 220), width=3)
    d.line([(16, 0), (16, 31)], fill=(240, 236, 220), width=2)
    d.line([(0, 16), (31, 16)], fill=(240, 236, 220), width=2)
    return img


# ---------------------------------------------------------------- pine tree (glb)
b = Builder()
trunk_m = b.material("Trunk", (0.45, 0.30, 0.18))
leaf_m = b.material("Needles", (0.18, 0.45, 0.25))
trunk = b.mesh("trunk", [(cylinder(0.12, 0.8, 6), trunk_m, None)])
tiers = []
for i, (r, h, y) in enumerate([(0.75, 1.0, 0.6), (0.6, 0.9, 1.15), (0.42, 0.8, 1.65)]):
    tiers.append(b.node("tier%d" % i, b.mesh("tier%d" % i, [(cone(r, h, 7, y, 0.12, i), leaf_m, None)])))
b.node("PineTree", trunk, children=tiers, root=True)
b.write("pine_tree", "glb")

# ---------------------------------------------------------------- round tree (glb)
b = Builder()
trunk_m = b.material("Trunk", (0.42, 0.28, 0.17))
leaf_m = b.material("Leaves", (0.30, 0.58, 0.24))
trunk = b.mesh("trunk", [(cylinder(0.14, 1.1, 6), trunk_m, None)])
crown = b.mesh("crown", [(rock_faces(9, 0.85), leaf_m, None)])
c = b.node("crown", crown, t=[0, 1.0, 0], s=[1.1, 1.0, 1.1])
b.node("RoundTree", trunk, children=[c], root=True)
b.write("round_tree", "glb")

# ---------------------------------------------------------------- rocks (glb, several nodes)
b = Builder()
stone_m = b.material("Stone", (0.55, 0.55, 0.58))
kids = []
for i, (x, z, s, rot) in enumerate([(0, 0, 1.0, 0), (0.7, 0.3, 0.6, 40), (-0.5, 0.5, 0.45, 75)]):
    m = b.mesh("rock%d" % i, [(rock_faces(20 + i), stone_m, None)])
    kids.append(b.node("rock%d" % i, m, t=[x, 0, z], r=quat_y(rot), s=[s, s, s]))
b.node("Rocks", children=kids, root=True)
b.write("rocks", "glb")

# ---------------------------------------------------------------- house (glb + embedded textures)
b = Builder()
wall_tex = b.png_texture(plank_texture())
win_tex = b.png_texture(window_texture(), nearest=True)
wall_m = b.material("Walls", (1.0, 1.0, 1.0), texture=wall_tex)
roof_m = b.material("Roof", (0.62, 0.20, 0.16))
door_m = b.material("Door", (0.35, 0.22, 0.12))
win_m = b.material("Window", (1.0, 1.0, 1.0), texture=win_tex)
chim_m = b.material("Chimney", (0.5, 0.45, 0.42))
walls = b.mesh("walls", [(box(2.4, 1.6, 2.0), wall_m, "box")])
roof = b.mesh("roof", [(prism_roof(2.7, 2.3, 1.0, 1.6), roof_m, None)])
door = b.mesh("door", [(box(0.5, 0.9, 0.06), door_m, None)])
win = b.mesh("window", [(box(0.5, 0.5, 0.06), win_m, "box")])
chim = b.mesh("chimney", [(box(0.3, 0.8, 0.3), chim_m, None)])
parts = [
    b.node("roof", roof),
    b.node("door", door, t=[0, 0, 1.0]),
    b.node("window_left", win, t=[-0.75, 0.75, 1.0]),
    b.node("window_right", win, t=[0.75, 0.75, 1.0]),
    # A node with an explicit matrix (translation 0.7, 2.0, -0.4).
    b.node("chimney", chim, matrix=[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.7, 2.0, -0.4, 1]),
]
b.node("House", walls, children=parts, root=True)
b.write("house", "glb")

# ---------------------------------------------------------------- fence (glb + LINES)
b = Builder()
wood_m = b.material("Wood", (0.62, 0.46, 0.30))
wire_m = b.material("Wire", (0.25, 0.25, 0.27))
post = b.mesh("post", [(box(0.12, 0.9, 0.12), wood_m, None)])
rail = b.mesh("rail", [(box(2.0, 0.08, 0.06), wood_m, None)])
wire = b.lines("wire", [[-1, 0.35, 0], [-0.33, 0.3, 0], [0.33, 0.3, 0], [1, 0.35, 0]], wire_m)
kids = [b.node("post_l", post, t=[-1, 0, 0]), b.node("post_r", post, t=[1, 0, 0]),
        b.node("rail_top", rail, t=[0, 0.7, 0]), b.node("wire", wire)]
b.node("Fence", children=kids, root=True)
b.write("fence", "glb")

# ---------------------------------------------------------------- lamp post (.gltf + external .bin)
b = Builder()
metal_m = b.material("Metal", (0.2, 0.22, 0.25))
glass_m = b.material("Glass", (1.0, 0.92, 0.6), emissive=(1.0, 0.85, 0.45))
pole = b.mesh("pole", [(cylinder(0.06, 2.4, 8), metal_m, None)])
head = b.mesh("head", [(box(0.36, 0.36, 0.36), glass_m, None)])
cap = b.mesh("cap", [(cone(0.3, 0.25, 4, 0.0), metal_m, None)])
h = b.node("lamp", head, t=[0, 2.4, 0])
c = b.node("cap", cap, t=[0, 2.76, 0], r=quat_y(45))
b.node("LampPost", pole, children=[h, c], root=True)
b.write("lamp_post", "external")

# ---------------------------------------------------------------- bush (.gltf with data: URI)
b = Builder()
leaf_m = b.material("Bush", (0.26, 0.52, 0.22))
kids = []
for i, (x, z, s) in enumerate([(0, 0, 0.55), (0.4, 0.2, 0.4), (-0.35, 0.25, 0.42)]):
    m = b.mesh("blob%d" % i, [(rock_faces(40 + i, 1.0), leaf_m, None)])
    kids.append(b.node("blob%d" % i, m, t=[x, 0, z], s=[s, s, s]))
b.node("Bush", children=kids, root=True)
b.write("bush", "embedded")

# ---------------------------------------------------------------- crate (glb, textured, nearest sampler)
b = Builder()
tex = b.png_texture(plank_texture(), nearest=True)
m = b.material("Crate", (1, 1, 1), texture=tex)
b.node("Crate", b.mesh("crate", [(box(0.8, 0.8, 0.8), m, "box")]), root=True)
b.write("crate", "glb")
