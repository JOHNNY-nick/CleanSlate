//! CleanSlate engine.
//!
//! This crate holds all the business logic of CleanSlate. It contains no FFI
//! code: the `cleanslate-ffi` crate exposes it to C#, and `cleanslate-cli`
//! drives it from the command line.

pub mod disk_map;
pub mod engine;
pub mod error;
pub mod evidence;
pub mod fs;
pub mod history;
pub mod inventory;
pub mod logging;
pub mod manager;
pub mod matcher;
pub mod model;
pub mod msi;
pub mod native;
pub mod process;
pub mod quarantine;
pub mod registry;
pub mod safety;
pub mod scan;
pub mod services;
pub mod shortcut;
pub mod store;
pub mod sysclean;
pub mod system;
pub mod tasks;
pub mod uninstall;
pub mod zones;

pub use engine::{Engine, EngineConfig};
pub use error::{CoreError, CoreResult};

/// Engine version, taken from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_package() {
        assert_eq!(VERSION, "0.1.0");
    }

    /// D96: every GitHub action of the CI and of the release is pinned to a full commit, and
    /// Inno Setup to one version; the checks of cargo-deny run in the CI.
    #[test]
    fn the_build_chain_is_pinned() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\..");
        let workflows = root.join(r".github\workflows");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&workflows).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for line in text.lines() {
                let Some((_, reference)) = line.trim().split_once("uses: ") else {
                    continue;
                };
                let pinned = reference.split_once('@').is_some_and(|(_, at)| {
                    let sha = at.split_whitespace().next().unwrap_or_default();
                    sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())
                });
                if !pinned {
                    offenders.push(format!("{}: {}", path.display(), line.trim()));
                }
            }
            if text.contains("choco install") && !text.contains("--version ") {
                offenders.push(format!("{}: choco without --version", path.display()));
            }
        }
        assert!(offenders.is_empty(), "unpinned: {offenders:?}");
        let ci = std::fs::read_to_string(workflows.join("ci.yml")).unwrap();
        assert!(ci.contains("cargo deny check"));
        assert!(root.join(r"core\deny.toml").is_file());
    }
}
