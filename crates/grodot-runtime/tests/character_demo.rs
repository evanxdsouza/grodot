//! Headless tests for skinned glTF animation: play the character demo without
//! a window and check the clip state, finished events and bone matrices.

use grodot_core::animation;
use grodot_core::config::Project;
use grodot_core::math::Mat4;
use grodot_core::scene::EntityId;
use grodot_runtime::{LogLevel, Runtime};
use groscript::Value;
use std::path::PathBuf;

const DT: f32 = 1.0 / 60.0;

fn runtime() -> Runtime {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/character-demo");
    let project = Project::open(&dir).expect("character demo opens");
    let mut rt = Runtime::from_project(&project).unwrap();
    rt.start();
    rt
}

fn run(rt: &mut Runtime, frames: usize) {
    for _ in 0..frames {
        rt.frame(DT);
    }
}

fn hero(rt: &Runtime) -> EntityId {
    rt.world.borrow().scene.find_by_name("Hero").unwrap().id
}

fn clip(rt: &Runtime) -> Option<String> {
    let w = rt.world.borrow();
    w.animations[&hero(rt)].clip_name().map(str::to_string)
}

fn joints(rt: &Runtime) -> Vec<Mat4> {
    grodot_runtime::animation::joint_matrices(&rt.world.borrow(), hero(rt), 0).expect("hero has a skin")
}

fn max_diff(a: &[Mat4], b: &[Mat4]) -> f32 {
    assert_eq!(a.len(), b.len());
    let mut d = 0.0f32;
    for (x, y) in a.iter().zip(b) {
        for c in 0..4 {
            for r in 0..4 {
                d = d.max((x.c[c][r] - y.c[c][r]).abs());
            }
        }
    }
    d
}

fn assert_no_errors(rt: &Runtime) {
    let w = rt.world.borrow();
    let errors: Vec<_> =
        w.log.entries.iter().filter(|e| e.level == LogLevel::Error).map(|e| e.message.clone()).collect();
    assert!(errors.is_empty(), "errors:\n{}", errors.join("\n\n"));
}

fn busy(rt: &Runtime) -> bool {
    let w = rt.world.borrow();
    match &w.script_objects[&hero(rt)] {
        Value::Object(o) => matches!(o.fields.borrow().get("busy"), Some(Value::Bool(true))),
        _ => panic!("not a script object"),
    }
}

#[test]
fn hero_autoplays_idle_and_the_pose_moves() {
    let mut rt = runtime();
    run(&mut rt, 1);
    assert_eq!(clip(&rt).as_deref(), Some("idle"));
    let a = joints(&rt);
    assert_eq!(a.len(), 11, "the hero skin has 11 joints");
    run(&mut rt, 30);
    let b = joints(&rt);
    assert!(max_diff(&a, &b) > 1e-3, "idle should move the bones");
    assert_no_errors(&rt);
}

#[test]
fn running_plays_run_and_bones_match_the_sampled_clip() {
    let mut rt = runtime();
    run(&mut rt, 20);
    rt.world.borrow_mut().input.press("D");
    run(&mut rt, 40); // well past the 0.2 s crossfade
    assert_eq!(clip(&rt).as_deref(), Some("run"));
    assert_no_errors(&rt);

    // The runtime's bone matrices must equal sampling the run clip directly
    // at the player's current time (the crossfade has finished).
    let w = rt.world.borrow();
    let st = &w.animations[&hero(&rt)];
    assert!(st.fade_from.is_none(), "crossfade should be done");
    let g = &st.gltf;
    let mut pose = animation::rest_pose(g);
    animation::apply(&g.animations[g.animation_index("run").unwrap()], st.time, &mut pose);
    let world = g.world_from_locals(&animation::local_matrices(&pose));
    let expected = animation::joint_matrices(g, 0, &world);
    let actual = grodot_runtime::animation::joint_matrices(&w, hero(&rt), 0).unwrap();
    assert!(max_diff(&expected, &actual) < 1e-5, "bones differ from the sampled clip");

    // And the run pose is not the rest pose: the thighs swing.
    let rest =
        animation::joint_matrices(g, 0, &g.world_from_locals(&animation::local_matrices(&animation::rest_pose(g))));
    assert!(max_diff(&rest, &actual) > 1e-2, "running should move the legs away from rest");
    drop(w);

    // Moving right turns the hero to face +X.
    let p = rt.world.borrow().scene.find_by_name("Hero").unwrap().transform().position;
    assert!(p.x > 1.0, "hero should have run right, x = {}", p.x);

    rt.world.borrow_mut().input.release("D");
    run(&mut rt, 2);
    assert_eq!(clip(&rt).as_deref(), Some("idle"));
}

#[test]
fn jump_is_one_shot_and_fires_on_animation_finished() {
    let mut rt = runtime();
    run(&mut rt, 30); // land on the ground first
    rt.world.borrow_mut().input.press("Space");
    run(&mut rt, 1);
    rt.world.borrow_mut().input.release("Space");
    assert_eq!(clip(&rt).as_deref(), Some("jump"));
    assert!(busy(&rt));
    let dur = rt.world.borrow().animations[&hero(&rt)].duration();
    // Playing the jump: still busy half way through.
    run(&mut rt, ((dur * 0.5) / DT) as usize);
    assert_eq!(clip(&rt).as_deref(), Some("jump"));
    assert!(busy(&rt));
    // After the clip ends, on_animation_finished clears @busy and idle resumes.
    run(&mut rt, ((dur * 0.5) / DT) as usize + 5);
    assert!(!busy(&rt), "on_animation_finished should have run");
    assert_eq!(clip(&rt).as_deref(), Some("idle"));
    assert_no_errors(&rt);
}
