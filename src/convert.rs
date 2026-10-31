//! The converter: `.glb` in, `.c35m` out, era law enforced.
//!
//! Pipeline (SPEC: Conversion rules): parse with the `gltf`
//! crate, refuse skinned input outright, bake node transforms,
//! rotate Z-up → Y-up (−90° about X), triangulate, then build
//! one `.c35m` per top-level named object with flat per-face
//! normals, first-UV-layer UVs, and house material names.
//!
//! Everything is built and validated in memory first; a refusal
//! (skinning, over budget, bad geometry) writes NOTHING.

use crate::config::{AssetClass, Config};
use crate::model::{render_c35m, sanitize, OutFace, OutModel};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::Path;

// ----------------------------------------------------------------
// Errors and reports
// ----------------------------------------------------------------

#[derive(Debug)]
pub enum ConvertError {
    /// I/O failure or an unparseable interchange file.
    Read(String),
    /// Skinned input: refused by SPEC, never mangled.
    Skinned(Vec<String>),
    /// Era refusal: a model over its class refuse budget.
    OverBudget {
        model: String,
        tris: usize,
        refuse: usize,
        class: &'static str,
    },
    /// Nothing convertible in the file.
    Empty(String),
    /// Geometry the converter cannot honestly carry.
    Geometry(String),
}

impl ConvertError {
    pub fn exit_code(&self) -> i32 {
        match self {
            ConvertError::OverBudget { .. } => 2,
            ConvertError::Skinned(_) => 3,
            _ => 1,
        }
    }

    /// A refusal is a report, not a stack trace: this is the
    /// text the artist (and the watch log) reads.
    pub fn describe(&self) -> String {
        match self {
            ConvertError::Read(m) => format!("error: {m}"),
            ConvertError::Skinned(skins) => {
                let names = if skins.is_empty() {
                    "(unnamed)".to_string()
                } else {
                    skins.join(", ")
                };
                format!(
                    "REFUSED: skinned input (skins: {names}). Hotwire v1 carries static \
                     geometry only — skinning is out of scope per SPEC and is never \
                     silently dropped. Export an unskinned pose or wait for the \
                     skinning phase."
                )
            }
            ConvertError::OverBudget {
                model,
                tris,
                refuse,
                class,
            } => format!(
                "REFUSED: model '{model}' has {tris} triangles, over the {class} refuse \
                 budget of {refuse}. Nothing was written. Reduce the mesh (decimate, \
                 retopo) and export again — the banner standard is low triangles, \
                 high surface craft."
            ),
            ConvertError::Empty(m) => format!("error: nothing to convert: {m}"),
            ConvertError::Geometry(m) => format!("error: {m}"),
        }
    }
}

#[derive(Debug)]
pub struct ObjectReport {
    pub name: String,
    pub file_name: String,
    pub tris: usize,
    pub verts: usize,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub materials: Vec<String>,
    pub sha256: String,
}

#[derive(Debug)]
pub struct Report {
    pub source_name: String,
    pub objects: Vec<ObjectReport>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn total_tris(&self) -> usize {
        self.objects.iter().map(|o| o.tris).sum()
    }

    pub fn print(&self) {
        println!("source: {}", self.source_name);
        for o in &self.objects {
            let size = [
                o.bbox_max[0] - o.bbox_min[0],
                o.bbox_max[1] - o.bbox_min[1],
                o.bbox_max[2] - o.bbox_min[2],
            ];
            println!("object: {} -> {}", o.name, o.file_name);
            println!("  triangles: {}  vertices: {}", o.tris, o.verts);
            println!(
                "  bbox: {:.3} x {:.3} x {:.3} m (min {:.3}, {:.3}, {:.3})",
                size[0], size[1], size[2], o.bbox_min[0], o.bbox_min[1], o.bbox_min[2]
            );
            println!("  materials: {}", o.materials.join(", "));
            println!("  sha256: {}", o.sha256);
        }
        for w in &self.warnings {
            println!("  ! warning: {w}");
        }
        println!(
            "total: {} object(s), {} triangles",
            self.objects.len(),
            self.total_tris()
        );
    }
}

// ----------------------------------------------------------------
// Small matrix helpers (column-major, matching glTF)
// ----------------------------------------------------------------

type Mat4 = [[f32; 4]; 4];

