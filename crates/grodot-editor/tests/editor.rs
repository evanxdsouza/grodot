//! Drive the editor headlessly with synthetic input.

use grodot_core::config::Project;
use grodot_editor::editor::{Action, Editor};
use grodot_editor::ui::InputFrame;
use grodot_render::raster;
use std::path::{Path, PathBuf};

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let t = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &t);
        } else {
            std::fs::copy(&p, &t).unwrap();
        }
    }
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn editor(name: &str) -> (Editor, Temp) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/orb-garden");
    let dst = std::env::temp_dir().join(format!("grodot-ed-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir(&src, &dst);
    let mut ed = Editor::open(Project::open(&dst).unwrap()).unwrap();
    ed.resize(1440, 900);
    ed.frame(InputFrame::default(), 1.0 / 60.0);
    (ed, Temp(dst))
}

fn idle(ed: &mut Editor, n: usize) {
    for _ in 0..n {
        ed.frame(InputFrame::default(), 1.0 / 60.0);
    }
}

fn click(ed: &mut Editor, x: f32, y: f32) {
    ed.frame(InputFrame { mouse: (x, y), left_pressed: true, left_down: true, ..Default::default() }, 1.0 / 60.0);
    ed.frame(InputFrame { mouse: (x, y), left_released: true, ..Default::default() }, 1.0 / 60.0);
}

#[test]
fn clicking_in_viewport_selects_entity() {
    let (mut ed, _t) = editor("pick");
    let vp = ed.layout().3;
    let view = ed.camera.view(vp);
    let id = ed.scene.find_by_name("Platform2").unwrap().id;
    let p = ed.scene.world_matrix(id).transform_point(grodot_core::Vec3 { x: 0.0, y: 0.5, z: 0.0 });
    let (x, y) = raster::project(&view, vp, p).unwrap();
    click(&mut ed, x, y);
    assert_eq!(ed.selection, Some(id));
}

#[test]
fn play_mode_does_not_change_the_edited_scene() {
    let (mut ed, _t) = editor("play");
    let before = ed.scene.clone();
    ed.apply(Action::TogglePlay);
    assert!(ed.play.is_some());
    // Hold W for half a second of game time.
    ed.frame(InputFrame { keys: vec!["W".into()], ..Default::default() }, 1.0 / 60.0);
    idle(&mut ed, 30);
    let moved_z = {
        let rt = ed.play.as_ref().unwrap();
        let w = rt.world.borrow();
        w.scene.find_by_name("Player").unwrap().transform().position.z
    };
    assert!(moved_z < 3.0, "player moved in play mode (z = {})", moved_z);
    ed.frame(InputFrame { keys_up: vec!["W".into()], ..Default::default() }, 1.0 / 60.0);
    // Escape stops the game.
    ed.frame(InputFrame { keys: vec!["Escape".into()], ..Default::default() }, 1.0 / 60.0);
    assert!(ed.play.is_none());
    assert_eq!(ed.scene, before, "stopping restores the editor scene");
    assert!(!ed.dirty);
}

#[test]
fn undo_redo_and_save_roundtrip() {
    let (mut ed, t) = editor("undo");
    ed.apply(Action::CreateEntity("Sphere".into()));
    idle(&mut ed, 1);
    assert!(ed.scene.find_by_name("Sphere").is_some());
    assert!(ed.dirty);
    ed.frame(InputFrame { keys: vec!["Z".into()], ctrl: true, ..Default::default() }, 1.0 / 60.0);
    assert!(ed.scene.find_by_name("Sphere").is_none(), "Ctrl+Z removes the new sphere");
    ed.frame(InputFrame { keys: vec!["Y".into()], ctrl: true, ..Default::default() }, 1.0 / 60.0);
    assert!(ed.scene.find_by_name("Sphere").is_some(), "Ctrl+Y brings it back");
    ed.frame(InputFrame { keys: vec!["S".into()], ctrl: true, ..Default::default() }, 1.0 / 60.0);
    assert!(!ed.dirty);
    let saved = std::fs::read_to_string(t.0.join("scenes/main.gscene")).unwrap();
    let reloaded = grodot_core::Scene::from_json_str(&saved).unwrap();
    assert_eq!(reloaded.entities, ed.scene.entities);
}

#[test]
fn script_errors_in_play_mode_are_shown_not_fatal() {
    let (mut ed, t) = editor("err");
    std::fs::write(
        t.0.join("scripts/orb.gro"),
        "class Orb < Entity\n  function update(delta)\n    self.explode()\n  end\nend\n",
    )
    .unwrap();
    ed.apply(Action::TogglePlay);
    idle(&mut ed, 5);
    let errors: Vec<String> = ed
        .log
        .entries
        .iter()
        .filter(|e| e.level == grodot_runtime::LogLevel::Error)
        .map(|e| e.message.clone())
        .collect();
    assert!(errors.iter().any(|e| e.contains("Method 'explode' does not exist")), "{:?}", errors);
    assert!(ed.play.is_some(), "the editor keeps running");
    ed.apply(Action::TogglePlay);
    idle(&mut ed, 1);
}

#[test]
fn delete_and_duplicate_shortcuts() {
    let (mut ed, _t) = editor("dup");
    let id = ed.scene.find_by_name("Tree1").unwrap().id;
    ed.selection = Some(id);
    let n = ed.scene.entities.len();
    ed.frame(InputFrame { keys: vec!["D".into()], ctrl: true, ..Default::default() }, 1.0 / 60.0);
    assert_eq!(ed.scene.entities.len(), n + 2, "tree and its leaves are duplicated");
    ed.frame(InputFrame { keys: vec!["Delete".into()], ..Default::default() }, 1.0 / 60.0);
    assert_eq!(ed.scene.entities.len(), n);
}
