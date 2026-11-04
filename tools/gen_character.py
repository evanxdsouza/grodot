#!/usr/bin/env python3
"""Generate Grodot's rigged sample character (CC0) as a skinned glTF 2.0 .glb.

Usage: python3 tools/gen_character.py <output-dir>

A blocky low-poly hero with an 11-joint skeleton and one skinned mesh.
Vertices near joints (knees, elbows, waist, hips) blend two joints so the
skinning code has real multi-influence work to do. Clips:

  idle  (2.0 s, loop)  LINEAR rotations + CUBICSPLINE breathing on the hips
  run   (0.6 s, loop)  LINEAR leg/arm swing and hip bob
  jump  (0.8 s, once)  LINEAR crouch-and-reach, plus a STEP head channel
  wave  (1.2 s, loop)  LINEAR right-arm wave

The character faces -Z, which is Grodot's forward direction. Released as CC0.
"""
import json
import math
import os
import struct
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(OUT, exist_ok=True)

# name, parent, local translation (bind pose rotations are identity)
JOINTS = [
    ("hips", None, (0.0, 0.95, 0.0)),
    ("spine", 0, (0.0, 0.12, 0.0)),
    ("head", 1, (0.0, 0.48, 0.0)),
    ("arm_l", 1, (0.27, 0.40, 0.0)),
    ("forearm_l", 3, (0.0, -0.30, 0.0)),
    ("arm_r", 1, (-0.27, 0.40, 0.0)),
    ("forearm_r", 5, (0.0, -0.30, 0.0)),
    ("thigh_l", 0, (0.11, -0.05, 0.0)),
    ("shin_l", 7, (0.0, -0.42, 0.0)),
    ("thigh_r", 0, (-0.11, -0.05, 0.0)),
    ("shin_r", 9, (0.0, -0.42, 0.0)),
]
J = {name: i for i, (name, _, _) in enumerate(JOINTS)}


def world_pos(i):
    x = y = z = 0.0
    while i is not None:
        _, p, t = JOINTS[i]
        x, y, z = x + t[0], y + t[1], z + t[2]
        i = p
    return (x, y, z)


def box(x0, x1, y0, y1, z0, z1, weight_fn):
    """Faces of an axis-aligned box. weight_fn(x, y, z) -> [(joint, weight), ...]."""
    v = lambda x, y, z: (x, y, z)
    faces = [
        ([v(x0, y0, z1), v(x1, y0, z1), v(x1, y1, z1), v(x0, y1, z1)], (0, 0, 1)),
        ([v(x1, y0, z0), v(x0, y0, z0), v(x0, y1, z0), v(x1, y1, z0)], (0, 0, -1)),
        ([v(x1, y0, z1), v(x1, y0, z0), v(x1, y1, z0), v(x1, y1, z1)], (1, 0, 0)),
        ([v(x0, y0, z0), v(x0, y0, z1), v(x0, y1, z1), v(x0, y1, z0)], (-1, 0, 0)),
        ([v(x0, y1, z1), v(x1, y1, z1), v(x1, y1, z0), v(x0, y1, z0)], (0, 1, 0)),
        ([v(x0, y0, z0), v(x1, y0, z0), v(x1, y0, z1), v(x0, y0, z1)], (0, -1, 0)),
    ]
    return [(f, n, weight_fn) for f, n in faces]


def rigid(j):
    return lambda x, y, z: [(j, 1.0)]


def blend_top(top_y, lower, upper, w_upper):
    """Vertices at the top edge blend into the parent joint."""
    return lambda x, y, z: [(lower, 1.0 - w_upper), (upper, w_upper)] if abs(y - top_y) < 1e-6 else [(lower, 1.0)]


SKIN, SHIRT, PANTS, VISOR, SHOES = range(5)
PARTS = []  # (material, faces)


def add(mat, faces):
    PARTS.append((mat, faces))


add(PANTS, box(-0.18, 0.18, 0.85, 1.05, -0.11, 0.11, rigid(J["hips"])))
add(SHIRT, box(-0.20, 0.20, 1.05, 1.50, -0.12, 0.12, blend_top(1.05, J["spine"], J["hips"], 0.0)
              if False else (lambda x, y, z: [(J["spine"], 0.6), (J["hips"], 0.4)] if abs(y - 1.05) < 1e-6 else [(J["spine"], 1.0)])))
