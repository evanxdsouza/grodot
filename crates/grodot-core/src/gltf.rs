//! glTF 2.0 loader (`.gltf` with external or embedded buffers, and binary `.glb`).
//!
//! Supported: triangle, strip, fan and line primitives; POSITION, NORMAL,
//! TEXCOORD_0 (any component type, normalized or not); node hierarchies with
//! TRS or matrix transforms; PBR metallic-roughness base colour factor and
//! texture (PNG or BMP images), alpha mode and double-sidedness.
//!
//! Not supported (reported as errors or warnings): Draco/meshopt compression,
//! KTX2/JPEG images, sparse accessors, morph targets.

use crate::image::{self, Image};
use crate::json::{self, Json};
use crate::math::{Color, Mat4, Quat, Vec2, Vec3};
use crate::mesh::Mesh;
use crate::scene::{AlphaMode, TextureFilter, TextureWrap};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct GltfError(pub String);

impl std::fmt::Display for GltfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

fn bad<T>(m: impl Into<String>) -> Result<T, GltfError> {
    Err(GltfError(m.into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveKind {
    Triangles,
    Lines,
}

#[derive(Debug, Clone)]
pub struct Primitive {
    /// For triangles: positions/normals/uvs/indices. For lines: `indices` are pairs.
    pub mesh: Mesh,
    pub kind: PrimitiveKind,
    pub material: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct GltfMesh {
    pub name: String,
    pub primitives: Vec<Primitive>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub mesh: Option<usize>,
    pub children: Vec<usize>,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// Set when the node uses `matrix` instead of TRS (cannot be animated).
    pub matrix: Option<Mat4>,
    pub skin: Option<usize>,
}

impl Node {
    pub fn local_matrix(&self) -> Mat4 {
        match self.matrix {
            Some(m) => m,
            None => Mat4::from_trs(self.translation, self.rotation, self.scale),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextureRef {
    pub image: usize,
    pub filter: TextureFilter,
    pub wrap: TextureWrap,
}

#[derive(Debug, Clone)]
pub struct GltfMaterial {
    pub name: String,
    pub base_color: Color,
    pub base_color_texture: Option<TextureRef>,
    pub emissive: Color,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
}

impl Default for GltfMaterial {
    fn default() -> Self {
        GltfMaterial {
            name: String::new(),
            base_color: Color::WHITE,
            base_color_texture: None,
            emissive: Color::BLACK,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Gltf {
    pub meshes: Vec<GltfMesh>,
    pub nodes: Vec<Node>,
    /// Root nodes of the default scene.
    pub roots: Vec<usize>,
    pub materials: Vec<GltfMaterial>,
    /// Decoded images (`None` when an image could not be decoded; see `warnings`).
    pub images: Vec<Option<Image>>,
    /// Non-fatal problems (unsupported image formats, ignored extensions).
    pub warnings: Vec<String>,
    pub skins: Vec<Skin>,
    pub animations: Vec<Animation>,
}

/// A skin: which nodes act as joints and how each joint maps from bind pose.
#[derive(Debug, Clone)]
pub struct Skin {
    pub name: String,
    /// Node index of each joint (JOINTS_0 values index into this list).
    pub joints: Vec<usize>,
    /// One per joint (identity when the file has none).
    pub inverse_bind: Vec<Mat4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimPath {
    Translation,
    Rotation,
    Scale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Step,
    CubicSpline,
}

/// Keyframes for one node property. For CUBICSPLINE, `values` holds
/// (in-tangent, value, out-tangent) per key, as in the glTF file.
#[derive(Debug, Clone)]
pub struct Channel {
    pub node: usize,
    pub path: AnimPath,
    pub interpolation: Interpolation,
    pub times: Vec<f32>,
    /// Flattened components: 3 per key (translation/scale) or 4 (rotation x, y, z, w).
    pub values: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub channels: Vec<Channel>,
    /// Length in seconds (the last keyframe time of any channel).
    pub duration: f32,
}

impl Gltf {
    pub fn animation_index(&self, name: &str) -> Option<usize> {
        self.animations.iter().position(|a| a.name == name)
    }
}

/// A mesh primitive placed in the model's space (all node transforms applied).
pub struct PlacedPrimitive<'a> {
    pub primitive: &'a Primitive,
    pub node: usize,
    pub transform: Mat4,
}

impl Gltf {
    /// World matrices of every node for the default scene, using each node's own TRS.
    pub fn node_world_matrices(&self) -> Vec<Mat4> {
        let locals: Vec<Mat4> = self.nodes.iter().map(|n| n.local_matrix()).collect();
        self.world_from_locals(&locals)
    }

    /// World matrices from per-node local matrices (used by animation).
    pub fn world_from_locals(&self, locals: &[Mat4]) -> Vec<Mat4> {
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        let mut stack: Vec<(usize, Mat4)> = self.roots.iter().map(|&r| (r, Mat4::IDENTITY)).collect();
        let mut guard = 0;
        while let Some((n, parent)) = stack.pop() {
            guard += 1;
            if guard > 100_000 || n >= self.nodes.len() {
                break;
            }
            let w = parent * locals[n];
            world[n] = w;
            for &c in &self.nodes[n].children {
                stack.push((c, w));
            }
        }
        world
    }

    /// Every mesh primitive of the default scene with its model-space transform.
    pub fn placed_primitives(&self) -> Vec<PlacedPrimitive<'_>> {
        let world = self.node_world_matrices();
        let mut out = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some(m) = node.mesh {
                if !self.in_scene(i) {
                    continue;
                }
                if let Some(mesh) = self.meshes.get(m) {
                    for p in &mesh.primitives {
                        out.push(PlacedPrimitive { primitive: p, node: i, transform: world[i] });
                    }
                }
            }
        }
        out
    }

    fn in_scene(&self, node: usize) -> bool {
        // A node is in the scene if it is reachable from a root.
        let mut stack = self.roots.clone();
        let mut seen = vec![false; self.nodes.len()];
        while let Some(n) = stack.pop() {
            if n >= self.nodes.len() || seen[n] {
                continue;
            }
            if n == node {
                return true;
            }
            seen[n] = true;
            stack.extend(self.nodes[n].children.iter().copied());
        }
        false
    }

    /// Axis-aligned bounds of the default scene in model space.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for p in self.placed_primitives() {
            for v in &p.primitive.mesh.positions {
                let w = p.transform.transform_point(*v);
                lo = lo.min(w);
                hi = hi.max(w);
            }
        }
        if lo.x > hi.x {
            (Vec3::ZERO, Vec3::ZERO)
        } else {
            (lo, hi)
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.placed_primitives()
            .iter()
            .filter(|p| p.primitive.kind == PrimitiveKind::Triangles)
            .map(|p| p.primitive.mesh.indices.len() / 3)
            .sum()
    }
}

// ------------------------------------------------------------------ loading

/// Load a `.gltf` or `.glb` file. External buffers and images are read from
/// the file's folder (and may not leave it).
pub fn load(path: &Path) -> Result<Gltf, GltfError> {
    let bytes = std::fs::read(path).map_err(|e| GltfError(format!("cannot read {}: {}", path.display(), e)))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    parse(&bytes, Some(dir))
}

/// Parse glTF JSON or GLB bytes. `base_dir` resolves external URIs (None forbids them).
pub fn parse(bytes: &[u8], base_dir: Option<&Path>) -> Result<Gltf, GltfError> {
    let (json_text, bin) = if bytes.starts_with(b"glTF") {
        split_glb(bytes)?
    } else {
        (std::str::from_utf8(bytes).map_err(|_| GltfError("glTF JSON is not valid UTF-8".into()))?, None)
    };
    let doc = json::parse(json_text).map_err(|e| GltfError(format!("invalid glTF JSON: {}", e)))?;
    Loader { doc: &doc, bin, base_dir, buffers: Vec::new() }.load()
}

fn split_glb(bytes: &[u8]) -> Result<(&str, Option<&[u8]>), GltfError> {
    let rd = |o: usize| -> Result<u32, GltfError> {
        bytes
            .get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| GltfError("GLB file is truncated".into()))
    };
    if rd(4)? != 2 {
        return bad(format!("unsupported GLB version {} (Grodot reads glTF 2.0)", rd(4)?));
    }
    let total = (rd(8)? as usize).min(bytes.len());
    let mut pos = 12;
    let mut json = None;
    let mut bin = None;
    while pos + 8 <= total {
        let len = rd(pos)? as usize;
        let ty = rd(pos + 4)?;
        let body = bytes.get(pos + 8..pos + 8 + len).ok_or_else(|| GltfError("GLB chunk is truncated".into()))?;
        match ty {
            0x4E4F534A => {
                json = Some(std::str::from_utf8(body).map_err(|_| GltfError("GLB JSON is not UTF-8".into()))?)
            }
            0x004E4942 => bin = Some(body),
            _ => {}
        }
        pos += 8 + len;
    }
    Ok((json.ok_or_else(|| GltfError("GLB file has no JSON chunk".into()))?, bin))
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>, GltfError> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut n = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return bad("invalid base64 data in a data: URI"),
        } as u32;
        acc = (acc << 6) | v;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            acc = 0;
            n = 0;
        }
    }
    match n {
        2 => out.push((acc >> 4) as u8),
        3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
        _ => {}
    }
    Ok(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Loader<'a> {
    doc: &'a Json,
    bin: Option<&'a [u8]>,
    base_dir: Option<&'a Path>,
    buffers: Vec<Vec<u8>>,
}

fn arr<'j>(j: &'j Json, key: &str) -> &'j [Json] {
    j.get(key).and_then(|v| v.as_array()).map(|v| v.as_slice()).unwrap_or(&[])
}

fn num(j: &Json, key: &str) -> Option<f64> {
    j.get(key).and_then(|v| v.as_f64())
}

fn idx(j: &Json, key: &str) -> Option<usize> {
    num(j, key).filter(|n| *n >= 0.0).map(|n| n as usize)
}

impl<'a> Loader<'a> {
    fn read_uri(&self, uri: &str) -> Result<Vec<u8>, GltfError> {
        if let Some(rest) = uri.strip_prefix("data:") {
            let comma = rest.find(',').ok_or_else(|| GltfError("malformed data: URI".into()))?;
            let (meta, data) = (&rest[..comma], &rest[comma + 1..]);
            if !meta.ends_with(";base64") {
                return bad("only base64 data: URIs are supported");
            }
            return base64_decode(data);
        }
        let dir = self.base_dir.ok_or_else(|| GltfError(format!("external file '{}' is not allowed here", uri)))?;
        let rel = percent_decode(uri);
        let p = Path::new(&rel);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return bad(format!("glTF file reference '{}' must stay inside the model's folder", uri));
        }
        std::fs::read(dir.join(p)).map_err(|e| GltfError(format!("cannot read '{}': {}", uri, e)))
    }

    fn load(mut self) -> Result<Gltf, GltfError> {
        let doc = self.doc;
        let version = doc.get("asset").and_then(|a| a.get("version")).and_then(|v| v.as_str()).unwrap_or("");
        if !version.starts_with('2') {
            return bad(format!("unsupported glTF version '{}' (Grodot reads glTF 2.0)", version));
        }
        if let Some(ext) = arr(doc, "extensionsRequired").iter().next() {
            let e = ext.as_str().unwrap_or("?");
            return bad(format!(
                "this model requires the glTF extension {} (not supported; re-export without compression)",
                e
            ));
        }
        let mut warnings = Vec::new();
        // Buffers
        for (i, b) in arr(doc, "buffers").iter().enumerate() {
            let data = match b.get("uri").and_then(|u| u.as_str()) {
                Some(uri) => self.read_uri(uri)?,
                None => self
                    .bin
                    .ok_or_else(|| GltfError(format!("buffer {} has no uri and there is no GLB binary chunk", i)))?
                    .to_vec(),
            };
            let need = num(b, "byteLength").unwrap_or(0.0) as usize;
            if data.len() < need {
                return bad(format!("buffer {} is shorter than its byteLength ({} < {})", i, data.len(), need));
            }
            self.buffers.push(data);
        }
        // Images
        let mut images = Vec::new();
        for (i, im) in arr(doc, "images").iter().enumerate() {
            let bytes = if let Some(uri) = im.get("uri").and_then(|u| u.as_str()) {
                self.read_uri(uri)
            } else if let Some(bv) = idx(im, "bufferView") {
                self.buffer_view(bv).map(|b| b.to_vec())
            } else {
                bad("image has neither uri nor bufferView")
            };
            match bytes.and_then(|b| image::decode(&b).map_err(|e| GltfError(e.0))) {
                Ok(img) => images.push(Some(img)),
                Err(e) => {
                    warnings.push(format!("image {}: {} (the texture is ignored)", i, e));
                    images.push(None);
                }
            }
        }
        // Samplers + textures
        let samplers: Vec<(TextureFilter, TextureWrap)> = arr(doc, "samplers")
            .iter()
            .map(|s| {
                let mag = num(s, "magFilter").unwrap_or(9729.0) as u32;
                let min = num(s, "minFilter").unwrap_or(9987.0) as u32;
                let filter = if mag == 9728 {
                    TextureFilter::Nearest
                } else if matches!(min, 9984..=9987) {
                    TextureFilter::Trilinear
                } else {
                    TextureFilter::Bilinear
                };
                let wrap = if num(s, "wrapS").unwrap_or(10497.0) as u32 == 33071 {
                    TextureWrap::Clamp
                } else {
                    TextureWrap::Repeat
                };
                (filter, wrap)
            })
            .collect();
        let textures: Vec<Option<TextureRef>> = arr(doc, "textures")
            .iter()
            .map(|t| {
                let image = idx(t, "source")?;
                let (filter, wrap) = idx(t, "sampler")
                    .and_then(|s| samplers.get(s).copied())
                    .unwrap_or((TextureFilter::Trilinear, TextureWrap::Repeat));
                Some(TextureRef { image, filter, wrap })
            })
            .collect();
        // Materials
        let mut materials = Vec::new();
        for m in arr(doc, "materials") {
            let mut mat = GltfMaterial {
                name: m.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
                ..Default::default()
            };
            if let Some(pbr) = m.get("pbrMetallicRoughness") {
                let f = arr(pbr, "baseColorFactor");
                if f.len() == 4 {
                    let c = |i: usize| f[i].as_f64().unwrap_or(1.0) as f32;
                    mat.base_color = Color { r: c(0), g: c(1), b: c(2), a: c(3) };
                }
                if let Some(t) = pbr.get("baseColorTexture") {
                    if num(t, "texCoord").unwrap_or(0.0) != 0.0 {
                        warnings.push(format!("material '{}': only TEXCOORD_0 is supported", mat.name));
                    }
                    mat.base_color_texture = idx(t, "index").and_then(|i| textures.get(i).cloned().flatten());
                }
            }
            let e = arr(m, "emissiveFactor");
            if e.len() == 3 {
                let c = |i: usize| e[i].as_f64().unwrap_or(0.0) as f32;
                mat.emissive = Color { r: c(0), g: c(1), b: c(2), a: 1.0 };
            }
            mat.alpha_mode = match m.get("alphaMode").and_then(|v| v.as_str()).unwrap_or("OPAQUE") {
                "MASK" => AlphaMode::Cutout,
                "BLEND" => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            };
            mat.alpha_cutoff = num(m, "alphaCutoff").unwrap_or(0.5) as f32;
            mat.double_sided = m.get("doubleSided").and_then(|v| v.as_bool()).unwrap_or(false);
            materials.push(mat);
        }
        // Meshes
        let mut meshes = Vec::new();
        for (mi, m) in arr(doc, "meshes").iter().enumerate() {
            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut prims = Vec::new();
            for p in arr(m, "primitives") {
                match self.primitive(p) {
                    Ok(Some(pr)) => prims.push(pr),
                    Ok(None) => warnings.push(format!("mesh {} ('{}'): point primitives are skipped", mi, name)),
                    Err(e) => return bad(format!("mesh {} ('{}'): {}", mi, name, e)),
                }
            }
            meshes.push(GltfMesh { name, primitives: prims });
        }
        // Nodes
        let mut nodes = Vec::new();
        for (ni, n) in arr(doc, "nodes").iter().enumerate() {
            let floats = |key: &str, len: usize| -> Result<Option<Vec<f32>>, GltfError> {
                let a = arr(n, key);
                if a.is_empty() {
                    return Ok(None);
                }
                if a.len() != len {
                    return bad(format!("node {} {} must have {} numbers", ni, key, len));
                }
                Ok(Some(a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect()))
            };
            let mut node = Node {
                name: n.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                mesh: idx(n, "mesh"),
                children: arr(n, "children").iter().filter_map(|c| c.as_f64()).map(|c| c as usize).collect(),
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
                matrix: None,
                skin: idx(n, "skin"),
            };
            if let Some(t) = floats("translation", 3)? {
                node.translation = Vec3::from_array([t[0], t[1], t[2]]);
            }
            if let Some(r) = floats("rotation", 4)? {
                node.rotation = Quat { x: r[0], y: r[1], z: r[2], w: r[3] }.normalized();
            }
            if let Some(s) = floats("scale", 3)? {
                node.scale = Vec3::from_array([s[0], s[1], s[2]]);
            }
            if let Some(m) = floats("matrix", 16)? {
                let mut mat = Mat4::IDENTITY;
                for c in 0..4 {
                    for r in 0..4 {
                        mat.c[c][r] = m[c * 4 + r];
                    }
                }
                if mat != Mat4::IDENTITY {
                    node.matrix = Some(mat);
                }
            }
            if node.mesh.map(|m| m >= meshes.len()).unwrap_or(false) {
                return bad(format!("node {} refers to missing mesh {}", ni, node.mesh.unwrap()));
            }
            nodes.push(node);
        }
        for (ni, n) in nodes.iter().enumerate() {
            if n.children.iter().any(|&c| c >= nodes.len() || c == ni) {
                return bad(format!("node {} has an invalid child index", ni));
            }
        }
        // Default scene roots (or every parentless node when there is no scene).
        let scene_index = idx(doc, "scene").unwrap_or(0);
        let mut roots: Vec<usize> = match arr(doc, "scenes").get(scene_index) {
            Some(s) => arr(s, "nodes")
                .iter()
                .filter_map(|v| v.as_f64())
                .map(|v| v as usize)
                .filter(|&v| v < nodes.len())
                .collect(),
            None => Vec::new(),
        };
        if roots.is_empty() {
            let mut has_parent = vec![false; nodes.len()];
            for n in &nodes {
                for &c in &n.children {
                    has_parent[c] = true;
                }
            }
            roots = (0..nodes.len()).filter(|&i| !has_parent[i]).collect();
        }
        if let Some(used) = doc.get("extensionsUsed").and_then(|v| v.as_array()) {
            for e in used {
                if let Some(e) = e.as_str() {
                    warnings.push(format!("extension {} is ignored", e));
                }
            }
        }
        let skins = self.skins(&nodes)?;
        let animations = self.animations(nodes.len(), &mut warnings)?;
        Ok(Gltf { meshes, nodes, roots, materials, images, warnings, skins, animations })
    }

    fn skins(&self, nodes: &[Node]) -> Result<Vec<Skin>, GltfError> {
        let mut out = Vec::new();
        for (si, s) in arr(self.doc, "skins").iter().enumerate() {
            let joints: Vec<usize> = arr(s, "joints").iter().filter_map(|j| j.as_f64()).map(|j| j as usize).collect();
            if joints.iter().any(|&j| j >= nodes.len()) {
                return bad(format!("skin {} refers to a missing joint node", si));
            }
            let inverse_bind = match idx(s, "inverseBindMatrices") {
                Some(a) => {
                    let (v, comps) = self.accessor_f32(a)?;
                    if comps != 16 || v.len() / 16 < joints.len() {
                        return bad(format!("skin {} inverseBindMatrices must hold one MAT4 per joint", si));
                    }
                    v.chunks_exact(16)
                        .take(joints.len())
                        .map(|m| {
                            let mut mat = Mat4::IDENTITY;
                            for c in 0..4 {
                                for r in 0..4 {
                                    mat.c[c][r] = m[c * 4 + r];
                                }
                            }
                            mat
                        })
                        .collect()
                }
                None => vec![Mat4::IDENTITY; joints.len()],
            };
            out.push(Skin {
                name: s.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                joints,
                inverse_bind,
            });
        }
        Ok(out)
    }

    fn animations(&self, node_count: usize, warnings: &mut Vec<String>) -> Result<Vec<Animation>, GltfError> {
        let mut out = Vec::new();
        for (ai, a) in arr(self.doc, "animations").iter().enumerate() {
            let name =
                a.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or(format!("animation_{}", ai));
            let samplers = arr(a, "samplers");
            let mut channels = Vec::new();
            let mut duration = 0.0f32;
            for c in arr(a, "channels") {
                let Some(target) = c.get("target") else { continue };
                let Some(node) = idx(target, "node") else { continue };
                if node >= node_count {
                    return bad(format!("animation '{}' targets a missing node", name));
                }
                let path = match target.get("path").and_then(|v| v.as_str()).unwrap_or("") {
                    "translation" => AnimPath::Translation,
                    "rotation" => AnimPath::Rotation,
                    "scale" => AnimPath::Scale,
                    "weights" => {
                        warnings.push(format!("animation '{}': morph target weights are not supported", name));
                        continue;
                    }
                    p => return bad(format!("animation '{}' has unknown target path '{}'", name, p)),
                };
                let samp = idx(c, "sampler")
                    .and_then(|i| samplers.get(i))
                    .ok_or_else(|| GltfError(format!("animation '{}' channel refers to a missing sampler", name)))?;
                let interpolation = match samp.get("interpolation").and_then(|v| v.as_str()).unwrap_or("LINEAR") {
                    "STEP" => Interpolation::Step,
                    "CUBICSPLINE" => Interpolation::CubicSpline,
                    _ => Interpolation::Linear,
                };
                let (times, _) =
                    self.accessor_f32(idx(samp, "input").ok_or_else(|| GltfError("sampler without input".into()))?)?;
                let (values, comps) =
                    self.accessor_f32(idx(samp, "output").ok_or_else(|| GltfError("sampler without output".into()))?)?;
                let want = if path == AnimPath::Rotation { 4 } else { 3 };
                let per_key = if interpolation == Interpolation::CubicSpline { 3 } else { 1 };
                if comps != want || values.len() != times.len() * want * per_key {
                    return bad(format!("animation '{}': keyframe count does not match its times", name));
                }
                if times.windows(2).any(|w| w[1] < w[0]) {
                    return bad(format!("animation '{}': keyframe times must increase", name));
                }
                duration = duration.max(times.last().copied().unwrap_or(0.0));
                channels.push(Channel { node, path, interpolation, times, values });
            }
            out.push(Animation { name, channels, duration });
        }
        Ok(out)
    }

    fn buffer_view(&self, i: usize) -> Result<&[u8], GltfError> {
        let bv = arr(self.doc, "bufferViews").get(i).ok_or_else(|| GltfError(format!("missing bufferView {}", i)))?;
        let buf = idx(bv, "buffer")
            .and_then(|b| self.buffers.get(b))
            .ok_or_else(|| GltfError(format!("bufferView {} refers to a missing buffer", i)))?;
        let off = idx(bv, "byteOffset").unwrap_or(0);
        let len = idx(bv, "byteLength").unwrap_or(0);
        buf.get(off..off + len).ok_or_else(|| GltfError(format!("bufferView {} is out of range", i)))
    }

    /// Read an accessor as f32 components (normalized integers scaled to 0..1 / -1..1).
    fn accessor_f32(&self, i: usize) -> Result<(Vec<f32>, usize), GltfError> {
        let a = arr(self.doc, "accessors").get(i).ok_or_else(|| GltfError(format!("missing accessor {}", i)))?;
        if a.get("sparse").is_some() {
            return bad("sparse accessors are not supported");
        }
        let count = idx(a, "count").ok_or_else(|| GltfError("accessor without count".into()))?;
        let comps = match a.get("type").and_then(|t| t.as_str()).unwrap_or("") {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            "MAT3" => 9,
            "MAT2" => 4,
            t => return bad(format!("unsupported accessor type '{}'", t)),
        };
        let ctype = num(a, "componentType").unwrap_or(0.0) as u32;
        let csize = match ctype {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return bad(format!("unsupported component type {}", ctype)),
        };
        let normalized = a.get("normalized").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut out = vec![0f32; count * comps];
        let Some(bv_i) = idx(a, "bufferView") else {
            return Ok((out, comps)); // all zeros per spec
        };
        let view = self.buffer_view(bv_i)?;
        let bv = &arr(self.doc, "bufferViews")[bv_i];
        let elem = comps * csize;
        let stride = idx(bv, "byteStride").filter(|&s| s > 0).unwrap_or(elem);
        let off = idx(a, "byteOffset").unwrap_or(0);
        if count > 0 && off + stride * (count - 1) + elem > view.len() {
            return bad(format!("accessor {} reads past the end of its buffer", i));
        }
        for k in 0..count {
            let base = off + k * stride;
            for c in 0..comps {
                let p = base + c * csize;
                let b = &view[p..p + csize];
                out[k * comps + c] = match ctype {
                    5120 => {
                        let v = b[0] as i8 as f32;
                        if normalized {
                            (v / 127.0).max(-1.0)
                        } else {
                            v
                        }
                    }
                    5121 => {
                        let v = b[0] as f32;
                        if normalized {
                            v / 255.0
                        } else {
                            v
                        }
                    }
                    5122 => {
                        let v = i16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized {
                            (v / 32767.0).max(-1.0)
                        } else {
                            v
                        }
                    }
                    5123 => {
                        let v = u16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized {
                            v / 65535.0
                        } else {
                            v
                        }
                    }
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                    _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                };
            }
        }
        Ok((out, comps))
    }

    fn accessor_indices(&self, i: usize) -> Result<Vec<u32>, GltfError> {
        let a = arr(self.doc, "accessors").get(i).ok_or_else(|| GltfError(format!("missing accessor {}", i)))?;
        let ctype = num(a, "componentType").unwrap_or(0.0) as u32;
        let count = idx(a, "count").unwrap_or(0);
        let bv_i = idx(a, "bufferView").ok_or_else(|| GltfError("index accessor without bufferView".into()))?;
        let view = self.buffer_view(bv_i)?;
        let off = idx(a, "byteOffset").unwrap_or(0);
        let size = match ctype {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => return bad(format!("indices must be unsigned integers (component type {})", ctype)),
        };
        if off + count * size > view.len() {
            return bad("index accessor reads past the end of its buffer");
        }
        Ok((0..count)
            .map(|k| {
                let p = off + k * size;
                match size {
                    1 => view[p] as u32,
                    2 => u16::from_le_bytes([view[p], view[p + 1]]) as u32,
                    _ => u32::from_le_bytes([view[p], view[p + 1], view[p + 2], view[p + 3]]),
                }
            })
            .collect())
    }

    fn primitive(&self, p: &Json) -> Result<Option<Primitive>, GltfError> {
        let mode = num(p, "mode").unwrap_or(4.0) as u32;
        let attrs = p.get("attributes").ok_or_else(|| GltfError("primitive has no attributes".into()))?;
        let pos_i = idx(attrs, "POSITION").ok_or_else(|| GltfError("primitive has no POSITION".into()))?;
        let (pos, pc) = self.accessor_f32(pos_i)?;
        if pc != 3 {
            return bad("POSITION must be VEC3");
        }
        let n = pos.len() / 3;
        let mut mesh = Mesh {
            positions: pos.chunks_exact(3).map(|c| Vec3 { x: c[0], y: c[1], z: c[2] }).collect(),
            ..Default::default()
        };
        if let Some(i) = idx(attrs, "NORMAL") {
            let (v, _) = self.accessor_f32(i)?;
            mesh.normals = v.chunks_exact(3).map(|c| Vec3 { x: c[0], y: c[1], z: c[2] }.normalized()).collect();
        }
        if let Some(i) = idx(attrs, "TEXCOORD_0") {
            let (v, _) = self.accessor_f32(i)?;
            mesh.uvs = v.chunks_exact(2).map(|c| Vec2::new(c[0], c[1])).collect();
        }
        if mesh.normals.len() != n {
            mesh.normals = vec![Vec3::ZERO; n];
        }
        mesh.ensure_uvs();
        if let (Some(ji), Some(wi)) = (idx(attrs, "JOINTS_0"), idx(attrs, "WEIGHTS_0")) {
            let (j, jc) = self.accessor_f32(ji)?;
            let (w, wc) = self.accessor_f32(wi)?;
            if jc != 4 || wc != 4 || j.len() != n * 4 || w.len() != n * 4 {
                return bad("JOINTS_0 and WEIGHTS_0 must be VEC4 with one entry per vertex");
            }
            mesh.joints = j.chunks_exact(4).map(|c| [c[0] as u16, c[1] as u16, c[2] as u16, c[3] as u16]).collect();
            mesh.weights = w
                .chunks_exact(4)
                .map(|c| {
                    let sum: f32 = c.iter().sum();
                    if sum > 1e-6 {
                        [c[0] / sum, c[1] / sum, c[2] / sum, c[3] / sum]
                    } else {
                        [1.0, 0.0, 0.0, 0.0]
                    }
                })
                .collect();
        }
        let raw: Vec<u32> = match idx(p, "indices") {
            Some(i) => self.accessor_indices(i)?,
            None => (0..n as u32).collect(),
        };
        if raw.iter().any(|&i| i as usize >= n) {
            return bad("an index is out of range");
        }
        let (kind, indices) = match mode {
            4 => (PrimitiveKind::Triangles, raw.chunks_exact(3).flatten().copied().collect()),
            5 => {
                let mut t = Vec::new();
                for k in 2..raw.len() {
                    if k % 2 == 0 {
                        t.extend_from_slice(&[raw[k - 2], raw[k - 1], raw[k]]);
                    } else {
                        t.extend_from_slice(&[raw[k - 1], raw[k - 2], raw[k]]);
                    }
                }
                (PrimitiveKind::Triangles, t)
            }
            6 => {
                let mut t = Vec::new();
                for k in 2..raw.len() {
                    t.extend_from_slice(&[raw[0], raw[k - 1], raw[k]]);
                }
                (PrimitiveKind::Triangles, t)
            }
            1 => (PrimitiveKind::Lines, raw.chunks_exact(2).flatten().copied().collect()),
            3 | 2 => {
                let mut l = Vec::new();
                for k in 1..raw.len() {
                    l.extend_from_slice(&[raw[k - 1], raw[k]]);
                }
                if mode == 2 && raw.len() > 2 {
                    l.extend_from_slice(&[*raw.last().unwrap(), raw[0]]);
                }
                (PrimitiveKind::Lines, l)
            }
            0 => return Ok(None),
            m => return bad(format!("unknown primitive mode {}", m)),
        };
        mesh.indices = indices;
        if kind == PrimitiveKind::Triangles && mesh.normals.iter().any(|n| n.length_sq() < 0.5) {
            // Missing normals: use face normals (flat shading).
            let mut flat = Mesh::default();
            for tri in mesh.indices.chunks_exact(3) {
                let (a, b, c) =
                    (mesh.positions[tri[0] as usize], mesh.positions[tri[1] as usize], mesh.positions[tri[2] as usize]);
                let nrm = (b - a).cross(c - a).normalized();
                for &i in tri {
                    flat.positions.push(mesh.positions[i as usize]);
                    flat.normals.push(nrm);
                    flat.uvs.push(mesh.uvs[i as usize]);
                    if !mesh.joints.is_empty() {
                        flat.joints.push(mesh.joints[i as usize]);
                        flat.weights.push(mesh.weights[i as usize]);
                    }
                    flat.indices.push(flat.positions.len() as u32 - 1);
                }
            }
            mesh = flat;
        }
        Ok(Some(Primitive { mesh, kind, material: idx(p, "material") }))
    }
}

/// True if a path looks like a glTF model.
pub fn is_model_path(p: &str) -> bool {
    let l = p.to_lowercase();
    l.ends_with(".gltf") || l.ends_with(".glb")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    /// A triangle (embedded buffer) placed by a child node under a translated parent.
    fn triangle_gltf() -> String {
        let mut buf = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for i in [0u16, 1, 2, 0] {
            buf.extend_from_slice(&i.to_le_bytes());
        }
        format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"name":"parent","translation":[10,0,0],"children":[1]}},{{"name":"child","mesh":0,"scale":[2,2,2]}}],
            "meshes":[{{"name":"tri","primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[1,0.5,0.25,1]}},"doubleSided":true,"alphaMode":"MASK","alphaCutoff":0.3}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3"}},{{"bufferView":1,"componentType":5123,"count":3,"type":"SCALAR"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":6}}],
            "buffers":[{{"byteLength":44,"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
            b64(&buf)
        )
    }

    #[test]
    fn base64_roundtrip() {
        for s in [&b""[..], b"a", b"ab", b"abc", b"hello world"] {
            assert_eq!(base64_decode(&b64(s)).unwrap(), s);
        }
    }

    #[test]
    fn parses_embedded_triangle_with_hierarchy() {
        let g = parse(triangle_gltf().as_bytes(), None).unwrap();
        assert_eq!(g.meshes[0].primitives.len(), 1);
        let p = &g.meshes[0].primitives[0];
        assert_eq!(p.kind, PrimitiveKind::Triangles);
        assert_eq!(p.mesh.indices.len(), 3);
        // Missing normals were generated (flat, +Z for this CCW triangle).
        assert!((p.mesh.normals[0] - Vec3::Z).length() < 1e-5);
        let placed = g.placed_primitives();
        assert_eq!(placed.len(), 1);
        let w = placed[0].transform.transform_point(Vec3 { x: 1.0, y: 0.0, z: 0.0 });
        assert!((w - Vec3 { x: 12.0, y: 0.0, z: 0.0 }).length() < 1e-5, "parent translate + child scale");
        let m = &g.materials[0];
        assert_eq!(m.base_color, Color { r: 1.0, g: 0.5, b: 0.25, a: 1.0 });
        assert!(m.double_sided);
        assert_eq!(m.alpha_mode, AlphaMode::Cutout);
        assert!((m.alpha_cutoff - 0.3).abs() < 1e-6);
        let (lo, hi) = g.bounds();
        assert!((lo.x - 10.0).abs() < 1e-5 && (hi.y - 2.0).abs() < 1e-5);
    }

    #[test]
    fn glb_container() {
        let json = triangle_gltf();
        let mut chunk = json.into_bytes();
        while !chunk.len().is_multiple_of(4) {
            chunk.push(b' ');
        }
        let mut glb = Vec::new();
        glb.extend_from_slice(b"glTF");
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&((12 + 8 + chunk.len()) as u32).to_le_bytes());
        glb.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
        glb.extend_from_slice(&chunk);
        let g = parse(&glb, None).unwrap();
        assert_eq!(g.triangle_count(), 1);
    }

    #[test]
    fn errors_are_reported_not_panics() {
        assert!(parse(b"{}", None).is_err(), "no asset.version");
        let draco = r#"{"asset":{"version":"2.0"},"extensionsRequired":["KHR_draco_mesh_compression"]}"#;
        assert!(parse(draco.as_bytes(), None).unwrap_err().0.contains("draco"));
        let bad_index = triangle_gltf().replace("\"count\":3,\"type\":\"SCALAR\"", "\"count\":30,\"type\":\"SCALAR\"");
        assert!(parse(bad_index.as_bytes(), None).is_err());
        let ext = triangle_gltf().replace("data:application/octet-stream;base64,", "../outside.bin#");
        assert!(parse(ext.as_bytes(), Some(Path::new("/tmp"))).is_err());
    }

    #[test]
    fn strips_fans_and_lines() {
        let src = triangle_gltf().replace("\"indices\":1,", "\"indices\":1,\"mode\":3,");
        let g = parse(src.as_bytes(), None).unwrap();
        let p = &g.meshes[0].primitives[0];
        assert_eq!(p.kind, PrimitiveKind::Lines);
        assert_eq!(p.mesh.indices, vec![0, 1, 1, 2]);
    }
}
