//! Particle emitters: rate, cap, bursts from scripts, lifetime, and
//! deterministic output.

use grodot_core::config::Project;
use grodot_core::scene::{Component, Scene};
use grodot_core::FieldValue;
use grodot_runtime::{particles, Runtime};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn demo() -> Runtime {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/particles-demo");
    let project = Project::open(&dir).expect("particles demo opens");
    let mut rt = Runtime::from_project(&project).unwrap();
    rt.start();
    rt
}

fn emitter_id(rt: &Runtime, name: &str) -> u64 {
    rt.world.borrow().scene.find_by_name(name).unwrap().id
}

fn run(rt: &mut Runtime, seconds: f32) {
    let frames = (seconds / (1.0 / 60.0)).round() as usize;
    for _ in 0..frames {
        rt.frame(1.0 / 60.0);
    }
}

fn count(rt: &Runtime, name: &str) -> usize {
    particles::live_count(&rt.world.borrow(), emitter_id(rt, name))
}

#[test]
fn flames_keep_a_steady_population() {
    let mut rt = demo();
    run(&mut rt, 1.0);
    // Rate 45 per second; lifetime 0.8 s shortened by up to 40% (mean 0.64 s),
    // so about 45 x 0.64 = 29 are alive in steady state (cap 96).
    let n = count(&rt, "Flame 1");
    assert!((24..=36).contains(&n), "steady flame population, got {}", n);
    run(&mut rt, 2.0);
    let later = count(&rt, "Flame 1");
    assert!((24..=36).contains(&later), "still steady after 3 s, got {}", later);
    assert!(rt.world.borrow().log.entries.iter().all(|e| e.level != grodot_runtime::LogLevel::Error));
}

#[test]
fn explosion_burst_from_a_script_then_fades_out() {
    let mut rt = demo();
    run(&mut rt, 0.1);
    assert_eq!(count(&rt, "Explosion"), 0, "the explosion waits for E");
    rt.world.borrow_mut().input.press("E");
    rt.frame(1.0 / 60.0);
    rt.world.borrow_mut().input.end_frame();
    let burst = count(&rt, "Explosion");
    assert_eq!(burst, 120, "emit_burst(120) spawns all 120 at once");
    // Lifetime is 1.2 s (plus up to 30% shorter): all gone after 2 s.
    run(&mut rt, 2.0);
    assert_eq!(count(&rt, "Explosion"), 0, "the burst has died out");
}

#[test]
fn max_particles_caps_the_population() {
    let mut scene = Scene::new("Cap");
    let id = scene.add_entity("Spray");
    let mut c = Component::new_default("ParticleEmitter").unwrap();
    c.set_field("rate", FieldValue::Float(1000.0)).unwrap();
    c.set_field("max_particles", FieldValue::Float(50.0)).unwrap();
    c.set_field("lifetime", FieldValue::Float(5.0)).unwrap();
    scene.entity_mut(id).unwrap().add(c);
    let mut rt = Runtime::new(scene, None, BTreeMap::new());
    rt.start();
    run(&mut rt, 1.0);
    assert_eq!(particles::live_count(&rt.world.borrow(), id), 50);
    let sprites = particles::sprites(&rt.world.borrow());
    assert_eq!(sprites.len(), 50);
    assert!(sprites.iter().all(|s| s.texture.is_none()), "no texture means a plain quad");
}

#[test]
fn stop_and_start_from_scripts_pause_emission() {
    let mut rt = demo();
    run(&mut rt, 0.5);
    {
        let mut w = rt.world.borrow_mut();
        let e = w.scene.find_by_name("Flame 2").unwrap().id;
        if let Some(Component::ParticleEmitter(p)) = w.scene.entity_mut(e).unwrap().get_mut("ParticleEmitter") {
            p.playing = false;
        }
    }
    run(&mut rt, 2.0);
    assert_eq!(count(&rt, "Flame 2"), 0, "a stopped emitter lets its particles die");
}

#[test]
fn particle_output_is_deterministic() {
    let snapshot = |rt: &Runtime| -> Vec<(f32, f32, f32)> {
        particles::sprites(&rt.world.borrow()).iter().map(|s| (s.pos.x, s.pos.y, s.pos.z)).collect()
    };
    let mut a = demo();
    let mut b = demo();
    run(&mut a, 1.5);
    run(&mut b, 1.5);
    assert_eq!(snapshot(&a), snapshot(&b));
}
