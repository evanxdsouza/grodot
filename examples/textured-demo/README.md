# Textured demo

Shows Grodot's textured rendering:

* **Ground**: a tiling grass texture (`uv_scale` 12 x 12) with trilinear mipmaps,
  so it stays smooth in the distance instead of shimmering.
* **Crate**: a textured, spinning cube.
* **Ferns**: alpha-cutout quads (`alpha_mode: cutout`, double-sided). Cut-away
  pixels are discarded, so they do not hide what is behind them.
* **Pond**: an alpha-blended water plane whose `uv_offset` is animated by
  `scripts/water.gro`.
* **Pillar**: press **T** to swap its texture from a script; press **Y** to try a
  missing texture and see the error point at the script line.

All textures were painted procedurally by `tools/gen_textures.py` and are CC0.

```sh
grodot run examples/textured-demo
```
