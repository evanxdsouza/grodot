//! Runtime behaviour around scripts: errors, timers, spawning and destroyed handles.

use grodot_core::scene::{Component, Scene};
use grodot_core::FieldValue;
use grodot_runtime::{LogLevel, Runtime};
use groscript::Value;
use std::path::PathBuf;

struct TempProject {
    root: PathBuf,
}

impl TempProject {
    fn new(name: &str) -> TempProject {
        let root = std::env::temp_dir().join(format!("grodot-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("prefabs")).unwrap();
        TempProject { root }
    }
    fn write(&self, rel: &str, text: &str) {
        std::fs::write(self.root.join(rel), text).unwrap();
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn scene_with_scripts(scripts: &[(&str, &str)]) -> Scene {
    let mut s = Scene::new("Test");
    for (name, path) in scripts {
        let id = s.add_entity(name);
        let mut c = Component::new_default("Script").unwrap();
        c.set_field("path", FieldValue::Text(path.to_string())).unwrap();
        s.entity_mut(id).unwrap().add(c);
    }
    s
}

fn field(rt: &Runtime, entity: &str, name: &str) -> Value {
    let w = rt.world.borrow();
    let id = w.scene.find_by_name(entity).unwrap().id;
    match &w.script_objects[&id] {
        Value::Object(o) => o.fields.borrow().get(name).cloned().unwrap_or(Value::Nil),
        _ => panic!(),
    }
}

fn errors(rt: &Runtime) -> Vec<String> {
    rt.world.borrow().log.entries.iter().filter(|e| e.level == LogLevel::Error).map(|e| e.message.clone()).collect()
}

#[test]
fn script_error_disables_only_that_script() {
    let p = TempProject::new("errors");
    p.write("scripts/bad.gro", "class Bad < Entity\n  function update(delta)\n    jump_now()\n  end\nend\n");
    p.write("scripts/good.gro", "class Good < Entity\n  function ready()\n    @frames = 0\n  end\n  function update(delta)\n    @frames += 1\n  end\nend\n");
    let scene = scene_with_scripts(&[("Bad", "scripts/bad.gro"), ("Good", "scripts/good.gro")]);
    let mut rt = Runtime::new(scene, Some(p.root.clone()), Default::default());
    for _ in 0..10 {
        rt.frame(0.016);
    }
    let errs = errors(&rt);
    assert_eq!(errs.len(), 1, "error reported once, then the script is disabled: {:?}", errs);
    assert!(errs[0].contains("undefined function 'jump_now'"));
    assert!(errs[0].contains("File: scripts/bad.gro"));
    assert!(errs[0].contains("Line: 3"));
    assert!(errs[0].contains("Bad.update"));
    assert_eq!(rt.disabled_scripts(), 1);
    assert!(matches!(field(&rt, "Good", "frames"), Value::Number(n) if n == 10.0));
}

#[test]
fn syntax_errors_and_missing_classes_are_reported() {
    let p = TempProject::new("syntax");
    p.write("scripts/broken.gro", "class Broken < Entity\n  function ready()\n    x = (1 +\n  end\nend\n");
    p.write("scripts/noclass.gro", "x = 1\n");
    let scene =
        scene_with_scripts(&[("A", "scripts/broken.gro"), ("B", "scripts/noclass.gro"), ("C", "scripts/missing.gro")]);
    let mut rt = Runtime::new(scene, Some(p.root.clone()), Default::default());
    rt.frame(0.016);
    let errs = errors(&rt).join("\n");
    assert!(errs.contains("SyntaxError"), "{}", errs);
    assert!(errs.contains("must define a class that extends Entity"), "{}", errs);
    assert!(errs.contains("cannot read 'scripts/missing.gro'"), "{}", errs);
    assert_eq!(rt.script_count(), 0);
}

#[test]
fn infinite_loop_is_stopped_by_budget() {
    let p = TempProject::new("budget");
    p.write(
        "scripts/loop.gro",
        "class Loop < Entity\n  function update(delta)\n    while true\n      x = 1\n    end\n  end\nend\n",
    );
    let mut rt =
        Runtime::new(scene_with_scripts(&[("L", "scripts/loop.gro")]), Some(p.root.clone()), Default::default());
    rt.frame(0.016);
    assert!(errors(&rt)[0].contains("execution budget exceeded"));
    rt.frame(0.016); // and the game keeps running
}

#[test]
fn timers_spawning_and_destroyed_handles() {
    let p = TempProject::new("timers");
    p.write(
        "prefabs/coin.gprefab",
        r#"{"scene_version":1,"entities":[{"id":1,"name":"Coin","components":[{"type":"Transform"},{"type":"Script","path":"scripts/coin.gro"}]}]}"#,
    );
    p.write("scripts/coin.gro", "class Coin < Entity\n  function ready()\n    $coins_ready += 1\n  end\nend\n");
    p.write(
        "scripts/spawner.gro",
        r#"
class Spawner < Entity
  function ready()
    $coins_ready = 0
    @ticks = 0
    @spawned = []
    Timer.after(0.05, :spawn_one)
    @repeat = Timer.every(0.02, :tick)
  end

  function spawn_one()
    coin = Scene.spawn("prefabs/coin.gprefab", Vector3.new(1, 2, 3))
    @spawned.push(coin)
    @first_pos = coin.position
  end

  function tick()
    @ticks += 1
    if @ticks == 3
      Timer.cancel(@repeat)
    end
  end

  function update(delta)
    if @spawned.length() > 0 and @destroyed == nil
      @spawned[0].destroy()
      @destroyed = true
    elsif @destroyed == true
      @valid_after = @spawned[0].valid?()
      @destroyed = false
    end
  end
end
"#,
    );
    let mut rt = Runtime::new(
        scene_with_scripts(&[("Spawner", "scripts/spawner.gro")]),
        Some(p.root.clone()),
        Default::default(),
    );
    for _ in 0..20 {
        rt.frame(0.01);
    }
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    assert!(matches!(field(&rt, "Spawner", "ticks"), Value::Number(n) if n == 3.0));
    assert!(matches!(field(&rt, "Spawner", "first_pos"), Value::Vec3(v) if v.x == 1.0 && v.z == 3.0));
    assert!(matches!(field(&rt, "Spawner", "valid_after"), Value::Bool(false)));
    assert!(matches!(rt.interp.gvars.get("coins_ready"), Some(Value::Number(n)) if *n == 1.0));
    assert!(rt.world.borrow().scene.find_by_name("Coin").is_none());
}

#[test]
fn using_a_destroyed_entity_is_a_clear_error() {
    let p = TempProject::new("dead");
    p.write(
        "scripts/a.gro",
        "class A < Entity\n  function ready()\n    @other = Scene.find(\"B\")\n    @other.destroy()\n    @n = 0\n  end\n  function update(delta)\n    @n += 1\n    if @n == 2\n      print(@other.position)\n    end\n  end\nend\n",
    );
    let mut scene = scene_with_scripts(&[("A", "scripts/a.gro")]);
    scene.add_entity("B");
    let mut rt = Runtime::new(scene, Some(p.root.clone()), Default::default());
    for _ in 0..3 {
        rt.frame(0.016);
    }
    let e = errors(&rt).join("\n");
    assert!(e.contains("EngineError") && e.contains("has been destroyed"), "{}", e);
}

#[test]
fn component_access_from_scripts() {
    let p = TempProject::new("components");
    p.write(
        "scripts/c.gro",
        r#"
class C < Entity
  function ready()
    cam = self.add_component("Camera")
    cam.field_of_view = 90
    @fov = self.get_component("Camera").field_of_view
    @has = self.has_component?("Camera")
    @none = self.get_component("RigidBody")
    self.add_component("MeshRenderer")
    self.color = Color.new(1, 0, 0)
    @mesh = self.get_component("MeshRenderer").mesh
  end
end
"#,
    );
    let mut rt = Runtime::new(scene_with_scripts(&[("C", "scripts/c.gro")]), Some(p.root.clone()), Default::default());
    rt.frame(0.016);
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    assert!(matches!(field(&rt, "C", "fov"), Value::Number(n) if n == 90.0));
    assert!(matches!(field(&rt, "C", "has"), Value::Bool(true)));
    assert!(matches!(field(&rt, "C", "none"), Value::Nil));
    assert!(matches!(field(&rt, "C", "mesh"), Value::Str(s) if &*s == "cube"));
}

#[test]
fn node_paths_and_groups_match_godot_style_apis() {
    let p = TempProject::new("node_paths_groups");
    p.write(
        "scripts/root.gro",
        r#"
class Root < Entity
  function ready()
    @child_name = self.get_node("Child").name
    @deep_name = self.get_node("Child/Deep").name
    @absolute_name = self.get_node("/root/Root/Child").name
    @parent_is_nil = self.get_parent() == nil
    @missing = self.get_node_or_null("Missing")
    @added = self.add_to_group("actors")
    @duplicate = self.add_to_group("actors")
    @in_group = self.is_in_group?("actors")
    @groups_count = self.get_groups().length()
    @query_count = Scene.get_nodes_in_group("actors").length()
  end
end
"#,
    );
    let mut scene = scene_with_scripts(&[("Root", "scripts/root.gro")]);
    let root = scene.find_by_name("Root").unwrap().id;
    let child = scene.add_entity("Child");
    scene.set_parent(child, Some(root)).unwrap();
    let deep = scene.add_entity("Deep");
    scene.set_parent(deep, Some(child)).unwrap();

    let mut rt = Runtime::new(scene, Some(p.root.clone()), Default::default());
    rt.frame(0.016);
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    assert!(matches!(field(&rt, "Root", "child_name"), Value::Str(s) if &*s == "Child"));
    assert!(matches!(field(&rt, "Root", "deep_name"), Value::Str(s) if &*s == "Deep"));
    assert!(matches!(field(&rt, "Root", "absolute_name"), Value::Str(s) if &*s == "Child"));
    assert!(matches!(field(&rt, "Root", "parent_is_nil"), Value::Bool(true)));
    assert!(matches!(field(&rt, "Root", "missing"), Value::Nil));
    assert!(matches!(field(&rt, "Root", "added"), Value::Bool(true)));
    assert!(matches!(field(&rt, "Root", "duplicate"), Value::Bool(false)));
    assert!(matches!(field(&rt, "Root", "in_group"), Value::Bool(true)));
    assert!(matches!(field(&rt, "Root", "groups_count"), Value::Number(n) if n == 1.0));
    assert!(matches!(field(&rt, "Root", "query_count"), Value::Number(n) if n == 1.0));
}

#[test]
fn named_signals_connect_emit_and_disconnect() {
    let p = TempProject::new("signals");
    p.write(
        "scripts/emitter.gro",
        r#"
class Emitter < Entity
  function ready()
    @receiver = Scene.find("Receiver")
    @connected = self.connect("value_changed", @receiver, "on_value_changed")
    @connected_again = self.connect("value_changed", @receiver, "on_value_changed")
    @is_connected = self.is_connected?("value_changed", @receiver, "on_value_changed")
    self.emit_signal("value_changed", 42, "hello")
  end

  function update(delta)
    if @connected and @disconnected == nil
      @disconnected = self.disconnect("value_changed", @receiver, "on_value_changed")
      @is_connected_after = self.is_connected?("value_changed", @receiver, "on_value_changed")
      self.emit_signal("value_changed", 99, "ignored")
    end
  end
end
"#,
    );
    p.write(
        "scripts/receiver.gro",
        r#"
class Receiver < Entity
  function ready()
    @value = 0
    @label = ""
  end

  function on_value_changed(value, label)
    @value = value
    @label = label
  end
end
"#,
    );
    let mut rt = Runtime::new(
        scene_with_scripts(&[("Emitter", "scripts/emitter.gro"), ("Receiver", "scripts/receiver.gro")]),
        Some(p.root.clone()),
        Default::default(),
    );
    rt.start();
    assert!(errors(&rt).is_empty(), "{:?}", errors(&rt));
    assert!(matches!(field(&rt, "Emitter", "connected"), Value::Bool(true)));
    assert!(matches!(field(&rt, "Emitter", "connected_again"), Value::Bool(false)));
    assert!(matches!(field(&rt, "Emitter", "is_connected"), Value::Bool(true)));
    assert!(matches!(field(&rt, "Receiver", "value"), Value::Number(n) if n == 42.0));
    assert!(matches!(field(&rt, "Receiver", "label"), Value::Str(s) if &*s == "hello"));

    rt.frame(0.016);
    assert!(matches!(field(&rt, "Emitter", "disconnected"), Value::Bool(true)));
    assert!(matches!(field(&rt, "Emitter", "is_connected_after"), Value::Bool(false)));
    assert!(matches!(field(&rt, "Receiver", "value"), Value::Number(n) if n == 42.0));
}

#[test]
fn material_bindings_animate_uvs_and_report_bad_textures() {
    let p = TempProject::new("material");
    std::fs::create_dir_all(p.root.join("assets/textures")).unwrap();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../grodot-core/tests/fixtures/images/rgba8.png");
    std::fs::copy(fixture, p.root.join("assets/textures/t.png")).unwrap();
    p.write(
        "scripts/w.gro",
        r#"class W < Entity
  function ready()
    self.material.texture = "assets/textures/t.png"
    self.material.uv_scale = Vector2.new(4, 2)
    self.material.alpha_mode = "cutout"
  end
  function update(delta)
    self.material.uv_offset = self.material.uv_offset + Vector2.new(1, 0.5) * delta
    @tex = self.material.texture
    @fields = self.material.fields().length()
    @mode = self.get_component("MeshRenderer").alpha_mode
  end
end
"#,
    );
    p.write(
        "scripts/bad.gro",
        "class Bad < Entity\n  function ready()\n    self.material.texture = \"assets/textures/nope.png\"\n  end\nend\n",
    );
    let mut scene = scene_with_scripts(&[("W", "scripts/w.gro"), ("Bad", "scripts/bad.gro")]);
    for e in scene.entities.iter_mut() {
        e.add(Component::new_default("MeshRenderer").unwrap());
    }
    let mut rt = Runtime::new(scene, Some(p.root.clone()), Default::default());
    for _ in 0..10 {
        rt.frame(0.1);
    }
    let errs = errors(&rt);
    assert_eq!(errs.len(), 1, "{:?}", errs);
    assert!(errs[0].contains("texture 'assets/textures/nope.png' not found"), "{}", errs[0]);
    assert!(errs[0].contains("File: scripts/bad.gro") && errs[0].contains("Line: 3"), "{}", errs[0]);
    assert!(matches!(field(&rt, "W", "tex"), Value::Str(s) if &*s == "assets/textures/t.png"));
    assert!(matches!(field(&rt, "W", "fields"), Value::Number(n) if n == 9.0));
    assert!(matches!(field(&rt, "W", "mode"), Value::Str(s) if &*s == "cutout"));
    let w = rt.world.borrow();
    let mr = w.scene.find_by_name("W").unwrap().mesh().unwrap().clone();
    assert!(
        (mr.material.uv_offset.x - 1.0).abs() < 1e-4,
        "10 frames x 0.1 s x 1 unit/s = 1, got {}",
        mr.material.uv_offset.x
    );
    assert!((mr.material.uv_offset.y - 0.5).abs() < 1e-4);
    assert_eq!(mr.material.uv_scale, grodot_core::math::Vec2::new(4.0, 2.0));
}

#[test]
fn animation_player_api_from_scripts() {
    let p = TempProject::new("anim");
    std::fs::create_dir_all(p.root.join("assets/models")).unwrap();
    let hero = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/character-demo/assets/models/hero.glb");
    std::fs::copy(hero, p.root.join("assets/models/hero.glb")).unwrap();
    p.write(
        "scripts/a.gro",
        r#"class A < Entity
  function ready()
    @clips = self.animation.clips().length()
    @has_run = self.animation.has_clip?("run")
    self.animation.play("wave", 0, false)
    @finished = nil
  end
  function update(delta)
    @clip = self.animation.current_clip
    @playing = self.animation.playing?()
  end
  function on_animation_finished(clip)
    @finished = clip
  end
end
"#,
    );
    p.write(
        "scripts/bad.gro",
        "class Bad < Entity\n  function ready()\n\n    self.animation.play(\"moonwalk\")\n  end\nend\n",
    );
    let mut scene = scene_with_scripts(&[("A", "scripts/a.gro"), ("Bad", "scripts/bad.gro")]);
    for e in scene.entities.iter_mut() {
        let mut mr = Component::new_default("MeshRenderer").unwrap();
        mr.set_field("mesh", FieldValue::Text("assets/models/hero.glb".into())).unwrap();
        e.add(mr);
        let mut ap = Component::new_default("AnimationPlayer").unwrap();
        ap.set_field("autoplay", FieldValue::Bool(false)).unwrap();
        e.add(ap);
    }
    let mut rt = Runtime::new(scene, Some(p.root.clone()), Default::default());
    rt.frame(0.1);
    assert!(matches!(field(&rt, "A", "clips"), Value::Number(n) if n == 4.0));
    assert!(matches!(field(&rt, "A", "has_run"), Value::Bool(true)));
    assert!(matches!(field(&rt, "A", "clip"), Value::Str(s) if &*s == "wave"));
    assert!(matches!(field(&rt, "A", "playing"), Value::Bool(true)));
    for _ in 0..20 {
        rt.frame(0.1); // wave is 1.2 s
    }
    assert!(matches!(field(&rt, "A", "finished"), Value::Str(s) if &*s == "wave"));
    assert!(matches!(field(&rt, "A", "playing"), Value::Bool(false)));
    let errs = errors(&rt);
    assert_eq!(errs.len(), 1, "{:?}", errs);
    assert!(errs[0].contains("no animation clip named 'moonwalk'") && errs[0].contains("run"), "{}", errs[0]);
    assert!(errs[0].contains("File: scripts/bad.gro") && errs[0].contains("Line: 4"), "{}", errs[0]);
}