add(SKIN, box(-0.13, 0.13, 1.55, 1.85, -0.13, 0.13, rigid(J["head"])))
add(VISOR, box(-0.10, 0.10, 1.66, 1.75, -0.17, -0.13, rigid(J["head"])))
for side, sx in (("l", 1), ("r", -1)):
    x0, x1 = sorted((sx * 0.22, sx * 0.32))
    arm, fore = J["arm_" + side], J["forearm_" + side]
    add(SHIRT, box(x0, x1, 1.17, 1.47, -0.05, 0.05, rigid(arm)))
    add(SKIN, box(x0, x1, 0.92, 1.17, -0.045, 0.045, blend_top(1.17, fore, arm, 0.5)))
    lx0, lx1 = sorted((sx * 0.04, sx * 0.18))
    thigh, shin = J["thigh_" + side], J["shin_" + side]
    add(PANTS, box(lx0, lx1, 0.48, 0.90, -0.07, 0.07, blend_top(0.90, thigh, J["hips"], 0.3)))
    add(PANTS, box(lx0, lx1, 0.08, 0.48, -0.065, 0.065, blend_top(0.48, shin, thigh, 0.5)))
    add(SHOES, box(lx0, lx1, 0.0, 0.08, -0.16, 0.07, rigid(shin)))

MATERIALS = [
    ("Skin", [0.95, 0.76, 0.60, 1.0]),
    ("Shirt", [0.24, 0.48, 0.85, 1.0]),
    ("Pants", [0.22, 0.22, 0.30, 1.0]),
    ("Visor", [0.10, 0.12, 0.16, 1.0]),
    ("Shoes", [0.55, 0.30, 0.18, 1.0]),
]

bin_ = bytearray()
views, accessors = [], []


def view(data, target=None):
    while len(bin_) % 4:
        bin_.append(0)
    off = len(bin_)
    bin_.extend(data)
    v = {"buffer": 0, "byteOffset": off, "byteLength": len(data)}
    if target:
        v["target"] = target
    views.append(v)
    return len(views) - 1


def accessor(fmt, values, ctype, kind, count, minmax=None):
    i = view(struct.pack("<%d%s" % (len(values), fmt), *values))
    a = {"bufferView": i, "componentType": ctype, "count": count, "type": kind}
    if minmax:
        a["min"], a["max"] = minmax
    accessors.append(a)
    return len(accessors) - 1


# Build one primitive per material.
primitives = []
for mat in range(len(MATERIALS)):
    pos, nrm, joints, weights, idx = [], [], [], [], []
    for m, faces in PARTS:
        if m != mat:
            continue
        for quad, n, wf in faces:
            base = len(pos) // 3
            for (x, y, z) in quad:
                pos += [x, y, z]
                nrm += list(n)
                infl = wf(x, y, z) + [(0, 0.0)] * 4
                joints += [infl[k][0] for k in range(4)]
                weights += [infl[k][1] for k in range(4)]
            idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    if not idx:
        continue
    n = len(pos) // 3
    xs, ys, zs = pos[0::3], pos[1::3], pos[2::3]
    primitives.append({
        "attributes": {
            "POSITION": accessor("f", pos, 5126, "VEC3", n, ([min(xs), min(ys), min(zs)], [max(xs), max(ys), max(zs)])),
            "NORMAL": accessor("f", nrm, 5126, "VEC3", n),
            "JOINTS_0": accessor("H", joints, 5123, "VEC4", n),
            "WEIGHTS_0": accessor("f", weights, 5126, "VEC4", n),
        },
        "indices": accessor("H", idx, 5123, "SCALAR", len(idx)),
        "material": mat,
    })

# Inverse bind matrices: bind rotations are identity, so each is a translation by -world position.
ibm = []
for i in range(len(JOINTS)):
    x, y, z = world_pos(i)
    ibm += [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, -y, -z, 1]
ibm_acc = accessor("f", ibm, 5126, "MAT4", len(JOINTS))

nodes = []
for i, (name, parent, t) in enumerate(JOINTS):
    node = {"name": name, "translation": list(t)}
    kids = [k for k, (_, p, _) in enumerate(JOINTS) if p == i]
    if kids:
        node["children"] = kids
    nodes.append(node)
nodes.append({"name": "Body", "mesh": 0, "skin": 0})


def qaxis(axis, deg):
    a = math.radians(deg) / 2
    s = math.sin(a)
    return [axis[0] * s, axis[1] * s, axis[2] * s, math.cos(a)]


X, Z = (1, 0, 0), (0, 0, 1)
samplers_all = []


def clip(name, channels):
    """channels: list of (joint, path, interpolation, times, values-per-key)."""
    samplers, chans = [], []
    for joint, path, interp, times, values in channels:
        flat = [c for v in values for c in v]
        inp = accessor("f", times, 5126, "SCALAR", len(times), ([min(times)], [max(times)]))
        kind = "VEC4" if path == "rotation" else "VEC3"
        out = accessor("f", flat, 5126, kind, len(values))
        samplers.append({"input": inp, "output": out, "interpolation": interp})
        chans.append({"sampler": len(samplers) - 1, "target": {"node": J[joint], "path": path}})
    return {"name": name, "samplers": samplers, "channels": chans}