const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `a ∘ b`: apply `b` first, then `a`.
fn mat_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0f32; 4]; 4];
    for col in 0..4 {
        for row in 0..4 {
            out[col][row] = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    out
}

fn apply_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

/// SPEC axis law: Blender Z-up → C35 Y-up, rotate −90° about X:
/// (x, y, z) → (x, z, −y).
fn z_up_to_y_up(p: [f32; 3]) -> [f32; 3] {
    [p[0], p[2], -p[1]]
}

// ----------------------------------------------------------------
// Geometry gathering from the glTF document
// ----------------------------------------------------------------

struct PrimGeom {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    tris: Vec<[u32; 3]>,
    material: Option<usize>,
}

fn read_primitive(
    prim: &gltf::Primitive,
    buffers: &[gltf::buffer::Data],
    world: &Mat4,
) -> Result<PrimGeom, ConvertError> {
    use gltf::mesh::Mode;
    let mode = prim.mode();
    if !matches!(
        mode,
        Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan
    ) {
        return Err(ConvertError::Geometry(format!(
            "unsupported primitive mode {mode:?} (only triangle modes cross in v1)"
        )));
    }
    let reader = prim.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
    let raw_positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or_else(|| ConvertError::Geometry("primitive has no POSITION attribute".into()))?
        .collect();
    for p in &raw_positions {
        if !p.iter().all(|x| x.is_finite()) {
            return Err(ConvertError::Geometry(
                "non-finite vertex position in input".into(),
            ));
        }
    }
    let positions: Vec<[f32; 3]> = raw_positions
        .iter()
        .map(|p| z_up_to_y_up(apply_point(world, *p)))
        .collect();
    let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
        Some(tc) => tc.into_f32().collect(),
        None => vec![[0.0, 0.0]; positions.len()],
    };
    if uvs.len() != positions.len() {
        return Err(ConvertError::Geometry(
            "TEXCOORD_0 count does not match POSITION count".into(),
        ));
    }
    let indices: Vec<u32> = match reader.read_indices() {
        Some(ix) => ix.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let n = positions.len() as u32;
    for i in &indices {
        if *i >= n {
            return Err(ConvertError::Geometry(
                "index out of range in primitive".into(),
            ));
        }
    }
    let mut tris = Vec::new();
    match mode {
        Mode::Triangles => {
            for t in indices.as_chunks::<3>().0 {
                tris.push(*t);
            }
        }
        Mode::TriangleStrip => {
            for i in 2..indices.len() {
                // Alternate winding so every strip triangle
                // faces the same way.
                if i % 2 == 0 {
                    tris.push([indices[i - 2], indices[i - 1], indices[i]]);
                } else {
                    tris.push([indices[i - 1], indices[i - 2], indices[i]]);
                }
            }
        }
        Mode::TriangleFan => {
            for i in 2..indices.len() {
                tris.push([indices[0], indices[i - 1], indices[i]]);
            }
        }
        _ => unreachable!(),
    }
    Ok(PrimGeom {
        positions,
        uvs,
        tris,
        material: prim.material().index(),
    })
}

/// Depth-first walk of one subtree, baking world transforms and
/// collecting every mesh primitive. `first_name` records the
/// first node name met (root first) for model naming.
fn walk_node(
    node: &gltf::Node,
    buffers: &[gltf::buffer::Data],
    parent: &Mat4,
    prims: &mut Vec<PrimGeom>,
    first_name: &mut Option<String>,
) -> Result<(), ConvertError> {
    if first_name.is_none() {
        *first_name = node.name().map(|s| s.to_string());
    }
    let world = mat_mul(parent, &node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            prims.push(read_primitive(&prim, buffers, &world)?);
        }
    }
    for child in node.children() {
        walk_node(&child, buffers, &world, prims, first_name)?;
    }
    Ok(())
}

// ----------------------------------------------------------------
// Assembling one .c35m model from gathered primitives
// ----------------------------------------------------------------

struct Assembled {
    model: OutModel,
    degenerate_skipped: usize,
}

