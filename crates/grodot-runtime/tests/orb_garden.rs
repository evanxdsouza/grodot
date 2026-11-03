//! Headless integration tests: play the sample game without a window.

use grodot_core::config::Project;
use grodot_core::math::vec3;
use grodot_runtime::{AudioCmd, LogLevel, Runtime};
use groscript::Value;
use std::path::PathBuf;

const DT: f32 = 1.0 / 60.0;

fn project() -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/orb-garden");
    Project::open(&dir).expect("sample project opens")
}

fn field(rt: &Runtime, entity: &str, name: &str) -> Value {
    let w = rt.world.borrow();
    let id = w.scene.find_by_name(entity).unwrap_or_else(|| panic!("no entity {}", entity)).id;
    match &w.script_objects[&id] {
        Value::Object(o) => o.fields.borrow().get(name).cloned().unwrap_or(Value::Nil),
        _ => panic!("not a script object"),
    }
}

fn pos(rt: &Runtime, entity: &str) -> grodot_core::Vec3 {
    rt.world.borrow().scene.find_by_name(entity).unwrap().transform().position
}

fn set_pos(rt: &Runtime, entity: &str, p: grodot_core::Vec3) {
    let mut w = rt.world.borrow_mut();
    let id = w.scene.find_by_name(entity).unwrap().id;
    w.scene.entity_mut(id).unwrap().transform_mut().position = p;
}

fn run(rt: &mut Runtime, frames: usize) {
    for _ in 0..frames {
        rt.frame(DT);
    }
}

fn assert_no_errors(rt: &Runtime) {
    let w = rt.world.borrow();
    let errors: Vec<_> = w.log.entries.iter().filter(|e| e.level == LogLevel::Error).map(|e| e.message.clone()).collect();
    assert!(errors.is_empty(), "script errors:\n{}", errors.join("\n\n"));
}

#[test]
fn game_starts_and_player_lands() {
    let mut rt = Runtime::from_project(&project()).unwrap();
    rt.start();
    assert_eq!(rt.script_count(), 1 + 1 + 1 + 5 + 2, "camera, manager, player, 5 orbs, 2 hazards");
    run(&mut rt, 90);
    assert_no_errors(&rt);
    let p = pos(&rt, "Player");
    assert!((p.y - 0.5).abs() < 0.05, "player should stand on the ground, y = {}", p.y);
    assert!(matches!(field(&rt, "GameManager", "total"), Value::Number(n) if n == 5.0));
    // Music started from ready().
    assert!(rt.world.borrow().audio_cmds.iter().any(|c| matches!(c, AudioCmd::PlayMusic { .. })));
    // HUD lines are produced every frame.
    assert_eq!(rt.world.borrow().hud.lines.len(), 3);
}

#[test]
fn input_moves_player_and_jump_works() {
    let mut rt = Runtime::from_project(&project()).unwrap();
    run(&mut rt, 60);
    let start = pos(&rt, "Player");
    rt.world.borrow_mut().input.press("W");
    run(&mut rt, 30);
    rt.world.borrow_mut().input.release("W");
    let moved = pos(&rt, "Player");
    assert!(moved.z < start.z - 2.0, "W should move the player forward (-Z): {} -> {}", start.z, moved.z);
    // Jump
    run(&mut rt, 10);
    rt.world.borrow_mut().input.press("Space");
    run(&mut rt, 1);
    rt.world.borrow_mut().input.release("Space");
    run(&mut rt, 15);
    assert!(pos(&rt, "Player").y > 1.5, "player should be in the air after jumping");
    assert_no_errors(&rt);
}

#[test]
fn collecting_orbs_wins_the_game() {
    let mut rt = Runtime::from_project(&project()).unwrap();
    run(&mut rt, 30);
    let orbs: Vec<String> = (1..=5).map(|i| format!("Orb{}", i)).collect();
    for (i, orb) in orbs.iter().enumerate() {
        let p = pos(&rt, orb);
        set_pos(&rt, "Player", p);
        run(&mut rt, 3);
        assert!(rt.world.borrow().scene.find_by_name(orb).is_none(), "{} should be collected", orb);
        assert!(matches!(field(&rt, "GameManager", "score"), Value::Number(n) if n == (i + 1) as f64));
    }
    assert!(matches!(field(&rt, "GameManager", "state"), Value::Symbol(s) if &*s == "won"));
    run(&mut rt, 1);
    assert_eq!(rt.world.borrow().hud.message.as_deref(), Some("You win!"));
    // Restart with R reloads the scene: all orbs are back.
    rt.world.borrow_mut().input.press("R");
    run(&mut rt, 2);
    assert!(rt.world.borrow().scene.find_by_name("Orb1").is_some());
    assert!(matches!(field(&rt, "GameManager", "score"), Value::Number(n) if n == 0.0));
    assert_no_errors(&rt);
}

#[test]
fn hazard_costs_a_life_and_respawns_player() {
    let mut rt = Runtime::from_project(&project()).unwrap();
    run(&mut rt, 30);
    let h = pos(&rt, "Hazard1");
    set_pos(&rt, "Player", h);
    run(&mut rt, 3);
    assert!(matches!(field(&rt, "GameManager", "lives"), Value::Number(n) if n == 2.0));
    let p = pos(&rt, "Player");
    assert!((p.z - 4.0).abs() < 0.5, "player respawned at start, z = {}", p.z);
    // Falling off the world also costs a life.
    set_pos(&rt, "Player", vec3(30.0, -20.0, 0.0));
    run(&mut rt, 2);
    assert!(matches!(field(&rt, "GameManager", "lives"), Value::Number(n) if n == 1.0));
    assert_no_errors(&rt);
}
