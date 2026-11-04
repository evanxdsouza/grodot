# Roadmap

## v0.1 milestones (blueprint section 16)

| Milestone | Done when | Status |
| --- | --- | --- |
| M1 Linux foundation | App launches and exits on the Linux desktop; CI passes | Done. X11 window via dlopen, clean shutdown, CI workflow. |
| M2 Renderer | A camera renders a coloured 3D mesh with correct depth and resize | Done. Software rasterizer with depth, near-plane clipping, resize. |
| M3 Groscript v0.1 | Lexer/parser/interpreter tests pass; errors show file and line | Done. 27 unit tests, 22 golden tests, 7 tutorial tests. |
| M4 Scripted scene | A script changes an entity transform at runtime | Done. |
| M5 Input and physics | A player responds to input; a collider event reaches a script | Done (tested headlessly). |
| M6 Persistence | Scene saves and reloads with entity/component data intact | Done, bit-exact round trip. |
| M7 Editor | Select entity, edit transform, save scene, Play and Stop | Done. |
| M8 Jam build | A clean Linux release package runs the complete sample game | Done: `grodot package examples/orb-garden`. |

### Minimum end-to-end demo (16.1)

1. Launch the editor: `grodot open examples/orb-garden`
2. Create or open a project: `grodot new my-game`
3. Open the main scene: opened automatically, or double-click it in Assets.
4. Select the player and inspect its Transform and Script.
5. Press Play and move with WASD.
6. Touch an orb: the score changes and a sound plays.
7. Stop, save (Ctrl+S) and reopen.
8. Run without the editor: `grodot run examples/orb-garden`, or the packaged `run.sh`.

## Deferred (in cut order)

* Textures and UV mapping in the rasterizer, including texture-backed sprites.
* 2D physics bodies, collision shapes, and tile maps.
* GDScript signal declarations, full NodePath features (unique names and exported paths), and syntax such as `match`, enums, annotations, and typed declarations.
* glTF 2.0 model import (OBJ and primitives work today).
* Rotate and scale gizmos, multi-select, copy/paste between scenes.
* Gamepad input.
* Script hot reload while playing.
* Spatial audio, OGG decoding, a mixer graph.
* Applying Play-mode changes back to the editor scene.
* A GPU renderer behind the same `SceneRenderer` interface.
* Closures and anonymous functions in Groscript (v0.2 candidate).
* A browser playground for Groscript (WebAssembly build of `crates/groscript`).

## Out of scope for the first release

Windows and macOS builds, networking, a plugin marketplace, visual shader,
animation or terrain editors.
