//! The `.c35m` output model and its writer.
//!
//! The writer mirrors the engine's `modeler::format::save`
//! byte-for-byte in shape: same directives, same order, and the
//! same float spelling (Rust's shortest round-trip `Display`
//! for `f32`, which is what the engine's `fmt_f` uses). That is
//! what makes Hotwire's output deterministic and diff-able in
//! exactly the house style.

/// One output model: becomes one `.c35m` file.
#[derive(Debug, Default)]
pub struct OutModel {
    pub name: String,
    pub materials: Vec<String>,
    pub verts: Vec<[f32; 3]>,
    pub faces: Vec<OutFace>,
}

/// One triangle. The corner UVs ride with the vertex indices;
/// the normal is per-face and always written explicitly
/// (SPEC: flat per-face normals).
#[derive(Debug)]
pub struct OutFace {
    pub material: u16,
    pub corners: [(u32, [f32; 2]); 3],
    pub normal: [f32; 3],
}

impl OutModel {
    pub fn tris(&self) -> usize {
        self.faces.len()
    }

    /// (min, max) over all vertices, in metres.
    pub fn bbox(&self) -> ([f32; 3], [f32; 3]) {
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in &self.verts {
            for i in 0..3 {
                min[i] = min[i].min(v[i]);
                max[i] = max[i].max(v[i]);
            }
        }
        if self.verts.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (min, max)
    }
}

fn fmt_f(x: f32) -> String {
    // Identical spelling to the engine's format::save: Rust's
    // Display for f32 is the shortest string that parses back
    // to the identical value. One normalization on top: -0.0
    // prints as "0" — the axis rotation mints -0.0 from honest
    // zeros, and the house spelling of zero is "0" (the value
    // is unchanged: -0.0 == 0.0).
    let x = if x == 0.0 { 0.0 } else { x };
    format!("{x}")
}

/// Serialize in the engine's `.c35m` v1 grammar.
pub fn render_c35m(model: &OutModel) -> String {
    let mut out = String::new();
    out.push_str("c35-model 1\n");
    out.push_str(&format!("name {}\n", model.name));
    for (i, name) in model.materials.iter().enumerate() {
        out.push_str(&format!("material {i} {name}\n"));
    }
    for v in &model.verts {
        out.push_str(&format!(
            "v {} {} {}\n",
            fmt_f(v[0]),
            fmt_f(v[1]),
            fmt_f(v[2])
        ));
    }
    for f in &model.faces {
        let mut line = format!("f {} 3", f.material);
        for (v, uv) in &f.corners {
            line.push_str(&format!(" {}:{},{}", v, fmt_f(uv[0]), fmt_f(uv[1])));
        }
        line.push_str(&format!(
            " n {} {} {}",
            fmt_f(f.normal[0]),
            fmt_f(f.normal[1]),
            fmt_f(f.normal[2])
        ));
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Sanitize an arbitrary object/material name into the single
/// lowercase token the `.c35m` grammar (and the house naming
/// style: `greyport-lamp-standard`) expects: ASCII alphanumerics
/// kept, everything else collapses to single dashes, no leading
/// or trailing dash. May return an empty string; callers fall
/// back to a generated name.
pub fn sanitize(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_cases() {
        assert_eq!(sanitize("Dock Bollard"), "dock-bollard");
        assert_eq!(sanitize("USP_Pistol.001"), "usp-pistol-001");
        assert_eq!(sanitize("__weird--name__"), "weird-name");
        assert_eq!(sanitize(""), "");
        assert_eq!(sanitize("painted-metal"), "painted-metal");
    }

    #[test]
    fn render_matches_engine_grammar() {
        let mut m = OutModel {
            name: "tri".to_string(),
            materials: vec!["painted-metal".to_string()],
            ..Default::default()
        };
        m.verts = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]];
        m.faces.push(OutFace {
            material: 0,
            corners: [(0, [0.0, 0.0]), (1, [1.0, 0.0]), (2, [0.0, 1.0])],
            normal: [0.0, 1.0, 0.0],
        });
        let text = render_c35m(&m);
        assert_eq!(
            text,
            "c35-model 1\nname tri\nmaterial 0 painted-metal\n\
             v 0 0 0\nv 1 0 0\nv 0 0 -1\n\
             f 0 3 0:0,0 1:1,0 2:0,1 n 0 1 0\n"
        );
    }
}
