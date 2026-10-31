//! Watch mode: poll a drop folder, convert what lands.
//!
//! A 1-second mtime poll (SPEC host contract: mtime poll is
//! fine at v1). A failed conversion is logged and the loop
//! keeps watching — the watcher never crashes on a bad export
//! and the converter never writes partial output on a refusal.

use crate::config::{AssetClass, Config};
use crate::convert::convert_file;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Clone, Copy, PartialEq)]
struct FileSig {
    len: u64,
    modified: Option<SystemTime>,
}

fn sig_of(path: &Path) -> Option<FileSig> {
    let md = std::fs::metadata(path).ok()?;
    Some(FileSig {
        len: md.len(),
        modified: md.modified().ok(),
    })
}

fn is_glb(path: &Path) -> bool {
    path.extension()
        .map(|e| e.to_string_lossy().eq_ignore_ascii_case("glb"))
        .unwrap_or(false)
}

/// Run the watch loop forever. Returns a process exit code
/// only for setup failures (bad folders).
pub fn run(drop_dir: &Path, models_dir: &Path, cfg: &Config, class: AssetClass) -> i32 {
    if !drop_dir.is_dir() {
        eprintln!(
            "hotwire watch: drop folder {} is not a directory",
            drop_dir.display()
        );
        return 1;
    }
    if let Err(e) = std::fs::create_dir_all(models_dir) {
        eprintln!(
            "hotwire watch: cannot create models folder {}: {e}",
            models_dir.display()
        );
        return 1;
    }
    println!(
        "hotwire watch: {} -> {} (class: {}, poll 1s; Ctrl-C to stop)",
        drop_dir.display(),
        models_dir.display(),
        class.key()
    );
    let mut seen: HashMap<PathBuf, FileSig> = HashMap::new();
    loop {
        let mut entries: Vec<PathBuf> = match std::fs::read_dir(drop_dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| is_glb(p))
                .collect(),
            Err(e) => {
                eprintln!("hotwire watch: cannot read drop folder: {e}");
                Vec::new()
            }
        };
        entries.sort();
        for path in entries {
            let Some(sig) = sig_of(&path) else {
                continue;
            };
            if seen.get(&path) == Some(&sig) {
                continue;
            }
            // Mark seen first: a refusal is logged once per
            // distinct file version, not once per second.
            seen.insert(path.clone(), sig);
            println!("hotwire watch: converting {}", path.display());
            match convert_file(&path, models_dir, cfg, class) {
                Ok(report) => report.print(),
                Err(e) => eprintln!("hotwire watch: {}", e.describe()),
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
