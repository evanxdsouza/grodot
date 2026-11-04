# Design decisions

Accepted choices and their rationale. Newest first within each section.

## Platform and dependencies

**D1. Zero third-party crates.** The blueprint recommended wgpu, winit, egui,
Rapier3D, Kira, glam, serde and clap. The build environment for v0.1 had no
access to crates.io, so every subsystem was written against the Rust standard
library only. This turned out to suit the project's goals:

* `cargo build` needs no system `-dev` packages, no C compiler and no network.
* The whole engine is small enough for one person (or one coding agent) to read.
* Rendering, physics and the full game loop run headless in CI.

Consequences and mitigations:

| Blueprint choice | v0.1 replacement | Notes |
| --- | --- | --- |
| wgpu | `grodot-render`: multithreaded software rasterizer | ~60 fps for jam-sized scenes at 1280x720. A GPU backend can implement the same `SceneRenderer` contract later. |
| winit | `grodot-platform::window`: X11 via `dlopen("libX11.so.6")` | Works on X11 and on Wayland through XWayland. |
| egui + egui_dock | `grodot-editor::ui`: immediate-mode widgets on the canvas | Fixed dock layout (hierarchy, viewport, inspector, console/assets). |
| Rapier3D | `grodot-runtime::physics`: boxes and spheres, fixed step | Rotation is not simulated. Enough for platformers and pickups. |
| Kira | `grodot-platform::audio`: WAV decoder + mixer over ALSA via `dlopen` | PipeWire and PulseAudio expose ALSA; missing audio is a warning. |
| glam | `grodot-core::math` | Vec3, Quat, Mat4, Color with tests. |
| serde + RON/JSON | `grodot-core::json` | Hand-written reader/writer that keeps key order. |
| clap | hand-written argument parsing | Eight commands. |
| gltf | Wavefront `.obj` loader plus primitives | Matches cut-order item 3. glTF is on the roadmap. |

**D2. Linux x86_64 only.** OS access sits behind `grodot-platform`. No Windows
or macOS code paths exist.

**D3. Libraries are loaded at runtime.** Because X11 and ALSA are `dlopen`ed,
the same binary starts on machines without a display or sound card. Headless
commands (`run --headless`, `check`, `test`, `script`) never touch them.

## Scene and project

**D4. Scene format: JSON** (`.gscene`, `.gprefab`), versioned with
`scene_version` (currently 1). Files newer than the engine are rejected.
Rotations are stored as quaternions `[x, y, z, w]`; `rotation_degrees` is also
accepted when hand-writing files. Asset paths are always project-relative.

**D5. ECS strategy: a simple entity list.** Entities hold a `Vec<Component>`
with at most one component per type. No archetypes or systems; scripts are the
behaviour layer.

**D6. Coordinates:** right-handed, +Y up, -Z forward, 1 unit = 1 metre,
Euler angles in degrees applied yaw (Y), pitch (X), roll (Z).

**D7. Play mode runs on a clone** of the editor scene. Stop discards runtime
changes. The inspector edits the live copy while playing.

**D8. Paths are project-scoped.** Scripts and the engine reject absolute paths
and `..` components for assets, scenes, prefabs and imports.

## Groscript

Answers to blueprint section 18.3:

| Question | Decision |
| --- | --- |
| Are functions first-class? | Named functions are values (`f = add; f(1, 2)`). No anonymous functions or closures in v0.1. Timers take a function value or a `:method` symbol. |
| Parentheses on calls? | Always required, even with zero arguments. No parentheses means a property. |
| Property and index assignment? | Yes: `player.hp = 5`, `items[0] = x`, `h[:k] = v`. |
| Return value without `return`? | `nil`. No implicit return. |
| Are strings immutable? | Yes. |
| Legal hash keys? | nil, booleans, numbers, strings, symbols. |
| How do modules expose definitions? | `module M` members are accessed as `M.f()` / `M.CONST`. Imports run a file once and make its classes, modules and functions global. |
| Script callback errors? | Reported with location and stack, then that script instance is disabled. |
| Create entities at runtime? | Yes: `Scene.spawn(prefab)`, `Scene.create(name)`, `entity.duplicate()`, `add_component()`. Attaching scripts at runtime is limited to prefabs. |
| Destroyed entity handles? | `entity.valid?()` returns false; any other use is an EngineError naming the entity. Destruction is deferred to the end of the frame. |

Other language choices:

* **Truthiness** follows Ruby: only `false` and `nil` are falsey.
* **One Number type** (f64). Integral numbers print without `.0`.
* **Object fields are public** (`obj.hp` reads `@hp`). This removes the need
  for getters in a beginner's first day.
