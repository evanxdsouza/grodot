//! The UI demo's menu → game → pause → quit flow, driven by mouse and keyboard.

use grodot_core::config::Project;
use grodot_runtime::ui::Cmd;
use grodot_runtime::Runtime;
use std::path::PathBuf;

const DT: f32 = 1.0 / 60.0;

fn demo() -> Runtime {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/ui-demo");
    let project = Project::open(&dir).expect("ui demo opens");
    let mut rt = Runtime::from_project(&project).unwrap();
    rt.start();
    rt
}

fn frame(rt: &mut Runtime) {
    rt.frame(DT);
}

/// Press a key or mouse button for one frame.
fn tap(rt: &mut Runtime, key: &str) {
    rt.world.borrow_mut().input.press(key);
    frame(rt);
    rt.world.borrow_mut().input.release(key);
    frame(rt);
}

/// Move the mouse to a point and click it.
fn click(rt: &mut Runtime, x: f32, y: f32) {
    {
        let mut w = rt.world.borrow_mut();
        w.input.mouse_position = (x, y);
        w.input.mouse_delta = (0.0, 0.0);
    }
    tap(rt, "MouseLeft");
}

fn buttons(rt: &Runtime) -> Vec<String> {
    rt.world
        .borrow()
        .ui
        .commands
        .iter()
        .filter_map(|c| match c {
            Cmd::Button { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect()
}

fn scene(rt: &Runtime) -> String {
    rt.world.borrow().scene_path.clone().unwrap_or_default()
}

fn errors(rt: &Runtime) -> Vec<String> {
    rt.world
        .borrow()
        .log
        .entries
        .iter()
        .filter(|e| e.level == grodot_runtime::LogLevel::Error)
        .map(|e| e.message.clone())
        .collect()
}

#[test]
fn menu_play_by_mouse_then_pause_resume_and_quit() {
    let mut rt = demo();
    frame(&mut rt);
    assert_eq!(scene(&rt), "scenes/menu.gscene");
    assert_eq!(buttons(&rt), vec!["Play".to_string(), "Quit".to_string()]);

    // Play is centred at (640, 274) on the default 1280x720 screen.
    click(&mut rt, 640.0, 274.0);
    assert_eq!(scene(&rt), "scenes/game.gscene", "Play loads the game scene");
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));

    // Escape pauses: the pause menu appears and the crate stops turning.
    frame(&mut rt);
    let crate_rotation = |rt: &Runtime| rt.world.borrow().scene.find_by_name("Crate").unwrap().transform().rotation;
    let before_pause = crate_rotation(&rt);
    tap(&mut rt, "Escape");
    assert!(rt.world.borrow().paused, "Escape pauses");
    assert_eq!(buttons(&rt), vec!["Resume".to_string(), "Main menu".to_string(), "Quit".to_string()]);
    let paused_at = crate_rotation(&rt);
    for _ in 0..30 {
        frame(&mut rt);
    }
    assert_eq!(crate_rotation(&rt), paused_at, "the spinner stops while paused");
    assert!(crate_rotation(&rt) != before_pause || paused_at == before_pause);

    // Resume (mouse) carries on: the crate turns again.
    click(&mut rt, 640.0, 292.0);
    assert!(!rt.world.borrow().paused, "Resume unpauses");
    for _ in 0..30 {
        frame(&mut rt);
    }
    assert!(crate_rotation(&rt) != paused_at, "the crate turns again after resume");

    // Pause again and go back to the menu.
    tap(&mut rt, "Escape");
    click(&mut rt, 640.0, 352.0);
    assert_eq!(scene(&rt), "scenes/menu.gscene", "Main menu returns to the menu");
    assert!(!rt.world.borrow().paused, "a new scene starts unpaused");
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
}

#[test]
fn quit_from_the_pause_menu() {
    let mut rt = demo();
    frame(&mut rt);
    click(&mut rt, 640.0, 274.0);
    tap(&mut rt, "Escape");
    click(&mut rt, 640.0, 412.0);
    assert!(rt.world.borrow().quit_requested, "Quit closes the game");
}

#[test]
fn keyboard_only_menu_flow() {
    let mut rt = demo();
    frame(&mut rt);
    // Down moves focus to the first button (Play); Enter activates it.
    tap(&mut rt, "Down");
    assert_eq!(rt.world.borrow().ui.focus.as_deref(), Some("Play"));
    tap(&mut rt, "Enter");
    assert_eq!(scene(&rt), "scenes/game.gscene");
    // Escape opens the pause menu. Focus from the old menu is gone, so the first
    // Down selects Resume; two more Downs reach Quit.
    tap(&mut rt, "Escape");
    tap(&mut rt, "Down");
    tap(&mut rt, "Down");
    tap(&mut rt, "Down");
    assert_eq!(rt.world.borrow().ui.focus.as_deref(), Some("Quit"));
    tap(&mut rt, "Enter");
    assert!(rt.world.borrow().quit_requested);
}
