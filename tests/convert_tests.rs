//! Integration tests: build minimal GLBs by hand and run the
//! real converter over them. Self-contained — no fixtures from
//! outside the tree.

use hotwire::config::{AssetClass, Config};
use hotwire::convert::{convert_file, ConvertError};
use std::path::PathBuf;

// ----------------------------------------------------------------
// A minimal hand-built GLB: one named node, one triangle, one
// material, positions + TEXCOORD_0 + u16 indices.
// ----------------------------------------------------------------

fn build_glb(extra_json: &str, with_skin: bool) -> Vec<u8> {
    // Binary payload: 3 positions (f32x9), 3 uvs (f32x6), 3 indices (u16x3).
    let mut bin: Vec<u8> = Vec::new();
    for f in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&f.to_le_bytes());
    }
    let uv_off = bin.len();
    for f in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0] {
        bin.extend_from_slice(&f.to_le_bytes());
    }
    let ix_off = bin.len();
    for i in [0u16, 1, 2] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let skin_json = if with_skin {
        r#","skins":[{"name":"rig","joints":[0]}]"#
    } else {
        ""
    };
    let json = format!(
        r#"{{
  "asset": {{"version": "2.0"}},
  "scene": 0,
  "scenes": [{{"nodes": [0]}}],
  "nodes": [{{"name": "Tri Test", "mesh": 0}}],
  "meshes": [{{"name": "TriMesh", "primitives": [{{"attributes": {{"POSITION": 0, "TEXCOORD_0": 1}}, "indices": 2, "material": 0}}]}}],
  "materials": [{{"name": "Raw Metal"}}],
  "accessors": [
    {{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0,0,0], "max": [1,1,0]}},
    {{"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC2"}},
    {{"bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR"}}
  ],
  "bufferViews": [
    {{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
    {{"buffer": 0, "byteOffset": {uv_off}, "byteLength": 24}},
    {{"buffer": 0, "byteOffset": {ix_off}, "byteLength": 6}}
  ],
  "buffers": [{{"byteLength": {}}}] {extra_json}{skin_json}
}}"#,
        bin.len()
    );
    let mut glb: Vec<u8> = Vec::new();
    let json_bytes = json.as_bytes();
    let mut json_chunk = json_bytes.to_vec();
    while json_chunk.len() % 4 != 0 {
        json_chunk.push(b' ');
    }
    let total = 12 + 8 + json_chunk.len() + 8 + bin.len();
    glb.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // 'glTF'
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json_chunk.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // 'JSON'
    glb.extend_from_slice(&json_chunk);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // 'BIN\0'
    glb.extend_from_slice(&bin);
    glb
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hotwire-test-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn converts_handbuilt_glb_with_axis_law() {
    let dir = temp_dir("basic");
    let glb_path = dir.join("tri.glb");
    std::fs::write(&glb_path, build_glb("", false)).unwrap();
    let out = dir.join("models");
    let report = convert_file(&glb_path, &out, &Config::default(), AssetClass::Prop).unwrap();
    assert_eq!(report.objects.len(), 1);
    let obj = &report.objects[0];
    assert_eq!(obj.name, "tri-test");
    assert_eq!(obj.tris, 1);
    let text = std::fs::read_to_string(out.join("tri-test.c35m")).unwrap();
    // Input Z-up triangle (0,0,0) (1,0,0) (0,1,0) becomes,
    // under rotate -90 about X: (0,0,0) (1,0,0) (0,0,-1),
    // with the flat normal pointing +Y.
    assert!(text.contains("v 0 0 -1"), "axis law broken:\n{text}");
    assert!(text.contains(" n 0 1 0"), "flat normal wrong:\n{text}");
    // Unmapped material: sanitized, and reported.
    assert!(text.contains("material 0 raw-metal"), "{text}");
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("Raw Metal") && w.contains("no mapping")));
    // Manifest line: name, source, tris, sha256 (64 hex).
    let manifest = std::fs::read_to_string(out.join("manifest.txt")).unwrap();
    let parts: Vec<&str> = manifest.split_whitespace().collect();
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[0], "tri-test");
    assert_eq!(parts[1], "tri.glb");
    assert_eq!(parts[2], "1");
    assert_eq!(parts[3].len(), 64);
    assert_eq!(parts[3], obj.sha256);
}

#[test]
fn deterministic_byte_identical_output() {
    let dir = temp_dir("determinism");
    let glb_path = dir.join("tri.glb");
    std::fs::write(&glb_path, build_glb("", false)).unwrap();
    let out1 = dir.join("m1");
    let out2 = dir.join("m2");
    convert_file(&glb_path, &out1, &Config::default(), AssetClass::Prop).unwrap();
    convert_file(&glb_path, &out2, &Config::default(), AssetClass::Prop).unwrap();
    let a = std::fs::read(out1.join("tri-test.c35m")).unwrap();
    let b = std::fs::read(out2.join("tri-test.c35m")).unwrap();
    assert_eq!(a, b, "same .glb in must give byte-identical .c35m out");
}

#[test]
fn skinned_input_is_refused_and_writes_nothing() {
    let dir = temp_dir("skin");
    let glb_path = dir.join("rigged.glb");
    std::fs::write(&glb_path, build_glb("", true)).unwrap();
    let out = dir.join("models");
    let err = convert_file(&glb_path, &out, &Config::default(), AssetClass::Character)
        .expect_err("skinned input must be refused");
    assert!(matches!(err, ConvertError::Skinned(_)), "{err:?}");
    assert_eq!(err.exit_code(), 3);
    assert!(
        !out.exists(),
        "a refusal writes nothing, not even the folder"
    );
}

#[test]
fn over_budget_is_refused_and_writes_nothing() {
    let dir = temp_dir("budget");
    let glb_path = dir.join("tri.glb");
    std::fs::write(&glb_path, build_glb("", false)).unwrap();
    // A config whose prop refuse budget is zero refuses even
    // one triangle — exercises the refusal path exactly.
    let cfg = Config::parse("[budgets.prop]\nrefuse = 0\n").unwrap();
    let out = dir.join("models");
    let err = convert_file(&glb_path, &out, &cfg, AssetClass::Prop)
        .expect_err("over-budget input must be refused");
    assert!(matches!(err, ConvertError::OverBudget { .. }), "{err:?}");
    assert_eq!(err.exit_code(), 2);
    assert!(
        !out.exists(),
        "a refusal writes nothing, not even the folder"
    );
}

#[test]
fn material_rename_mapping_applies() {
    let dir = temp_dir("rename");
    let glb_path = dir.join("tri.glb");
    std::fs::write(&glb_path, build_glb("", false)).unwrap();
    let cfg = Config::parse("[materials]\n\"Raw Metal\" = \"painted-metal\"\n").unwrap();
    let out = dir.join("models");
    let report = convert_file(&glb_path, &out, &cfg, AssetClass::Prop).unwrap();
    let text = std::fs::read_to_string(out.join("tri-test.c35m")).unwrap();
    assert!(text.contains("material 0 painted-metal"), "{text}");
    assert!(!report.warnings.iter().any(|w| w.contains("no mapping")));
}
