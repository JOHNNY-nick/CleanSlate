//! The engine: configuration plus injected file system and registry.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::fs::{FileSystem, RealFileSystem};
use crate::model::{InstalledApp, ScanOptions, ScanProgress, ScanReport};
use crate::registry::{Registry, WinRegistry};
use crate::safety::{KnownFolders, SafetyRules};
use crate::scan::{scan_orphans, ScanEnv};

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
    /// User exclusions: folders or keys never proposed (EF-20).
    #[serde(default)]
    pub exclusions: Vec<String>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            dry_run: true,
            data_dir: None,
            exclusions: Vec::new(),
        }
    }
}

/// Clears the "busy" flag when a long operation ends, even on panic.
struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The CleanSlate engine. One instance per UI (or CLI) session.
///
/// `Sync`: `cs_cancel` may be called from another thread while a scan runs.
pub struct Engine {
    config: EngineConfig,
    fs: Box<dyn FileSystem>,
    registry: Box<dyn Registry>,
    folders: KnownFolders,
    rules: SafetyRules,
    /// Query running processes and services (only on the real system).
    probe_system: bool,
    cancel: AtomicBool,
    busy: AtomicBool,
}

impl Engine {
    /// Creates an engine on the given file system and registry, for a
    /// fictitious machine whose system drive is `C:\` (tests inject fakes).
    pub fn new(config: EngineConfig, fs: Box<dyn FileSystem>, registry: Box<dyn Registry>) -> Self {
        Self::with_folders(config, fs, registry, KnownFolders::under(r"C:\"))
    }

    /// Same as [`Engine::new`] with explicit known folders.
    pub fn with_folders(
        config: EngineConfig,
        fs: Box<dyn FileSystem>,
        registry: Box<dyn Registry>,
        folders: KnownFolders,
    ) -> Self {
        tracing::debug!(dry_run = config.dry_run, "engine created");
        let rules = SafetyRules::new(&folders, &config.exclusions);
        Engine {
            config,
            fs,
            registry,
            folders,
            rules,
            probe_system: false,
            cancel: AtomicBool::new(false),
            busy: AtomicBool::new(false),
        }
    }

    /// Creates an engine on the real disk and registry. Nothing is read
    /// until an operation is called.
    pub fn with_system(config: EngineConfig) -> Self {
        let mut engine = Self::with_folders(
            config,
            Box::new(RealFileSystem),
            Box::new(WinRegistry::system()),
            KnownFolders::from_system(),
        );
        engine.probe_system = true;
        engine
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

    /// Safety rules of this engine.
    pub fn rules(&self) -> &SafetyRules {
        &self.rules
    }

    /// Known folders of this engine.
    pub fn folders(&self) -> &KnownFolders {
        &self.folders
    }

    /// Lists installed applications, read-only (EF-01, EF-02).
    pub fn list_apps(&self) -> CoreResult<Vec<InstalledApp>> {
        crate::inventory::list_apps(self.registry())
    }

    /// Asks the running long operation to stop (EF-11); it returns a
    /// partial result flagged `cancelled`.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Marks the engine busy for one long operation; `BUSY` if another runs.
    fn begin(&self) -> CoreResult<BusyGuard<'_>> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(CoreError::Busy);
        }
        self.cancel.store(false, Ordering::Release);
        Ok(BusyGuard(&self.busy))
    }

    /// Scans for orphan leftovers, read-only (EF-05 to EF-11).
    pub fn scan_orphans(
        &self,
        options: &ScanOptions,
        progress: &(dyn Fn(&ScanProgress) + Sync),
    ) -> CoreResult<ScanReport> {
        let _guard = self.begin()?;
        let apps = self.list_apps()?;
        let extra = if self.probe_system {
            let mut exes = crate::system::service_executables(self.registry());
            exes.extend(crate::system::process_executables());
            exes
        } else {
            Vec::new()
        };
        let env = ScanEnv {
            fs: self.fs(),
            registry: self.registry(),
            folders: &self.folders,
            rules: &self.rules,
            apps: &apps,
            extra_executables: &extra,
            cancel: &self.cancel,
            now: Utc::now(),
        };
        Ok(scan_orphans(&env, options, progress))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use crate::registry::TestRegistryKey;
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
        assert!(engine.folders().windir.ends_with("Windows"));
    }

    #[test]
    fn scan_through_the_engine_and_busy_guard() {
        let (fs, _) = crate::scan::tests::fixture();
        let area = TestRegistryKey::new();
        let config = EngineConfig {
            exclusions: vec![r"C:\Users\Ana\AppData\Roaming\obs-studio".into()],
            ..EngineConfig::default()
        };
        let engine = Engine::new(config, Box::new(fs), Box::new(area.registry()));

        let report = engine
            .scan_orphans(&ScanOptions::default(), &|_| {})
            .unwrap();
        let all: Vec<&str> = report
            .groups
            .iter()
            .flat_map(|g| g.items.iter().map(|i| i.location.as_str()))
            .collect();
        assert!(all.iter().any(|l| l.ends_with("audacity")));
        assert!(!all.iter().any(|l| l.contains("obs-studio")), "excluded");

        let _held = engine.begin().unwrap();
        assert_eq!(
            engine
                .scan_orphans(&ScanOptions::default(), &|_| {})
                .unwrap_err()
                .code(),
            "BUSY"
        );
    }

    #[test]
    fn cancel_flag_is_reset_by_the_next_operation() {
        let engine = Engine::new(
            EngineConfig::default(),
            Box::new(FakeFileSystem::new()),
            Box::new(TestRegistryKey::new().registry()),
        );
        engine.cancel();
        let guard = engine.begin().unwrap();
        assert!(!engine.cancel.load(Ordering::Acquire));
        drop(guard);
        assert!(!engine.busy.load(Ordering::Acquire));
    }
}
