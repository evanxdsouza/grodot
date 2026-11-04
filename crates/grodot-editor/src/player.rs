//! The standalone game player (`grodot run`): runtime + renderer + window, no editor.

use crate::hud;
use grodot_core::config::Project;
use grodot_platform::{Audio, Event, MouseButton, PlatformWindow};
use grodot_render::canvas::Canvas;
use grodot_render::font::FontId;
use grodot_render::scene_render::{RenderOptions, SceneRenderer};
use grodot_runtime::{AudioCmd, LogLevel, Runtime};
use std::time::Instant;

/// Render one game frame into `canvas`.
pub fn render_game(canvas: &mut Canvas, renderer: &mut SceneRenderer, rt: &Runtime) {
    let area = canvas.full_rect();
    let w = rt.world.borrow();
    match SceneRenderer::camera_view(&w.scene, area) {
        Some(view) => {
            let extras = grodot_render::scene_render::RenderExtras { poses: Some(&w.poses) };
            renderer.render_ex(canvas, area, &w.scene, &view, &RenderOptions::default(), &extras);
            hud::draw_debug_lines(canvas, area, &view, &w);
        }
        None => {
            canvas.clear(0x101114);
            canvas.text(
                20,
                20,
                "This scene has no active Camera. Add a Camera entity in the editor.",
                0xffd23f,
                FontId::UiBold,
            );
        }
    }
    hud::draw_hud(canvas, area, &w);
}

fn forward_logs(rt: &Runtime, verbose: bool) {
    let entries = std::mem::take(&mut rt.world.borrow_mut().log.entries);
    for e in entries {
        match e.level {
            LogLevel::Info => {
                if verbose {
                    println!("{}", e.message)
                }
            }
            LogLevel::Warning => eprintln!("warning: {}", e.message),
            LogLevel::Error => eprintln!("{}", e.message),
        }
    }
}

/// Run the game in a window until it is closed or calls `Game.quit()`.
pub fn run_game(project: &Project) -> Result<(), String> {
    let mut rt = Runtime::from_project(project)?;
    let cfg = &project.config;
    let mut win = PlatformWindow::open(&cfg.window_title, cfg.window_width, cfg.window_height)?;
    let mut audio = match Audio::new() {
        Ok(a) => Some(a),
        Err(e) => {
            eprintln!("warning: audio disabled: {}", e);
            None
        }
    };
    let mut canvas = Canvas::new(win.width as usize, win.height as usize);
    let mut renderer = SceneRenderer::new(Some(project.root.clone()));
    let mut last = Instant::now();
    let mut mouse = (0.0f32, 0.0f32);
    rt.start();
    'main: loop {
        let start = Instant::now();
        {
            let mut w = rt.world.borrow_mut();
            for ev in win.poll_events() {
                match ev {
                    Event::Close => break 'main,
                    Event::Resize(nw, nh) => canvas.resize(nw as usize, nh as usize),
                    Event::KeyDown { key: Some(k), .. } => w.input.press(&k),
                    Event::KeyUp { key: Some(k) } => w.input.release(&k),
                    Event::MouseDown(b) => w.input.press(button_name(b)),
                    Event::MouseUp(b) => w.input.release(button_name(b)),
                    Event::MouseMove(x, y) => {
                        w.input.mouse_delta.0 += x - mouse.0;
                        w.input.mouse_delta.1 += y - mouse.1;
                        mouse = (x, y);
                        w.input.mouse_position = mouse;
                    }
                    Event::FocusLost => w.input.release_all(),
                    _ => {}
                }
            }
        }
        let dt = last.elapsed().as_secs_f32();
        last = Instant::now();
        rt.frame(dt);
        if let Some(a) = &mut audio {
            let cmds: Vec<AudioCmd> = std::mem::take(&mut rt.world.borrow_mut().audio_cmds);
            for c in cmds {
                let r = match c {
                    AudioCmd::Play { path, volume } => a.play(&path, volume, false, false),
                    AudioCmd::PlayMusic { path, volume } => a.play(&path, volume, true, true),
                    AudioCmd::StopMusic => {
                        let _: () = a.stop_music();
                        Ok(())
                    }
                    AudioCmd::SetVolume(v) => {
                        let _: () = a.set_volume(v);
                        Ok(())
                    }
                };
                if let Err(e) = r {
                    eprintln!("warning: audio: {}", e);
                }
            }
        } else {
            rt.world.borrow_mut().audio_cmds.clear();
        }
        forward_logs(&rt, true);
        if rt.world.borrow().quit_requested {
            break;
        }
        render_game(&mut canvas, &mut renderer, &rt);
        for w in renderer.warnings.drain(..) {
            eprintln!("warning: {}", w);
        }
        win.present(&canvas.color, canvas.width, canvas.height);
        grodot_platform::sleep_until(start, 1.0 / 60.0);
    }
    rt.shutdown();
    forward_logs(&rt, true);
    Ok(())
}

fn button_name(b: MouseButton) -> &'static str {
    match b {
        MouseButton::Left => "MouseLeft",
        MouseButton::Right => "MouseRight",
        MouseButton::Middle => "MouseMiddle",
    }
}

/// Run the game without a window for `frames` frames (CI smoke test). Returns the number of errors.
pub fn run_headless(
    project: &Project,
    frames: u32,
    screenshot: Option<&std::path::Path>,
    verbose: bool,
) -> Result<usize, String> {
    let mut rt = Runtime::from_project(project)?;
    rt.start();
    let mut errors = 0;
    let count_errors =
        |rt: &Runtime| rt.world.borrow().log.entries.iter().filter(|e| e.level == LogLevel::Error).count();
    for _ in 0..frames {
        rt.frame(1.0 / 60.0);
        rt.world.borrow_mut().audio_cmds.clear();
        errors += count_errors(&rt);
        forward_logs(&rt, verbose);
    }
    if let Some(path) = screenshot {
        let cfg = &project.config;
        let mut canvas = Canvas::new(cfg.window_width as usize, cfg.window_height as usize);
        let mut renderer = SceneRenderer::new(Some(project.root.clone()));
        render_game(&mut canvas, &mut renderer, &rt);
        canvas.save_png(path).map_err(|e| e.to_string())?;
    }
    rt.shutdown();
    errors += count_errors(&rt);
    forward_logs(&rt, verbose);
    Ok(errors)
}
