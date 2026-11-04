# Grodot feature parity handover

## Objective

Continue moving Grodot toward Godot and GDScript feature parity. Treat parity as
a long-term compatibility goal, not a claim that the current engine is close to
complete. Before adding broad features, maintain a feature matrix that records
Godot behavior, Grodot behavior, gaps, and compatibility decisions.

## Repository state

- Repository: `evanxdsouza/grodot`
- Current branch: `ccr-781408d1-r0cg41`
- Last pushed commit: `c03bac1` (`Format runtime signal bindings`)
- The working tree was clean at handover.
- Latest GitHub Actions run passed: CI run `41823908247`.
- Local verification passed: `cargo test --workspace --all-targets` and
  `cargo build --release`.
- The local machine has Rust 1.85 and does not have `cargo fmt` installed.
  GitHub CI uses Rust stable and runs formatting, Clippy, tests, and smoke
  checks. Use CI to confirm formatting and current-stable lints.

## Recent features

- Basic 2D: `Camera2D` with zoom and pixels-per-unit, plus colored,
  alpha-blended rectangular `Sprite2D` drawing with transformed rotation,
  scale, z ordering, editor creation, and selection.
- Example project: `examples/2d-starter`.
- Node groups: group membership is stored in scene JSON, editable in the
  inspector, and exposed as `add_to_group`, `remove_from_group`,
  `is_in_group?`, `get_groups`, and `Scene.get_nodes_in_group`.
- Node helpers: `get_parent`, relative and root-based `get_node` paths,
  `get_node_or_null`, and `queue_free`.
- Runtime signals: `connect(signal, target, method)`, `disconnect`,
  `is_connected?`, and `emit_signal(signal, args...)`. Connections are unique
  per source/signal/target/method. Signals are dynamically named; there is no
  `signal` declaration syntax in Groscript yet. Events emitted during `ready`
  are dispatched after all ready callbacks. Events from frame callbacks are
  delivered after updates and timers, before deferred destruction.

## Important gaps

The new features are small Godot-inspired slices; they do not provide parity.
Current documented gaps include:

- GDScript language compatibility: `match`, enums, annotations, typed
  declarations, closures/lambdas, and declared signals.
- 2D textures, tile maps, 2D collision/physics, and a 2D input/game loop.
- Full NodePath behavior, such as unique-name paths and exported paths.
- Godot's node/resource model, scene inheritance, animation system, editor
  workflows, and platform coverage.
- The renderer is a CPU software renderer; textures, glTF, and a GPU backend
  are absent.

See `ROADMAP.md`, `docs/SPEC.md`, and `website/docs.html` before extending an
existing subsystem. Update these docs when behavior changes.

## Architecture map

- `crates/groscript`: lexer, parser, AST, interpreter, and language values.
- `crates/grodot-core`: scene/entity/component model, scene JSON, transforms,
  math, project config, and meshes.
- `crates/grodot-render`: canvas, software rasterizer, scene rendering, 2D
  sprite pass, and picking.
- `crates/grodot-runtime`: `World`, physics, script bindings, lifecycle,
  timers, signals, and deferred operations.
- `crates/grodot-editor`: immediate-mode editor and player.
- `crates/grodot-cli`: `new`, `open`, `run`, `check`, `test`, and packaging.

Grodot intentionally has no third-party Cargo dependencies. Platform services
use `dlopen`; preserve headless operation and avoid adding dependencies without
revisiting `docs/DECISIONS.md`.

## Suggested next steps

1. Create a parity matrix from the Godot documentation and the current
   `docs/SPEC.md`/engine API. Separate exact syntax compatibility from
   Godot-inspired API names.
2. Pick one bounded, high-value vertical slice and implement it through parser
   or scene model, runtime/editor integration, docs, and an example.
3. Good candidates are Groscript `enum` plus `match`, declared signals with
   arity checking, or image textures and a textured `Sprite2D` pipeline.
   Avoid attempting all GDScript syntax in one patch.
4. Add regression tests for the behavior and update the docs and roadmap.
5. Run:

   ```sh
   cargo test --workspace --all-targets
   cargo build --release
   ```

   Also run the relevant CLI example and check the GitHub Actions run for
   formatting, Clippy, and platform smoke tests.

## Current examples and checks

```sh
cargo run -p grodot-cli -- check examples/2d-starter
cargo run -p grodot-cli -- run examples/2d-starter --headless --frames 1
cargo run -p grodot-cli -- run examples/orb-garden --headless --frames 600
```

The current 2D starter uses flat colored rectangles. It is a renderer and scene
foundation, not a complete 2D game stack.
