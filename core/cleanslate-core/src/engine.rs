//! The engine: configuration plus injected file system and registry.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::fs::{FileSystem, RealFileSystem};
use crate::history::History;
use crate::model::{
    CleanPlan, InstalledApp, Leftover, PurgeRequest, PurgeResult, RestoreRequest, ScanOptions,
    ScanProgress, ScanReport, Session,
};
use crate::quarantine::{simulate, Quarantine};
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
    /// Items of the last scan, by id: `cs_clean` only accepts these.
    last_report: Mutex<HashMap<String, Leftover>>,
    /// History database, opened on first use.
    history: Mutex<Option<History>>,
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
            last_report: Mutex::new(HashMap::new()),
            history: Mutex::new(None),
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
        let report = scan_orphans(&env, options, progress);
        self.remember(&report);
        Ok(report)
    }

    /// Keeps the items of `report` so that `clean` can find them by id.
    fn remember(&self, report: &ScanReport) {
        if let Ok(mut last) = self.last_report.lock() {
            last.clear();
            for item in report.groups.iter().flat_map(|g| &g.items) {
                last.insert(item.id.clone(), item.clone());
            }
        }
    }

    /// Data folder: configured one, `%LOCALAPPDATA%\CleanSlate` on the real
    /// system, none for a test engine (so tests never write outside their
    /// temporary folders).
    pub fn data_dir(&self) -> Option<PathBuf> {
        match &self.config.data_dir {
            Some(dir) => Some(dir.clone()),
            None if self.probe_system => Some(self.folders.local.join("CleanSlate")),
            None => None,
        }
    }

    /// `true` when nothing may be modified: engine or request in simulation.
    fn simulating(&self, request_dry_run: bool) -> bool {
        self.config.dry_run || request_dry_run
    }

    fn history(&self) -> CoreResult<MutexGuard<'_, Option<History>>> {
        let mut guard = self
            .history
            .lock()
            .map_err(|_| CoreError::InvalidInput("history lock poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(match self.data_dir() {
                Some(dir) => History::open(&dir.join("cleanslate.db"))?,
                None => History::in_memory()?,
            });
        }
        Ok(guard)
    }

    fn with_quarantine<T>(&self, f: impl FnOnce(&Quarantine) -> CoreResult<T>) -> CoreResult<T> {
        let dir = self
            .data_dir()
            .ok_or_else(|| CoreError::InvalidInput("no data folder configured".into()))?;
        let root = dir.join("quarantine");
        f(&Quarantine {
            root: &root,
            fs: self.fs(),
            registry: self.registry(),
            rules: &self.rules,
        })
    }

    /// Moves the selected items of the last scan to quarantine (EF-17), or
    /// only lists them in simulation mode (ENF-04).
    pub fn clean(&self, plan: &CleanPlan) -> CoreResult<Session> {
        let _guard = self.begin()?;
        let items: Vec<Leftover> = {
            let last = self
                .last_report
                .lock()
                .map_err(|_| CoreError::InvalidInput("report lock poisoned".into()))?;
            plan.item_ids
                .iter()
                .map(|id| {
                    last.get(id)
                        .cloned()
                        .ok_or_else(|| CoreError::InvalidInput(format!("unknown item: {id}")))
                })
                .collect::<CoreResult<_>>()?
        };
        if self.simulating(plan.dry_run) {
            return Ok(simulate(plan.origin.clone(), &items, Utc::now()));
        }
        let session = self.with_quarantine(|q| {
            q.clean(
                plan.origin.clone(),
                &items,
                plan.confirmation.as_deref(),
                Utc::now(),
            )
        })?;
        if let Some(history) = self.history()?.as_mut() {
            history.save_session(&session)?;
        }
        if let Ok(mut last) = self.last_report.lock() {
            for id in &plan.item_ids {
                last.remove(id);
            }
        }
        Ok(session)
    }

    /// Restores a session, entirely or partly (EF-18).
    pub fn restore(&self, request: &RestoreRequest) -> CoreResult<Session> {
        let _guard = self.begin()?;
        if self.simulating(request.dry_run) {
            let manifest = self.with_quarantine(|q| q.load(request.session_id))?;
            let mut session = manifest.session;
            session.simulated = true;
            return Ok(session);
        }
        let session = self.with_quarantine(|q| {
            q.restore(request.session_id, request.item_ids.as_deref(), Utc::now())
        })?;
        if let Some(history) = self.history()?.as_mut() {
            history.save_session(&session)?;
        }
        Ok(session)
    }

    /// Sessions of the history, most recent first (EF-20). Interrupted
    /// sessions are recovered on the way.
    pub fn list_sessions(&self) -> CoreResult<Vec<Session>> {
        if self.data_dir().is_none() {
            return Ok(Vec::new());
        }
        let sessions = self.with_quarantine(|q| Ok(q.sessions()))?;
        if let Some(history) = self.history()?.as_mut() {
            for session in &sessions {
                history.save_session(session)?;
            }
        }
        Ok(sessions)
    }

    /// Deletes for good the sessions older than the given age (EF-19).
    pub fn purge(&self, request: &PurgeRequest) -> CoreResult<PurgeResult> {
        let _guard = self.begin()?;
        if self.data_dir().is_none() {
            return Ok(PurgeResult {
                freed_bytes: 0,
                simulated: true,
            });
        }
        if self.simulating(request.dry_run) {
            let limit = Utc::now() - chrono::Duration::days(i64::from(request.older_than_days));
            let freed = self.with_quarantine(|q| {
                Ok(q.sessions()
                    .iter()
                    .filter(|s| {
                        s.created_at <= limit && s.status != crate::model::SessionStatus::Purged
                    })
                    .map(|s| s.freed_bytes)
                    .sum())
            })?;
            return Ok(PurgeResult {
                freed_bytes: freed,
                simulated: true,
            });
        }
        let freed = self.with_quarantine(|q| q.purge(request.older_than_days, Utc::now()))?;
        Ok(PurgeResult {
            freed_bytes: freed,
            simulated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use crate::model::{CleanPlan, PurgeRequest, RestoreRequest};
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

    /// Full cycle in a temporary machine under core/target (never %TEMP%,
    /// whose « Temp » component is protected) and the registry test area.
    #[test]
    fn scan_clean_list_restore_cycle_respects_dry_run() {
        use crate::fs::strip_verbatim;
        use crate::model::{Origin, SessionStatus};

        let parent =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\engine-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        let tool = base.join(r"Users\Ana\AppData\Roaming\OldTool");
        std::fs::create_dir_all(&tool).unwrap();
        std::fs::write(tool.join("settings.ini"), b"[x]").unwrap();
        let area = TestRegistryKey::new();
        let make = |dry_run: bool| {
            let config = EngineConfig {
                dry_run,
                data_dir: Some(base.join("data")),
                exclusions: vec![],
            };
            Engine::with_folders(
                config,
                Box::new(RealFileSystem),
                Box::new(area.registry()),
                KnownFolders::under(&base),
            )
        };
        let id_of = |report: &ScanReport| report.groups[0].items[0].id.clone();

        // Engine in simulation (default): even a plan with dry_run false only simulates.
        let safe = make(true);
        let report = safe.scan_orphans(&ScanOptions::default(), &|_| {}).unwrap();
        let mut plan = CleanPlan::new(vec![id_of(&report)], Origin::Orphans);
        plan.dry_run = false;
        assert!(safe.clean(&plan).unwrap().simulated);
        assert!(tool.exists());
        assert_eq!(
            safe.clean(&CleanPlan::new(vec!["nope".into()], Origin::Orphans))
                .unwrap_err()
                .code(),
            "INVALID_INPUT"
        );

        // Real engine: the plan must also say dry_run = false.
        let real = make(false);
        let report = real.scan_orphans(&ScanOptions::default(), &|_| {}).unwrap();
        let id = id_of(&report);
        assert!(
            real.clean(&CleanPlan::new(vec![id.clone()], Origin::Orphans))
                .unwrap()
                .simulated
        );
        let mut plan = CleanPlan::new(vec![id], Origin::Orphans);
        plan.dry_run = false;
        let session = real.clean(&plan).unwrap();
        assert!(!session.simulated);
        assert_eq!(session.status, SessionStatus::Completed);
        assert!(!tool.exists());

        let sessions = real.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        let simulated = real
            .purge(&PurgeRequest {
                older_than_days: 0,
                dry_run: true,
            })
            .unwrap();
        assert!(simulated.simulated);
        assert!(simulated.freed_bytes > 0);

        let mut request = RestoreRequest {
            session_id: session.id,
            item_ids: None,
            dry_run: true,
        };
        assert!(real.restore(&request).unwrap().simulated);
        assert!(!tool.exists());
        request.dry_run = false;
        assert_eq!(
            real.restore(&request).unwrap().status,
            SessionStatus::Restored
        );
        assert_eq!(std::fs::read(tool.join("settings.ini")).unwrap(), b"[x]");
    }

    #[test]
    fn test_engine_has_no_data_folder() {
        let engine = Engine::new(
            EngineConfig::default(),
            Box::new(FakeFileSystem::new()),
            Box::new(TestRegistryKey::new().registry()),
        );
        assert!(engine.data_dir().is_none());
        assert!(engine.list_sessions().unwrap().is_empty());
        assert!(
            engine
                .purge(&PurgeRequest {
                    older_than_days: 30,
                    dry_run: false
                })
                .unwrap()
                .simulated
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
