# Contributing to Grodot

Thanks for helping! Grodot stays small on purpose, so the bar for a change is:
it works on Linux, it has tests, and it keeps the build dependency-free.

## Development loop

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -p grodot-cli -- test tests/language
cargo run -p grodot-cli -- test docs/examples
cargo run -p grodot-cli -- run examples/orb-garden --headless --frames 600
```

To try the editor or game without a desktop session, use Xvfb:

```sh
Xvfb :99 -screen 0 1440x900x24 &
DISPLAY=:99 cargo run -p grodot-cli -- open examples/orb-garden
```

Headless screenshots are handy for reviewing UI changes:

```sh
cargo run -p grodot-cli -- open examples/orb-garden --screenshot editor.png --select Player
cargo run -p grodot-cli -- run examples/orb-garden --headless --frames 120 --screenshot game.png
```

## Rules of thumb

* **No new crates.** The engine depends only on `std`. If you need a system
  library, load it at runtime in `grodot-platform` (see `dl.rs`).
* **Groscript changes need tests.** Add a golden test to `tests/language`
  (`NAME.gro` + `NAME.out`; files ending in `_error` must fail and their `.out`
  lists lines that must appear in the error report). `BLESS=1 cargo test -p
  groscript --test golden` regenerates expected output; review the diff.
* **Keep the language learnable in a day.** New syntax needs a strong reason
  and an update to `docs/SPEC.md`, `website/docs.html` and, if it is
  beginner-facing, a lesson in `docs/examples`.
* **Engine API changes** go through `crates/grodot-runtime/src/bindings.rs`.
  Never expose renderer or physics internals to scripts.
* **Record decisions** in `docs/DECISIONS.md` and progress in `ROADMAP.md`.
* Keep the main branch compiling and the sample game playable.

## Regenerating generated assets

* Fonts: `python3 tools/gen_font.py` (needs Pillow and the Inter font).
* Sample sounds: `python3 tools/gen_sounds.py examples/orb-garden/assets/audio`.