def rot(joint, axis, times, degs, interp="LINEAR"):
    return (joint, "rotation", interp, times, [qaxis(axis, d) for d in degs])


def hips_y(times, ys, interp="LINEAR"):
    return ("hips", "translation", interp, times, [[0.0, y, 0.0] for y in ys])


animations = []
# idle: breathing on the hips uses CUBICSPLINE (in-tangent, value, out-tangent per key).
idle_t = [0.0, 1.0, 2.0]
spline = [[0, 0, 0], [0, 0.95, 0], [0, 0, 0], [0, 0, 0], [0, 0.935, 0], [0, 0, 0], [0, 0, 0], [0, 0.95, 0], [0, 0, 0]]
animations.append(clip("idle", [
    ("hips", "translation", "CUBICSPLINE", idle_t, spline),
    rot("spine", X, idle_t, [0, 3, 0]),
    rot("arm_l", Z, idle_t, [4, 7, 4]),
    rot("arm_r", Z, idle_t, [-4, -7, -4]),
    rot("head", X, idle_t, [0, -3, 0]),
]))
run_t = [0.0, 0.15, 0.3, 0.45, 0.6]
animations.append(clip("run", [
    hips_y(run_t, [0.93, 0.97, 0.93, 0.97, 0.93]),
    rot("spine", X, run_t, [8, 10, 8, 10, 8]),
    rot("thigh_l", X, run_t, [35, 0, -35, 0, 35]),
    rot("thigh_r", X, run_t, [-35, 0, 35, 0, -35]),
    rot("shin_l", X, run_t, [-10, -60, -15, -25, -10]),
    rot("shin_r", X, run_t, [-15, -25, -10, -60, -15]),
    rot("arm_l", X, run_t, [-30, 0, 30, 0, -30]),
    rot("arm_r", X, run_t, [30, 0, -30, 0, 30]),
    rot("forearm_l", X, run_t, [45, 50, 45, 50, 45]),
    rot("forearm_r", X, run_t, [45, 50, 45, 50, 45]),
]))
jump_t = [0.0, 0.15, 0.35, 0.8]
animations.append(clip("jump", [
    hips_y(jump_t, [0.95, 0.80, 0.98, 0.95]),
    rot("thigh_l", X, jump_t, [0, 50, -10, 0]),
    rot("thigh_r", X, jump_t, [0, 50, -10, 0]),
    rot("shin_l", X, jump_t, [0, -85, -5, 0]),
    rot("shin_r", X, jump_t, [0, -85, -5, 0]),
    rot("arm_l", X, jump_t, [0, -40, 165, 0]),
    rot("arm_r", X, jump_t, [0, -40, 165, 0]),
    # STEP: the head snaps to look down while crouching and up while reaching.
    rot("head", X, [0.0, 0.1, 0.35, 0.7], [0, 12, -15, 0], "STEP"),
]))
wave_t = [0.0, 0.3, 0.6, 0.9, 1.2]
animations.append(clip("wave", [
    rot("arm_r", Z, wave_t, [-150, -150, -150, -150, -150]),
    rot("forearm_r", Z, wave_t, [0, 30, 0, 30, 0]),
    rot("head", Z, wave_t, [0, -5, 0, -5, 0]),
]))

doc = {
    "asset": {"version": "2.0", "generator": "grodot tools/gen_character.py", "copyright": "CC0"},
    "scene": 0,
    "scenes": [{"nodes": [0, len(nodes) - 1]}],
    "nodes": nodes,
    "meshes": [{"name": "Body", "primitives": primitives}],
    "skins": [{"name": "Rig", "joints": list(range(len(JOINTS))), "inverseBindMatrices": ibm_acc, "skeleton": 0}],
    "materials": [{"name": n, "pbrMetallicRoughness": {"baseColorFactor": c, "metallicFactor": 0, "roughnessFactor": 0.9}}
                  for n, c in MATERIALS],
    "animations": animations,
    "accessors": accessors,
    "bufferViews": views,
}
while len(bin_) % 4:
    bin_.append(0)
doc["buffers"] = [{"byteLength": len(bin_)}]
js = json.dumps(doc, separators=(",", ":")).encode()
while len(js) % 4:
    js += b" "
glb = b"glTF" + struct.pack("<II", 2, 12 + 8 + len(js) + 8 + len(bin_))
glb += struct.pack("<II", len(js), 0x4E4F534A) + js + struct.pack("<II", len(bin_), 0x004E4942) + bytes(bin_)
path = os.path.join(OUT, "hero.glb")
open(path, "wb").write(glb)
print("wrote", path, len(glb), "bytes,", len(JOINTS), "joints,", len(animations), "clips")
