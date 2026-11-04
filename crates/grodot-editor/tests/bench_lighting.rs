//! Lighting benchmark: time frames of examples/orb-garden at 1280x720 for each
//! lighting configuration. Ignored by default (timing is machine-dependent):
//!
//!     cargo test --release -p grodot-editor --test bench_lighting -- --ignored --nocapture

use grodot_core::config::Project;
use grodot_editor::player::render_game;
use grodot_render::canvas::Canvas;
use grodot_render::raster::LightingMode;
use grodot_render::scene_render::SceneRenderer;
use grodot_runtime::Runtime;
use std::path::PathBuf;
use std::time::Instant;

fn time(label: &str, mode: LightingMode, shadow_size: usize) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/orb-garden");
    let project = Project::open(&root).unwrap();
    let mut rt = Runtime::from_project(&project).unwrap();
    rt.start();
    for _ in 0..30 {
        rt.frame(1.0 / 60.0); // settle the physics and the camera
    }
    let mut renderer = SceneRenderer::new(Some(project.root.clone()));
    renderer.lighting_mode = mode;
    renderer.shadow_map_size = shadow_size;
    let mut canvas = Canvas::new(1280, 720);
    render_game(&mut canvas, &mut renderer, &rt); // warm the asset caches
    let frames = 60;
    let t = Instant::now();
    for _ in 0..frames {
        render_game(&mut canvas, &mut renderer, &rt);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!("BENCH {:<34} {:>8.2} ms/frame  ({:>6.1} fps)", label, ms, 1000.0 / ms);
}

#[test]
#[ignore]
fn lighting_costs_orb_garden_1280x720() {
    time("per-vertex, no shadows", LightingMode::PerVertex, 0);
    time("per-vertex, shadows 1024", LightingMode::PerVertex, 1024);
    time("per-pixel, no shadows", LightingMode::PerPixel, 0);
    time("per-pixel, shadows 512", LightingMode::PerPixel, 512);
    time("per-pixel, shadows 1024", LightingMode::PerPixel, 1024);
    time("per-pixel, shadows 2048", LightingMode::PerPixel, 2048);
}
