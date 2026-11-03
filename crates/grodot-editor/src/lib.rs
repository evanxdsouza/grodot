//! The Grodot editor and the standalone game player.
//!
//! Both run on the same scene model, runtime and renderer. The editor's Play
//! mode runs a *copy* of the scene, so stopping never changes what you edited.

pub mod editor;
pub mod hud;
pub mod player;
pub mod ui;

pub use editor::Editor;

use grodot_platform::{Event, MouseButton};
use std::time::Instant;
use ui::InputFrame;

/// Turns platform events into per-frame UI input.
#[derive(Default)]
pub struct InputCollector {
    frame: InputFrame,
    mouse: (f32, f32),
    left: bool,
    right: bool,
    middle: bool,
    ctrl: bool,
    shift: bool,
    last_click: Option<(Instant, (f32, f32))>,
}

impl InputCollector {
    pub fn handle(&mut self, ev: &Event) {
        match ev {
            Event::MouseMove(x, y) => {
                self.frame.mouse_delta.0 += x - self.mouse.0;
                self.frame.mouse_delta.1 += y - self.mouse.1;
                self.mouse = (*x, *y);
            }
            Event::MouseDown(b) => match b {
                MouseButton::Left => {
                    self.left = true;
                    self.frame.left_pressed = true;
                    let now = Instant::now();
                    if let Some((t, p)) = self.last_click {
                        if now.duration_since(t).as_millis() < 400 && (p.0 - self.mouse.0).abs() < 5.0 && (p.1 - self.mouse.1).abs() < 5.0 {
                            self.frame.double_click = true;
                        }
                    }
                    self.last_click = Some((now, self.mouse));
                }
                MouseButton::Right => {
                    self.right = true;
                    self.frame.right_pressed = true;
                }
                MouseButton::Middle => self.middle = true,
            },
            Event::MouseUp(b) => match b {
                MouseButton::Left => {
                    self.left = false;
                    self.frame.left_released = true;
                }
                MouseButton::Right => self.right = false,
                MouseButton::Middle => self.middle = false,
            },
            Event::Wheel(d) => self.frame.wheel += d,
            Event::KeyDown { key, text, ctrl, shift, .. } => {
                self.ctrl = *ctrl || key.as_deref() == Some("Ctrl");
                self.shift = *shift || key.as_deref() == Some("Shift");
                if let Some(k) = key {
                    if k == "Ctrl" {
                        self.ctrl = true;
                    }
                    if k == "Shift" {
                        self.shift = true;
                    }
                    self.frame.keys.push(k.clone());
                }
                if let Some(t) = text {
                    if !*ctrl {
                        self.frame.text.push_str(t);
                    }
                }
            }
            Event::KeyUp { key } => {
                if let Some(k) = key {
                    if k == "Ctrl" {
                        self.ctrl = false;
                    }
                    if k == "Shift" {
                        self.shift = false;
                    }
                    self.frame.keys_up.push(k.clone());
                }
            }
            Event::FocusLost => {
                self.left = false;
                self.right = false;
                self.middle = false;
                self.ctrl = false;
                self.shift = false;
                self.frame.focus_lost = true;
            }
            Event::Close | Event::Resize(..) => {}
        }
    }

    /// The input for this frame; resets per-frame flags.
    pub fn take(&mut self) -> InputFrame {
        let mut f = std::mem::take(&mut self.frame);
        f.mouse = self.mouse;
        f.left_down = self.left;
        f.right_down = self.right;
        f.middle_down = self.middle;
        f.ctrl = self.ctrl;
        f.shift = self.shift;
        f
    }
}

/// Open the editor window for a project directory.
pub fn run_editor(project_dir: &std::path::Path) -> Result<(), String> {
    let project = grodot_core::config::Project::open(project_dir)?;
    let mut editor = Editor::open(project)?;
    let mut win = grodot_platform::PlatformWindow::open(&editor.title(), 1440, 900)?;
    editor.resize(win.width, win.height);
    match grodot_platform::Audio::new() {
        Ok(a) => editor.audio = Some(a),
        Err(e) => editor.log.warn(format!("Audio disabled: {}", e)),
    }
    let mut input = InputCollector::default();
    let mut last = Instant::now();
    let mut title = String::new();
    loop {
        let start = Instant::now();
        let mut close = false;
        for ev in win.poll_events() {
            match ev {
                Event::Close => close = true,
                Event::Resize(w, h) => editor.resize(w, h),
                other => input.handle(&other),
            }
        }
        if close && editor.request_close() {
            break;
        }
        let dt = last.elapsed().as_secs_f32().min(0.1);
        last = Instant::now();
        editor.frame(input.take(), dt);
        if editor.quit {
            break;
        }
        let c = &editor.ui.canvas;
        win.present(&c.color, c.width, c.height);
        let t = editor.title();
        if t != title {
            win.set_title(&t);
            title = t;
        }
        grodot_platform::sleep_until(start, 1.0 / 60.0);
    }
    if let Some(mut rt) = editor.play.take() {
        rt.shutdown();
    }
    Ok(())
}

/// Render the editor headlessly and save a PNG (used for docs and smoke tests).
pub fn editor_screenshot(
    project_dir: &std::path::Path,
    out: &std::path::Path,
    width: u32,
    height: u32,
    select: Option<&str>,
    play_frames: u32,
) -> Result<(), String> {
    let project = grodot_core::config::Project::open(project_dir)?;
    let mut editor = Editor::open(project)?;
    editor.resize(width, height);
    if let Some(name) = select {
        editor.selection = editor.scene.find_by_name(name).map(|e| e.id);
        if editor.selection.is_none() {
            return Err(format!("no entity named '{}'", name));
        }
    }
    editor.frame(InputFrame::default(), 1.0 / 60.0);
    if play_frames > 0 {
        editor.toggle_play();
        for _ in 0..play_frames {
            editor.frame(InputFrame::default(), 1.0 / 60.0);
        }
    }
    editor.frame(InputFrame::default(), 1.0 / 60.0);
    editor.ui.canvas.save_png(out).map_err(|e| format!("cannot write {}: {}", out.display(), e))
}
