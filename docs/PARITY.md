# Godot / GDScript parity matrix

Parity is a long-term goal. Groscript is Ruby-flavoured, so "parity" means
equivalent capability, not identical syntax, unless stated. Status: **Done**,
**Partial**, **Missing**, **Won't** (deliberate divergence).

## Language

| Godot / GDScript | Grodot | Status | Notes |
| --- | --- | --- | --- |
| `if/elif/else`, `while`, `for` | `if/elsif/else`, `while`, `for` | Done | Syntax differs by design. |
| `match` (value patterns) | `match`/`when`/`else` | Partial | No binding, array/dict patterns, or guards. |
| `enum` | `enum Name ... end` | Partial | Numeric members only; no anonymous enums, no `keys()`/`values()`. |
| `signal` declarations | dynamic `connect`/`emit_signal` | Partial | No declaration or arity check. |
| Lambdas / closures, `Callable` | none | Missing | Roadmap v0.2 candidate. |
| Static typing (`var x: int`) | none | Missing | |
| Annotations (`@export`, `@onready`) | none | Missing | Inspector fields come from scene JSON. |
| `class_name`, inner classes, `extends` | `class A < B` | Partial | Single inheritance; no class methods or static. |
| `await` / coroutines | none | Missing | |
| String formatting (`%`), `format()` | interpolation only | Partial | |
| Ranges, `in` operator | `range()` function | Partial | |

## Engine

| Area | Status | Notes |
| --- | --- | --- |
| Scene tree, node groups, `get_node`, `queue_free` | Partial | No unique-name or exported NodePaths. |
| Scene inheritance / instancing | Missing | |
| Resources and `.tres` | Missing | |
| 2D: Camera2D, coloured Sprite2D | Partial | No textures, tile maps, or 2D physics. |
| 3D: meshes, software rasterizer | Partial | No glTF, materials beyond flat colour, or GPU backend. |
| Physics (3D bodies/colliders) | Partial | |
| Input | Partial | Keyboard/mouse; no gamepad or action map editor. |
| Audio | Partial | WAV only. |
| Animation (AnimationPlayer, tweens) | Missing | |
| UI/Control nodes | Missing | |
| Editor | Partial | Move gizmo only; no multi-select. |
| Export platforms | Partial | Linux only. |

Update this file whenever a row changes.