fn assemble(
    name: String,
    prims: Vec<PrimGeom>,
    material_names: &[Option<String>],
    cfg: &Config,
    warnings: &mut Vec<String>,
) -> Assembled {
    let mut model = OutModel {
        name,
        ..Default::default()
    };
    let mut vert_ids: HashMap<[u32; 5], u32> = HashMap::new();
    let mut material_ids: HashMap<String, u16> = HashMap::new();
    let mut degenerate_skipped = 0usize;

    let intern_material = |model: &mut OutModel,
                           material_ids: &mut HashMap<String, u16>,
                           warnings: &mut Vec<String>,
                           src: Option<usize>|
     -> u16 {
        let source_name = match src {
            Some(idx) => material_names
                .get(idx)
                .cloned()
                .flatten()
                .unwrap_or_else(|| format!("material-{idx}")),
            None => "default".to_string(),
        };
        let (house_raw, mapped) = cfg.map_material(&source_name);
        let house = sanitize(&house_raw);
        let house = if house.is_empty() {
            "material".to_string()
        } else {
            house
        };
        if let Some(id) = material_ids.get(&house) {
            return *id;
        }
        if !mapped {
            warnings.push(format!(
                "material '{source_name}' has no mapping in hotwire.toml; kept as '{house}'"
            ));
        }
        if house != house_raw {
            warnings.push(format!(
                "material name '{house_raw}' sanitized to '{house}'"
            ));
        }
        let id = model.materials.len() as u16;
        model.materials.push(house.clone());
        material_ids.insert(house, id);
        id
    };

    for prim in &prims {
        let mat_id = intern_material(&mut model, &mut material_ids, warnings, prim.material);
        // Intern this primitive's vertices (position + UV is
        // the identity of a .c35m vertex).
        let base: Vec<u32> = prim
            .positions
            .iter()
            .zip(&prim.uvs)
            .map(|(p, uv)| {
                let key = [
                    p[0].to_bits(),
                    p[1].to_bits(),
                    p[2].to_bits(),
                    uv[0].to_bits(),
                    uv[1].to_bits(),
                ];
                if let Some(id) = vert_ids.get(&key) {
                    *id
                } else {
                    let id = model.verts.len() as u32;
                    model.verts.push(*p);
                    vert_ids.insert(key, id);
                    id
                }
            })
            .collect();
        for tri in &prim.tris {
            let (ia, ib, ic) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (pa, pb, pc) = (prim.positions[ia], prim.positions[ib], prim.positions[ic]);
            let e1 = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let e2 = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len <= 0.0 || !len.is_finite() {
                degenerate_skipped += 1;
                continue;
            }
            let normal = [n[0] / len, n[1] / len, n[2] / len];
            model.faces.push(OutFace {
                material: mat_id,
                corners: [
                    (base[ia], prim.uvs[ia]),
                    (base[ib], prim.uvs[ib]),
                    (base[ic], prim.uvs[ic]),
                ],
                normal,
            });
        }
    }
    Assembled {
        model,
        degenerate_skipped,
    }
}

// ----------------------------------------------------------------
// The conversion entry point
// ----------------------------------------------------------------

