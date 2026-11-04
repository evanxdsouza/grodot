//! Golden-image tests: the software renderer is deterministic, so a scene
//! rendered after a fixed number of fixed-length frames always produces the
//! same pixels. Each test hashes the framebuffer and compares it with
//! `tests/golden/<name>.hash`.
//!
//! * `BLESS=1 cargo test -p grodot-editor --test golden_images` rewrites the hashes.
//! * On a mismatch the actual frame is saved to `target/golden/<name>.png` so
//!   you can look at what changed.

use grodot_core::config::Project;
use grodot_render::canvas::Canvas;
use grodot_render::scene_render::SceneRenderer;
use grodot_runtime::Runtime;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fnv64(c: &Canvas) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for px in &c.color {
        for b in px.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// Play `example` for `frames` frames at 60 fps, then render a `w` x `h` frame.
pub fn render_example(example: &str, frames: u32, w: usize, h: usize) -> Canvas {
    let project = Project::open(&root().join("examples").join(example)).unwrap();
    let mut rt = Runtime::from_project(&project).unwrap();
    rt.start();
    for _ in 0..frames {
        rt.frame(1.0 / 60.0);
    }
    let errors: Vec<String> = rt
        .world
        .borrow()
        .log
        .entries
        .iter()
        .filter(|e| e.level == grodot_runtime::LogLevel::Error)
        .map(|e| e.message.clone())
        .collect();
    assert!(errors.is_empty(), "script errors in {}:\n{}", example, errors.join("\n"));
    let mut canvas = Canvas::new(w, h);
    let mut renderer = SceneRenderer::new(Some(project.root.clone()));
    grodot_editor::player::render_game(&mut canvas, &mut renderer, &rt);
    assert!(renderer.warnings.is_empty(), "asset warnings: {:?}", renderer.warnings);
    canvas
}

pub fn check_golden(name: &str, canvas: &Canvas) {
    let golden = root().join("crates/grodot-editor/tests/golden").join(format!("{}.hash", name));
    let actual = format!("{:016x}", fnv64(canvas));
    if std::env::var("BLESS").is_ok() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, format!("{}\n", actual)).unwrap();
        let out = root().join("target/golden");
        std::fs::create_dir_all(&out).unwrap();
        canvas.save_png(&out.join(format!("{}.png", name))).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&golden).unwrap_or_default();
    if expected.trim() != actual {
        let out = root().join("target/golden");
        std::fs::create_dir_all(&out).unwrap();
        let png = out.join(format!("{}.png", name));
        canvas.save_png(&png).unwrap();
        panic!(
            "golden image '{}' changed: expected {}, got {}.\nThe new frame is at {}.\nIf the change is intended, rerun with BLESS=1.",
            name,
            expected.trim(),
            actual,
            png.display()
        );
    }
}

#[test]
fn textured_demo_matches_golden_image() {
    // 45 frames: the crate has turned and the water texture has scrolled.
    let canvas = render_example("textured-demo", 45, 640, 360);
    check_golden("textured-demo", &canvas);
}

#[test]
fn rendering_is_deterministic_across_runs() {
    let a = render_example("textured-demo", 10, 320, 180);
    let b = render_example("textured-demo", 10, 320, 180);
    assert_eq!(fnv64(&a), fnv64(&b));
}

#[test]
fn gltf_village_matches_golden_image() {
    let canvas = render_example("gltf-demo", 30, 640, 360);
    check_golden("gltf-demo", &canvas);
}

#[test]
fn skinned_character_matches_golden_image() {
    // 40 frames into the idle clip: the CUBICSPLINE breathing and arm sway are mid-cycle.
    let canvas = render_example("character-demo", 40, 640, 360);
    check_golden("character-demo", &canvas);
}

#[test]
fn lighting_demo_matches_golden_image() {
    // 30 frames: the orbiting point light has moved, the candle has flickered.
    let canvas = render_example("lighting-demo", 30, 640, 360);
    check_golden("lighting-demo", &canvas);
}

#[test]
fn particles_demo_matches_golden_image() {
    // 40 frames: the flames and sparks are in steady state (the explosion waits for E).
    let canvas = render_example("particles-demo", 40, 640, 360);
    check_golden("particles-demo", &canvas);
}

#[test]
fn ui_demo_menu_matches_golden_image() {
    // 1280x720 because the menu is laid out in game-area pixels.
    let canvas = render_example("ui-demo", 30, 1280, 720);
    check_golden("ui-demo", &canvas);
}
