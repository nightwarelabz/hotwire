//! `hotwire.toml` — the bridge's configuration.
//!
//! Two things live here: the material rename table (Blender
//! material name → house material name) and the per-class era
//! budgets. Both have defaults so the tool runs with no config
//! at all; the file only overrides what it names.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Triangle budget for one asset class: warn past `warn`,
/// refuse past `refuse` (SPEC: era enforcement).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub warn: usize,
    pub refuse: usize,
}

/// The asset classes the era law knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetClass {
    Prop,
    Character,
}

impl AssetClass {
    pub fn parse(s: &str) -> Result<AssetClass, String> {
        match s {
            "prop" | "props" => Ok(AssetClass::Prop),
            "character" | "characters" => Ok(AssetClass::Character),
            other => Err(format!(
                "unknown asset class '{other}' (want 'prop' or 'character')"
            )),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            AssetClass::Prop => "prop",
            AssetClass::Character => "character",
        }
    }
}

/// SPEC defaults: props warn 2,000 / refuse 20,000;
/// characters warn 4,000 / refuse 40,000.
pub fn default_budget(class: AssetClass) -> Budget {
    match class {
        AssetClass::Prop => Budget {
            warn: 2_000,
            refuse: 20_000,
        },
        AssetClass::Character => Budget {
            warn: 4_000,
            refuse: 40_000,
        },
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Blender material name → house material name.
    pub material_renames: BTreeMap<String, String>,
    /// Per-class budget overrides, keyed by `AssetClass::key`.
    pub budget_overrides: BTreeMap<String, Budget>,
}

impl Config {
    pub fn budget(&self, class: AssetClass) -> Budget {
        self.budget_overrides
            .get(class.key())
            .copied()
            .unwrap_or_else(|| default_budget(class))
    }

    /// Resolve a Blender material name to its house name.
    /// Returns `(house_name, was_mapped)`.
    pub fn map_material(&self, blender_name: &str) -> (String, bool) {
        match self.material_renames.get(blender_name) {
            Some(house) => (house.clone(), true),
            None => (blender_name.to_string(), false),
        }
    }

    /// Load from a TOML file. A missing file is an error only
    /// when the path was named explicitly by the caller.
    pub fn load_from(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading config {}: {e}", path.display()))?;
        Config::parse(&text).map_err(|e| format!("parsing config {}: {e}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let file: FileConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut budget_overrides = BTreeMap::new();
        for (key, b) in file.budgets {
            let class = AssetClass::parse(&key)?;
            let dflt = default_budget(class);
            budget_overrides.insert(
                class.key().to_string(),
                Budget {
                    warn: b.warn.unwrap_or(dflt.warn),
                    refuse: b.refuse.unwrap_or(dflt.refuse),
                },
            );
        }
        Ok(Config {
            material_renames: file.materials,
            budget_overrides,
        })
    }

    /// The default lookup: an explicit `--config` path must
    /// exist; otherwise `./hotwire.toml` is used when present.
    pub fn discover(explicit: Option<&Path>) -> Result<Config, String> {
        match explicit {
            Some(p) => Config::load_from(p),
            None => {
                let p = Path::new("hotwire.toml");
                if p.exists() {
                    Config::load_from(p)
                } else {
                    Ok(Config::default())
                }
            }
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct FileConfig {
    materials: BTreeMap<String, String>,
    budgets: BTreeMap<String, FileBudget>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct FileBudget {
    warn: Option<usize>,
    refuse: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let c = Config::default();
        assert_eq!(
            c.budget(AssetClass::Prop),
            Budget {
                warn: 2_000,
                refuse: 20_000
            }
        );
        assert_eq!(
            c.budget(AssetClass::Character),
            Budget {
                warn: 4_000,
                refuse: 40_000
            }
        );
    }

    #[test]
    fn toml_overrides_and_renames() {
        let c = Config::parse(
            r#"
[materials]
"Metal Dark" = "painted-metal"

[budgets.prop]
warn = 500
"#,
        )
        .unwrap();
        assert_eq!(
            c.map_material("Metal Dark"),
            ("painted-metal".to_string(), true)
        );
        assert_eq!(c.map_material("Other"), ("Other".to_string(), false));
        assert_eq!(
            c.budget(AssetClass::Prop),
            Budget {
                warn: 500,
                refuse: 20_000
            }
        );
        assert_eq!(c.budget(AssetClass::Character).warn, 4_000);
    }

    #[test]
    fn bad_class_is_an_error() {
        assert!(AssetClass::parse("vehicle").is_err());
        assert!(Config::parse("[budgets.vehicle]\nwarn = 1\n").is_err());
    }
}
