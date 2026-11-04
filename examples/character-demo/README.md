# Character Demo

A rigged, skinned glTF character driven by Orb Garden-style input.

- **WASD / arrows**: run (the `run` clip)
- **Space**: jump (the one-shot `jump` clip, crossfade 0.1 s)
- **E**: wave (the one-shot `wave` clip)
- Standing still plays `idle`. Clips crossfade over the AnimationPlayer's
  `crossfade` time (0.2 s).

`scripts/hero.gro` picks the clips with `self.animation.play(...)` and uses
`on_animation_finished(clip)` to know when a one-shot clip has ended.

## Assets

`assets/models/hero.glb` is an original CC0 model made by
`tools/gen_character.py`. It has 11 joints, up to 2 influences per vertex at
the knees, elbows and waist, and four clips: `idle` (LINEAR rotations plus
CUBICSPLINE hips), `run`, `jump` (STEP keys on the head) and `wave`.
Run `python3 tools/gen_character.py` to regenerate it.

Skinning runs on the CPU every frame before rasterization. See
`docs/DECISIONS.md` for the performance notes.
