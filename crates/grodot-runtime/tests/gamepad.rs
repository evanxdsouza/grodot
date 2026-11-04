//! Gamepad input from scripts: pad events feed the input state, the stick
//! drives the standard movement axis, and script errors point at the line.

use grodot_core::scene::{Component, Scene};
use grodot_core::FieldValue;
use grodot_runtime::{LogLevel, Runtime};
use groscript::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn ev(kind: u8, number: u8, value: i16) -> [u8; 8] {
    let v = value.to_le_bytes();
    [0, 0, 0, 0, v[0], v[1], kind, number]
}

fn project(name: &str) -> (PathBuf, Scene) {
    let root = std::env::temp_dir().join(format!("grodot-gamepad-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    let mut s = Scene::new("Test");
    let id = s.add_entity("Probe");
    let mut c = Component::new_default("Script").unwrap();
    c.set_field("path", FieldValue::Text("scripts/probe.gro".into())).unwrap();
    s.entity_mut(id).unwrap().add(c);
    (root, s)
}

fn field(rt: &Runtime, name: &str) -> Value {
    let w = rt.world.borrow();
    let id = w.scene.find_by_name("Probe").unwrap().id;
    match &w.script_objects[&id] {
        Value::Object(o) => o.fields.borrow().get(name).cloned().unwrap_or(Value::Nil),
        _ => panic!("not a script object"),
    }
}

fn errors(rt: &Runtime) -> Vec<String> {
    rt.world.borrow().log.entries.iter().filter(|e| e.level == LogLevel::Error).map(|e| e.message.clone()).collect()
}

#[test]
fn pad_buttons_and_the_stick_reach_scripts() {
    let (root, scene) = project("probe");
    std::fs::write(
        root.join("scripts/probe.gro"),
        r#"class Probe < Entity
  function update(delta)
    @connected = Gamepad.connected?()
    @a_down = Gamepad.down?("a")
    @a_pressed = Gamepad.just_pressed?("a")
    @jump_pressed = Input.just_pressed("jump")
    @move = Input.axis("move_left", "move_right")
    @fwd = Input.axis("move_forward", "move_back")
    @lx = Gamepad.axis("left_x")
  end
end
"#,
    )
    .unwrap();
    let mut actions = BTreeMap::new();
    actions.insert("jump".to_string(), vec!["Space".to_string(), "PadA".to_string()]);
    actions.insert("move_left".to_string(), vec!["A".to_string()]);
    actions.insert("move_right".to_string(), vec!["D".to_string()]);
    actions.insert("move_forward".to_string(), vec!["W".to_string()]);
    actions.insert("move_back".to_string(), vec!["S".to_string()]);
    let mut rt = Runtime::new(scene, Some(root.clone()), actions);
    rt.start();
    rt.frame(1.0 / 60.0);
    assert!(matches!(field(&rt, "connected"), Value::Bool(false)), "no pad in the test environment");
    assert!(matches!(field(&rt, "move"), Value::Number(n) if n == 0.0));

    // Connect a pad, press A (a binding for the jump action), push the stick right and up.
    {
        let mut w = rt.world.borrow_mut();
        let w = &mut *w;
        w.gamepad.set_virtual(true);
        w.gamepad.feed_event(ev(1, 0, 1), &mut w.input);
        w.gamepad.feed_event(ev(2, 0, 32767), &mut w.input);
        w.gamepad.feed_event(ev(2, 1, -32767), &mut w.input);
    }
    rt.frame(1.0 / 60.0);
    assert!(matches!(field(&rt, "connected"), Value::Bool(true)));
    assert!(matches!(field(&rt, "a_down"), Value::Bool(true)));
    assert!(matches!(field(&rt, "a_pressed"), Value::Bool(true)), "the press is seen in the frame it happened");
    assert!(matches!(field(&rt, "jump_pressed"), Value::Bool(true)), "PadA is a jump binding");
    assert!(matches!(field(&rt, "move"), Value::Number(n) if (n - 1.0).abs() < 1e-3), "stick right moves right");
    assert!(matches!(field(&rt, "fwd"), Value::Number(n) if (n + 1.0).abs() < 1e-3), "stick up is forward");
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));

    // Next frame the press is gone but the button is still held.
    rt.frame(1.0 / 60.0);
    assert!(matches!(field(&rt, "a_pressed"), Value::Bool(false)));
    assert!(matches!(field(&rt, "a_down"), Value::Bool(true)));

    // Keyboard still works. The larger magnitude wins, and on a tie the keyboard
    // does: A (-1) against a full stick right (+1) gives -1.
    rt.world.borrow_mut().input.press("A");
    rt.frame(1.0 / 60.0);
    assert!(matches!(field(&rt, "move"), Value::Number(n) if (n + 1.0).abs() < 1e-3), "keyboard wins a tie");
    rt.world.borrow_mut().input.end_frame();
    rt.world.borrow_mut().input.release("A");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_pad_button_is_reported_at_the_script_line() {
    let (root, scene) = project("bad");
    std::fs::write(
        root.join("scripts/probe.gro"),
        "class Probe < Entity\n  function update(delta)\n    Gamepad.down?(\"triangle\")\n  end\nend\n",
    )
    .unwrap();
    let mut rt = Runtime::new(scene, Some(root.clone()), BTreeMap::new());
    rt.start();
    rt.frame(1.0 / 60.0);
    let errs = errors(&rt);
    assert_eq!(errs.len(), 1, "{:?}", errs);
    assert!(errs[0].contains("unknown pad button 'triangle'"), "{}", errs[0]);
    assert!(errs[0].contains("Line: 3"), "{}", errs[0]);
    let _ = std::fs::remove_dir_all(&root);
}
