//! Load every model of the sample kit (`tools/gen_gltf_kit.py`) and check the
//! parts each file is meant to exercise.

use grodot_core::gltf::{self, PrimitiveKind};
use grodot_core::math::Vec3;
use std::path::PathBuf;

fn load(name: &str) -> gltf::Gltf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/gltf-demo/assets/models").join(name);
    gltf::load(&p).unwrap_or_else(|e| panic!("{}: {}", name, e))
}

#[test]
fn house_glb_has_textures_hierarchy_and_a_matrix_node() {
    let g = load("house.glb");
    assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    assert_eq!(g.nodes.len(), 6);
    assert_eq!(g.roots.len(), 1);
    assert_eq!(g.images.iter().filter(|i| i.is_some()).count(), 2, "embedded PNGs decode");
    let walls = g.materials.iter().find(|m| m.name == "Walls").unwrap();
    assert!(walls.base_color_texture.is_some());
    let world = g.node_world_matrices();
    let chimney = g.nodes.iter().position(|n| n.name == "chimney").unwrap();
    assert!(g.nodes[chimney].matrix.is_some());
    let p = world[chimney].get_translation();
    assert!((p - Vec3 { x: 0.7, y: 2.0, z: -0.4 }).length() < 1e-5, "{:?}", p);
    let (lo, hi) = g.bounds();
    assert!(lo.y >= -1e-4 && hi.y > 2.5, "house sits on the ground and has a roof: {:?} {:?}", lo, hi);
}

#[test]
fn fence_has_a_line_primitive() {
    let g = load("fence.glb");
    let lines: Vec<_> =
        g.placed_primitives().into_iter().filter(|p| p.primitive.kind == PrimitiveKind::Lines).collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].primitive.mesh.indices.len(), 6, "a 4-point line strip is 3 segments");
}

#[test]
fn external_and_embedded_buffers() {
    let lamp = load("lamp_post.gltf");
    assert_eq!(lamp.nodes.len(), 3);
    let glass = lamp.materials.iter().find(|m| m.name == "Glass").unwrap();
    assert!(glass.emissive.r > 0.9, "emissive factor is read");
    let bush = load("bush.gltf");
    assert_eq!(bush.meshes.len(), 3);
    assert!(bush.triangle_count() > 50);
}

#[test]
fn trees_and_rocks_use_trs_transforms() {
    let rocks = load("rocks.glb");
    let world = rocks.node_world_matrices();
    let r1 = rocks.nodes.iter().position(|n| n.name == "rock1").unwrap();
    assert!((world[r1].get_translation() - Vec3 { x: 0.7, y: 0.0, z: 0.3 }).length() < 1e-5);
    for name in ["pine_tree.glb", "round_tree.glb", "crate.glb"] {
        let g = load(name);
        assert!(g.triangle_count() > 0, "{}", name);
        for p in g.placed_primitives() {
            let m = &p.primitive.mesh;
            assert_eq!(m.positions.len(), m.normals.len());
            assert_eq!(m.positions.len(), m.uvs.len());
        }
    }
}