* **Chained comparisons are rejected** rather than given a surprising meaning.
* **Constants resolve through the receiver's class first**, so a subclass can
  override a constant used by an inherited method.
* **Vectors and colours are immutable values**: `pos.x = 1` is an error with a
  hint to use `with_x`.
* **Execution budget** of 5,000,000 statements per engine callback.

## Editor

**D9. Undo is snapshot-based.** The editor compares the scene each frame and
records a snapshot when an edit finishes (not during a drag). 100 levels.

**D10. Scripts are edited in the user's own editor.** The Script inspector and
the console open files with `$VISUAL` (with `+line`) or `xdg-open`.

## Rendering and assets

**D11. glTF skinning runs on the CPU.** Each frame, an entity with a Skeleton
gets its mesh skinned into a scratch vertex buffer (linear blend skinning, up
to 4 joint influences per vertex) before rasterization. Joint matrices are
`joint world × inverse bind`, computed once per entity per frame.

Performance notes:

* The cost is about 4 matrix-vector products per vertex, for positions and
  normals. The demo hero has 336 vertices, so skinning is a few microseconds,
  far below rasterization cost.
* The skinned buffer is rebuilt every frame and is not cached between frames,
  even when the pose has not changed.
* Characters of around 10k vertices are still cheap next to raster time. GPU
  skinning would only pay off together with a GPU rasterizer.

Animation sampling supports LINEAR (with slerp for rotations), STEP and
CUBICSPLINE. Crossfades blend the outgoing pose into the new clip; the default
is 0.2 s.

**D12. Lights: directional, point and spot, with shadows for suns.** A `Light`
component has a `light_type` (directional, point or spot), `color`,
`intensity`, `range` (point and spot), `spot_angle` (spot) and `shadows`.
Only directional lights cast shadows. Point and spot shadows are not done yet.
At most two shadowed suns are rendered per frame; any extra sun is lit without
shadows.

**D13. Shadow maps are orthographic depth passes.** Each frame, every shadowed
sun gets a shadow map sized to the part of the view frustum within 40 m of the
camera (`shadow_distance`). The map is fitted to the frustum slice's bounding
sphere, and its centre is snapped to whole texels so edges do not shimmer when
the camera moves. The depth pass reuses the main rasterizer's setup and
depth-test code, without culling, so open or thin meshes cast shadows too.
Lookups use a 3x3 bilinear-weighted PCF (16 depth taps), with a normal-offset
and a depth bias that grows at grazing angles to avoid acne.

The blob shadows are gone. Shadows now come from real geometry, so they land
on the right surfaces (roofs, platforms) instead of on the ground below.

**D14. Per-pixel versus per-vertex lighting: per-pixel is the default.**
Benchmark: `cargo test --release -p grodot-editor --test bench_lighting --
--ignored --nocapture`, examples/orb-garden at 1280x720, 60 frames after a
warm-up. Machine: 4 vCPUs (Intel Xeon 2.10 GHz), software renderer, no GPU.

| Configuration                | ms/frame | fps  |
|------------------------------|---------:|-----:|
| per-vertex, no shadows       |    25.91 | 38.6 |
| per-vertex, shadows 1024     |    52.95 | 18.9 |
| per-pixel, no shadows        |    24.43 | 40.9 |
| per-pixel, shadows 512       |    47.03 | 21.3 |
| per-pixel, shadows 1024      |    49.29 | 20.3 |
| per-pixel, shadows 2048      |    64.95 | 15.4 |

Per-vertex lighting did not pay off. The gather is cheap per vertex, but
shading still runs per pixel, and with many small triangles the vertex
interpolation saves little. The cost of per-pixel lighting is about 1 ms, and
it removes the Gouraud look on point lights and spot cones. The `PerVertex`
mode stays available for slow machines, but it is not the default.

Shadow cost is the real cost: roughly +25 ms at 1024. Most of it is the PCF
lookup, which runs once for every lit pixel (16 taps), and the depth pass for
all casters. The map size matters less than the number of lit pixels. Ways
to cut it, if needed: a 2x2 PCF (4 taps), a smaller `shadow_distance`, or
shadows only on the nearest sun. 1024 is kept as the default, following the
starting point. Setting `shadows` to false on the sun turns the cost off
completely.

**D15. Emissive surfaces.** A Material has `emissive` (a colour) and
`emissive_texture`. The glow is added after lighting, so emissive surfaces
stay bright in shadow. glTF `emissiveFactor` and `emissiveTexture` are read
too.
