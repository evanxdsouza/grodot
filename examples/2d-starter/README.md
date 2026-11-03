# 2D Starter

A small, editable 2D scene showing `Camera2D` and `Sprite2D` with colored,
alpha-blended rectangles. Run it with `grodot run examples/2d-starter`, or open
it in the editor with `grodot open examples/2d-starter`.

Positions, rotation, scale, and parenting use the existing Transform component
in the XY plane. `Camera2D.zoom` scales the view, `pixels_per_unit` controls the
world-to-screen scale, and `Sprite2D.z_index` controls draw order. Texture
assets and 2D physics are not part of this first 2D slice.
