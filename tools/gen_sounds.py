#!/usr/bin/env python3
"""Generate the small chiptune-style sound effects used by the examples.

Usage: python3 tools/gen_sounds.py <output-dir>
All sounds are synthesized here (no third-party samples), 22050 Hz mono 16-bit WAV.
"""
import math, random, struct, sys, os, wave

RATE = 22050
random.seed(7)

def write(path, samples):
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32000)) for s in samples))

def env(i, n, attack=0.01, release=0.3):
    t = i / RATE
    a = min(1.0, t / attack) if attack > 0 else 1.0
    r = min(1.0, (n - i) / (release * RATE)) if release > 0 else 1.0
    return a * max(0.0, r)

def square(phase):
    return 1.0 if (phase % 1.0) < 0.5 else -1.0

def tone(freqs, dur, kind="square", vol=0.35, release=0.15):
    n = int(dur * RATE)
    out = []
    phase = 0.0
    for i in range(n):
        f = freqs(i / n)
        phase += f / RATE
        s = square(phase) if kind == "square" else math.sin(2 * math.pi * phase)
        out.append(s * vol * env(i, n, 0.005, release))
    return out

def seq(notes, step, kind="square", vol=0.3):
    out = []
    for f in notes:
        out += tone(lambda t, f=f: f, step, kind, vol, release=step * 0.6)
    return out

def noise(dur, vol=0.4):
    n = int(dur * RATE)
    return [(random.random() * 2 - 1) * vol * env(i, n, 0.002, dur * 0.9) for i in range(n)]

out = sys.argv[1] if len(sys.argv) > 1 else "."
os.makedirs(out, exist_ok=True)
write(os.path.join(out, "pickup.wav"), seq([988, 1319, 1760], 0.06))
write(os.path.join(out, "jump.wav"), tone(lambda t: 300 + 500 * t, 0.16, vol=0.25))
write(os.path.join(out, "hurt.wav"), [a * 0.7 + b for a, b in zip(noise(0.25, 0.35), tone(lambda t: 220 - 120 * t, 0.25, vol=0.25))])
write(os.path.join(out, "win.wav"), seq([523, 659, 784, 1047, 784, 1047], 0.11))
write(os.path.join(out, "lose.wav"), seq([392, 330, 262, 196], 0.18, vol=0.28))
# A short looping background tune (sine bass + soft square lead).
bass = [131, 131, 175, 175, 147, 147, 196, 196]
lead = [523, 659, 784, 659, 698, 880, 1047, 880, 587, 698, 880, 698, 784, 988, 1175, 988]
step = 0.2
music = []
for i, f in enumerate(lead):
    b = bass[(i // 2) % len(bass)]
    l = tone(lambda t, f=f: f, step, "square", 0.08, release=step * 0.5)
    s = tone(lambda t, b=b: b, step, "sine", 0.22, release=0.02)
    music += [x + y for x, y in zip(l, s)]
write(os.path.join(out, "music.wav"), music)
print("wrote sounds to", out)
