//! The engine: configuration plus injected file system and registry.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::fs::{FileSystem, RealFileSystem};
use crate::model::InstalledApp;
use crate::registry::{Registry, WinRegistry};

fn default_true() -> bool {
    true
}

/// Engine settings, sent as JSON by the UI to `cs_engine_new`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    /// Simulation mode: no destructive operation. `true` when absent
    /// from the JSON (safety rule 5).
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// Data folder (database, quarantine, logs). `None` means
    /// `%LOCALAPPDATA%\CleanSlate`.
    #[serde(default)]
    pub data_dir: Option<PathBuf>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            dry_run: true,
            data_dir: None,
        }
    }
}

/// The CleanSlate engine. One instance per UI (or CLI) session.
pub struct Engine {
    config: EngineConfig,
    fs: Box<dyn FileSystem>,
    registry: Box<dyn Registry>,
}

impl Engine {
    /// Creates an engine on the given file system and registry (tests
    /// inject fakes here).
    pub fn new(config: EngineConfig, fs: Box<dyn FileSystem>, registry: Box<dyn Registry>) -> Self {
        tracing::debug!(dry_run = config.dry_run, "engine created");
        Engine {
            config,
            fs,
            registry,
        }
    }

    /// Creates an engine on the real disk and registry. Nothing is read
    /// until an operation is called.
    pub fn with_system(config: EngineConfig) -> Self {
        Self::new(
            config,
            Box::new(RealFileSystem),
            Box::new(WinRegistry::system()),
        )
    }

    /// Engine version.
    pub fn version(&self) -> &'static str {
        crate::VERSION
    }

    /// Current configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Injected file system.
    pub fn fs(&self) -> &dyn FileSystem {
        self.fs.as_ref()
    }

    /// Injected registry.
    pub fn registry(&self) -> &dyn Registry {
        self.registry.as_ref()
    }

    /// Lists installed applications, read-only (EF-01, EF-02).
    pub fn list_apps(&self) -> CoreResult<Vec<InstalledApp>> {
        crate::inventory::list_apps(self.registry())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use std::path::Path;

    #[test]
    fn config_defaults_to_dry_run() {
        assert!(EngineConfig::default().dry_run);
        let parsed: EngineConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, EngineConfig::default());
        let explicit: EngineConfig = serde_json::from_str(r#"{"dry_run":false}"#).unwrap();
        assert!(!explicit.dry_run);
    }

    #[test]
    fn engine_uses_injected_file_system() {
        let mut fake = FakeFileSystem::new();
        fake.add_dir(r"C:\Fake");
        let engine = Engine::new(
            EngineConfig::default(),
            Box::new(fake),
            Box::new(WinRegistry::system()),
        );
        assert!(engine.fs().exists(Path::new(r"C:\Fake")));
        assert!(engine.config().dry_run);
        assert_eq!(engine.version(), "0.1.0");
    }
}