/// Convert one `.glb` file into `<out_dir>` (one `.c35m` per
/// top-level object + manifest lines appended). On any refusal
/// or error, nothing is written.
pub fn convert_file(
    input: &Path,
    out_dir: &Path,
    cfg: &Config,
    class: AssetClass,
) -> Result<Report, ConvertError> {
    let source_name = input
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.display().to_string());
    let (document, buffers, _images) = gltf::import(input)
        .map_err(|e| ConvertError::Read(format!("parsing {}: {e}", input.display())))?;

    // SPEC: skinning is out of scope for v1 — refuse, clearly.
    let mut skin_names: Vec<String> = document
        .skins()
        .map(|s| s.name().unwrap_or("(unnamed skin)").to_string())
        .collect();
    let mut has_joint_attrs = false;
    for mesh in document.meshes() {
        for prim in mesh.primitives() {
            if prim
                .attributes()
                .any(|(sem, _)| matches!(sem, gltf::Semantic::Joints(_)))
            {
                has_joint_attrs = true;
            }
        }
    }
    if !skin_names.is_empty() || has_joint_attrs {
        if skin_names.is_empty() {
            skin_names.push("(joint attributes, no skin object)".to_string());
        }
        return Err(ConvertError::Skinned(skin_names));
    }

    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or_else(|| ConvertError::Empty("document has no scene".into()))?;

    let material_names: Vec<Option<String>> = document
        .materials()
        .map(|m| m.name().map(|s| s.to_string()))
        .collect();

    let file_stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model".to_string());

    let mut warnings: Vec<String> = Vec::new();
    let mut built: Vec<OutModel> = Vec::new();
    let mut used_names: HashSet<String> = HashSet::new();

    for (root_idx, root) in scene.nodes().enumerate() {
        let mut prims = Vec::new();
        let mut first_name = None;
        walk_node(&root, &buffers, &IDENTITY, &mut prims, &mut first_name)?;
        if prims.iter().map(|p| p.tris.len()).sum::<usize>() == 0 {
            continue;
        }
        let mut name = first_name.as_deref().map(sanitize).unwrap_or_default();
        if name.is_empty() {
            name = sanitize(&file_stem);
        }
        if name.is_empty() {
            name = format!("model-{}", root_idx + 1);
        }
        if used_names.contains(&name) {
            let mut n = 2;
            while used_names.contains(&format!("{name}-{n}")) {
                n += 1;
            }
            name = format!("{name}-{n}");
        }
        used_names.insert(name.clone());
        let assembled = assemble(name, prims, &material_names, cfg, &mut warnings);
        if assembled.degenerate_skipped > 0 {
            warnings.push(format!(
                "model '{}': skipped {} degenerate (zero-area) triangle(s)",
                assembled.model.name, assembled.degenerate_skipped
            ));
        }
        built.push(assembled.model);
    }

    if built.is_empty() {
        return Err(ConvertError::Empty(
            "no mesh triangles found in the scene".into(),
        ));
    }

    // Era enforcement happens BEFORE anything touches the disk:
    // a refusal writes nothing.
    let budget = cfg.budget(class);
    for m in &built {
        if m.tris() > budget.refuse {
            return Err(ConvertError::OverBudget {
                model: m.name.clone(),
                tris: m.tris(),
                refuse: budget.refuse,
                class: class.key(),
            });
        }
        if m.tris() > budget.warn {
            warnings.push(format!(
                "model '{}': {} triangles is past the {} warn budget of {} (refuse at {})",
                m.name,
                m.tris(),
                class.key(),
                budget.warn,
                budget.refuse
            ));
        }
    }

    // Render everything, then write.
    let mut rendered: Vec<(String, String, String)> = Vec::new(); // (file_name, text, sha)
    for m in &built {
        let text = render_c35m(m);
        let digest = Sha256::digest(text.as_bytes());
        let sha: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        rendered.push((format!("{}.c35m", m.name), text, sha));
    }

    std::fs::create_dir_all(out_dir)
        .map_err(|e| ConvertError::Read(format!("creating {}: {e}", out_dir.display())))?;
    let mut objects = Vec::new();
    let mut manifest = String::new();
    for (m, (file_name, text, sha)) in built.iter().zip(&rendered) {
        std::fs::write(out_dir.join(file_name), text).map_err(|e| {
            ConvertError::Read(format!(
                "writing {}: {e}",
                out_dir.join(file_name).display()
            ))
        })?;
        manifest.push_str(&format!(
            "{} {} {} {}\n",
            m.name,
            source_name,
            m.tris(),
            sha
        ));
        let (bbox_min, bbox_max) = m.bbox();
        objects.push(ObjectReport {
            name: m.name.clone(),
            file_name: file_name.clone(),
            tris: m.tris(),
            verts: m.verts.len(),
            bbox_min,
            bbox_max,
            materials: m.materials.clone(),
            sha256: sha.clone(),
        });
    }
    let manifest_path = out_dir.join("manifest.txt");
    let mut existing = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .map_err(|e| ConvertError::Read(format!("reading manifest: {e}")))?
    } else {
        String::new()
    };
    existing.push_str(&manifest);
    std::fs::write(&manifest_path, existing)
        .map_err(|e| ConvertError::Read(format!("writing manifest: {e}")))?;

    Ok(Report {
        source_name,
        objects,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_law() {
        assert_eq!(z_up_to_y_up([1.0, 2.0, 3.0]), [1.0, 3.0, -2.0]);
        // A Blender floor (Z-up, normal +Z) becomes a C35 floor
        // (normal +Y).
        assert_eq!(z_up_to_y_up([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]);
    }

    #[test]
    fn matrix_composition_bakes_translation() {
        let mut t = IDENTITY;
        t[3] = [5.0, 6.0, 7.0, 1.0];
        let world = mat_mul(&IDENTITY, &t);
        assert_eq!(apply_point(&world, [1.0, 1.0, 1.0]), [6.0, 7.0, 8.0]);
    }
}
