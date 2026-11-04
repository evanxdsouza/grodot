//! Hot reload in play: scripts keep their state across an edit, a broken edit
//! keeps the last working version running, and changed assets are reported.

use grodot_core::scene::{Component, Scene};
use grodot_core::FieldValue;
use grodot_runtime::{LogLevel, Runtime};
use groscript::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

const V1: &str = r#"class Counter < Entity
  function ready()
    @n = 0
  end

  function update(delta)
    @n += 1
    @tag = "v1"
  end
end
"#;

const V2: &str = r#"class Counter < Entity
  function update(delta)
    @n += 1
    @tag = "v2"
  end
end
"#;

const BROKEN: &str = "class Counter < Entity\n  function update(delta)\n    @n += \n  end\nend\n";

/// Make sure the file's modification time moves on, even on coarse file systems.
fn write(path: &PathBuf, text: &str) {
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(path, text).unwrap();
}

fn field(rt: &Runtime, name: &str) -> Value {
    let w = rt.world.borrow();
    let id = w.scene.find_by_name("Counter").unwrap().id;
    match &w.script_objects[&id] {
        Value::Object(o) => o.fields.borrow().get(name).cloned().unwrap_or(Value::Nil),
        _ => panic!("not a script object"),
    }
}

fn errors(rt: &Runtime) -> Vec<String> {
    rt.world.borrow().log.entries.iter().filter(|e| e.level == LogLevel::Error).map(|e| e.message.clone()).collect()
}

/// Run frames and check for changes. Each frame is 0.1 s of game time, so six
/// frames pass the watcher's throttle.
fn step(rt: &mut Runtime) -> grodot_runtime::Reloaded {
    let mut last = Default::default();
    for _ in 0..6 {
        rt.frame(0.1);
        let r = rt.hot_reload();
        if !r.is_empty() {
            last = r;
        }
    }
    last
}

#[test]
fn edits_reload_scripts_and_keep_their_state() {
    let root = std::env::temp_dir().join(format!("grodot-hot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("assets/textures")).unwrap();
    let script = root.join("scripts/counter.gro");
    write(&script, V1);
    std::fs::write(root.join("assets/textures/t.png"), b"not really a png").unwrap();

    let mut scene = Scene::new("Test");
    let id = scene.add_entity("Counter");
    let mut s = Component::new_default("Script").unwrap();
    s.set_field("path", FieldValue::Text("scripts/counter.gro".into())).unwrap();
    scene.entity_mut(id).unwrap().add(s);
    let mut mr = Component::new_default("MeshRenderer").unwrap();
    mr.set_field("texture", FieldValue::Text("assets/textures/t.png".into())).unwrap();
    scene.entity_mut(id).unwrap().add(mr);

    let mut rt = Runtime::new(scene, Some(root.clone()), BTreeMap::new());
    rt.start();
    rt.hot_reload(); // records the modification times
    step(&mut rt);
    let before = match field(&rt, "n") {
        Value::Number(n) => n,
        other => panic!("n should be a number, got {:?}", other),
    };
    assert!(before >= 6.0);
    assert!(matches!(field(&rt, "tag"), Value::Str(s) if &*s == "v1"));

    // A working edit: the new code runs, and the count carries on from where it was.
    write(&script, V2);
    let r = step(&mut rt);
    assert_eq!(r.scripts, vec!["scripts/counter.gro".to_string()], "{:?}", r);
    assert!(matches!(field(&rt, "tag"), Value::Str(s) if &*s == "v2"));
    let after = match field(&rt, "n") {
        Value::Number(n) => n,
        other => panic!("n should be a number, got {:?}", other),
    };
    assert!(after > before, "state should survive the reload: {} then {}", before, after);
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));

    // A broken edit: reported, and the last working version keeps running.
    write(&script, BROKEN);
    let r = step(&mut rt);
    assert_eq!(r.failed, vec!["scripts/counter.gro".to_string()]);
    assert!(!errors(&rt).is_empty(), "the syntax error is logged");
    assert!(matches!(field(&rt, "tag"), Value::Str(s) if &*s == "v2"), "the old version still runs");

    // Fixing the file brings it back.
    write(&script, V1);
    let r = step(&mut rt);
    assert_eq!(r.scripts, vec!["scripts/counter.gro".to_string()]);
    assert!(matches!(field(&rt, "tag"), Value::Str(s) if &*s == "v1"));

    // A changed texture is reported so the host can drop its cache.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(root.join("assets/textures/t.png"), b"changed").unwrap();
    let r = step(&mut rt);
    assert_eq!(r.assets, vec!["assets/textures/t.png".to_string()]);
    assert!(!r.scene);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn changed_helper_module_reloads_the_scripts_that_use_it() {
    let root = std::env::temp_dir().join(format!("grodot-hot-helper-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    let helper = root.join("scripts/helper.gro");
    write(&helper, "module Helpers\n  function speed()\n    return 1\n  end\nend\n");
    write(
        &root.join("scripts/counter.gro"),
        "import \"helper\"\nclass Counter < Entity\n  function update(delta)\n    @speed = Helpers.speed()\n  end\nend\n",
    );

    let mut scene = Scene::new("Test");
    let id = scene.add_entity("Counter");
    let mut s = Component::new_default("Script").unwrap();
    s.set_field("path", FieldValue::Text("scripts/counter.gro".into())).unwrap();
    scene.entity_mut(id).unwrap().add(s);
    let mut rt = Runtime::new(scene, Some(root.clone()), BTreeMap::new());
    rt.start();
    rt.hot_reload();
    step(&mut rt);
    assert!(matches!(field(&rt, "speed"), Value::Number(n) if n == 1.0), "{:?} {:?}", field(&rt, "speed"), errors(&rt));

    write(&helper, "module Helpers\n  function speed()\n    return 7\n  end\nend\n");
    let r = step(&mut rt);
    assert_eq!(r.scripts, vec!["scripts/counter.gro".to_string()], "{:?}", r);
    assert!(matches!(field(&rt, "speed"), Value::Number(n) if n == 7.0));
    let _ = std::fs::remove_dir_all(&root);
}
