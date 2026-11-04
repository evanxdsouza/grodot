# Particles Demo

Two torches with flame and spark emitters, and a crate with an explosion
emitter.

- **E** (or **B** on a gamepad): set off the explosion, a burst of 120 particles.
- The HUD shows how many explosion particles are alive.

## How it works

* `ParticleEmitter` is a component: `rate`, `max_particles`, `lifetime`,
  `lifetime_random`, `speed`, `spread` (cone degrees), `gravity`, `size_start`
  and `size_end`, `color_start` and `color_end` (alpha is opacity), and an
  optional `texture`. Particles fly in the emitter's +Y direction.
* `scripts/bomb.gro` calls `emit_burst(120)` and `count()` on the emitter from
  `get_component("ParticleEmitter")`. `start()` and `stop()` switch emission.
* Particles are simulated on the CPU and drawn as camera-facing quads, sorted
  back to front with the other transparent geometry.

## Assets

`assets/textures/flame.png` and `spark.png` are original CC0 sprites, and
`grass.png` is copied from `examples/textured-demo`. Regenerate everything with
`python3 tools/gen_particles_demo.py`.
