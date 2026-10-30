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
    AppKind, AppLeftoversRequest, CleanPlan, Confidence, InstalledApp, Leftover, PurgeRequest,
    PurgeResult, RestoreRequest, ScanOptions, ScanProgress, ScanReport, Session, UninstallRequest,
    UninstallResult,
};
use crate::quarantine::{simulate, Quarantine};
use crate::registry::{Registry, WinRegistry};
use crate::safety::{KnownFolders, SafetyRules};
use crate::scan::{scan_app_leftovers, scan_orphans, ScanEnv};
use crate::services::{NoServiceControl, ScmServiceControl, ServiceControl};
use crate::store::{NoPackageStore, PackageStore, StorePackage, WinRtPackageStore};
use crate::tasks::{ComTaskStore, NoTaskStore, TaskStore};
use crate::uninstall::{
    build_command, program_exists, JobProcessRunner, NoProcessRunner, NoRestorePoints,
    ProcessRunner, RestorePoints, SystemRestorePoints, UNINSTALL_TIMEOUT,
};

fn default_true() -> bool {
    true
}

/// The application is still listed (uninstaller missing, D45): no item is
/// High, so none is prechecked (EF-10), and each says why.
fn cap_at_medium(report: &mut ScanReport) {
    for item in report.groups.iter_mut().flat_map(|g| g.items.iter_mut()) {
        if item.confidence == Confidence::High {
            item.confidence = Confidence::Medium;
        }
        item.reasons
            .push("Programme de désinstallation introuvable : vérifiez avant de cocher".to_owned());
    }
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
    /// Applications captured before their uninstall (fingerprints, § 6.4).
    removed_apps: Mutex<HashMap<String, InstalledApp>>,
    /// Starts uninstallers (refuses on a test engine).
    runner: Box<dyn ProcessRunner>,
    /// Creates restore points (unavailable on a test engine).
    restore_points: Box<dyn RestorePoints>,
    /// Scheduled tasks (refuses everything on a test engine).
    tasks: Box<dyn TaskStore>,
    /// Services (refuses everything on a test engine).
    services: Box<dyn ServiceControl>,
    /// Store packages of the current user (none on a test engine).
    packages: Box<dyn PackageStore>,
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
            removed_apps: Mutex::new(HashMap::new()),
            runner: Box::new(NoProcessRunner),
            restore_points: Box::new(NoRestorePoints),
            tasks: Box::new(NoTaskStore),
            services: Box::new(NoServiceControl),
            packages: Box::new(NoPackageStore),
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
        if let Some(dir) = engine.data_dir() {
            crate::logging::init(&dir.join("logs"));
        }
        engine.set_uninstall_backends(Box::new(JobProcessRunner), Box::new(SystemRestorePoints));
        engine.set_system_backends(Box::new(ComTaskStore), Box::new(ScmServiceControl));
        engine.set_package_store(Box::new(WinRtPackageStore));
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

    /// Lists installed applications, read-only (EF-01, EF-02): the
    /// Uninstall keys, then the Store packages of the current user (EF-04).
    /// A failure of the package listing leaves the Win32 list intact.
    pub fn list_apps(&self) -> CoreResult<Vec<InstalledApp>> {
        let mut apps = crate::inventory::list_apps(self.registry())?;
        match self.packages.list() {
            Ok(packages) => apps.extend(
                packages
                    .iter()
                    .filter(|p| p.is_listed())
                    .map(StorePackage::to_app),
            ),
            Err(err) => tracing::warn!(%err, "Store packages not listed"),
        }
        Ok(apps)
    }

    /// Family names of every package of the current user, listed now;
    /// `None` when the listing fails or is empty (Store leftovers disabled).
    fn installed_families(&self) -> Option<Vec<String>> {
        let families: Vec<String> = self
            .packages
            .list()
            .ok()?
            .into_iter()
            .map(|p| p.family_name)
            .collect();
        (!families.is_empty()).then_some(families)
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
        let families = self.installed_families();
        let env = ScanEnv {
            fs: self.fs(),
            registry: self.registry(),
            folders: &self.folders,
            rules: &self.rules,
            apps: &apps,
            extra_executables: &extra,
            cancel: &self.cancel,
            now: Utc::now(),
            installed_packages: families.as_deref(),
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
            tasks: self.tasks.as_ref(),
            services: self.services.as_ref(),
            packages: self.packages.as_ref(),
        })
    }

    /// Replaces the Store package store (tests inject a fake).
    pub fn set_package_store(&mut self, packages: Box<dyn PackageStore>) {
        self.packages = packages;
    }

    /// Replaces the task store and the service control (tests inject fakes).
    pub fn set_system_backends(
        &mut self,
        tasks: Box<dyn TaskStore>,
        services: Box<dyn ServiceControl>,
    ) {
        self.tasks = tasks;
        self.services = services;
    }

    /// Replaces the process runner and restore points (tests inject fakes).
    pub fn set_uninstall_backends(
        &mut self,
        runner: Box<dyn ProcessRunner>,
        restore_points: Box<dyn RestorePoints>,
    ) {
        self.runner = runner;
        self.restore_points = restore_points;
    }

    /// Runs the official uninstaller of an application (EF-12, EF-14,
    /// EF-15) and waits for its whole process tree. In simulation (the
    /// default) only reports the command that would run.
    pub fn uninstall(&self, request: &UninstallRequest) -> CoreResult<UninstallResult> {
        let _guard = self.begin()?;
        let app = self
            .list_apps()?
            .into_iter()
            .find(|a| a.id == request.app_id)
            .ok_or_else(|| CoreError::NotFound {
                location: request.app_id.clone(),
            })?;
        if app.kind == AppKind::Store {
            return self.uninstall_package(&app, request);
        }
        let command = build_command(&app, request.quiet)?;
        if !program_exists(&command.program) {
            return Err(CoreError::NoUninstaller {
                location: command.program.display().to_string(),
            });
        }
        // Fingerprint captured before uninstalling (specs § 7.1, step 1).
        if let Ok(mut removed) = self.removed_apps.lock() {
            removed.insert(app.id.clone(), app.clone());
        }
        let mut result = UninstallResult {
            app_id: app.id.clone(),
            display_name: app.display_name.clone(),
            command: command.command_line(),
            simulated: true,
            restore_point_created: false,
            restore_point_error: None,
            exit_code: None,
            timed_out: false,
            cancelled: false,
            still_installed: true,
        };
        if self.simulating(request.dry_run) {
            return Ok(result);
        }
        result.simulated = false;
        let sequence = if request.create_restore_point {
            match self.restore_points.begin(&format!(
                "CleanSlate : désinstallation de {}",
                app.display_name
            )) {
                Ok(seq) => {
                    result.restore_point_created = true;
                    Some(seq)
                }
                Err(err) => {
                    result.restore_point_error = Some(err);
                    None
                }
            }
        } else {
            None
        };
        tracing::info!(app = %app.display_name, command = %result.command, "running uninstaller");
        let outcome = self.runner.run(&command, &self.cancel, UNINSTALL_TIMEOUT);
        if let Some(seq) = sequence {
            self.restore_points.end(seq, outcome.is_ok());
        }
        let outcome = outcome?;
        result.exit_code = outcome.exit_code;
        result.timed_out = outcome.timed_out;
        result.cancelled = outcome.cancelled;
        result.still_installed = self.list_apps()?.iter().any(|a| a.id == app.id);
        Ok(result)
    }

    /// Removes a Store application for the current user only (EF-04), with
    /// the progress reported by Windows. No restore point: it does not cover
    /// packages, and the removal is not run through an uninstaller.
    fn uninstall_package(
        &self,
        app: &InstalledApp,
        request: &UninstallRequest,
    ) -> CoreResult<UninstallResult> {
        let full_name = app
            .package_full_name
            .clone()
            .ok_or_else(|| CoreError::NoUninstaller {
                location: app.display_name.clone(),
            })?;
        if let Ok(mut removed) = self.removed_apps.lock() {
            removed.insert(app.id.clone(), app.clone());
        }
        let mut result = UninstallResult {
            app_id: app.id.clone(),
            display_name: app.display_name.clone(),
            command: format!("RemovePackageAsync {full_name}"),
            simulated: true,
            restore_point_created: false,
            restore_point_error: None,
            exit_code: None,
            timed_out: false,
            cancelled: false,
            still_installed: true,
        };
        if self.simulating(request.dry_run) {
            return Ok(result);
        }
        result.simulated = false;
        tracing::info!(package = %full_name, "removing Store package for the current user");
        self.packages.remove(&full_name, &|_| {})?;
        result.exit_code = Some(0);
        result.still_installed = self
            .packages
            .list()?
            .iter()
            .any(|p| p.full_name == full_name);
        Ok(result)
    }

    /// Leftovers of an application just uninstalled (EF-13): empty if it is
    /// still installed. Its items can then be passed to [`Engine::clean`].
    ///
    /// With `uninstaller_missing` (design § 5, D45), the application is
    /// still listed but cannot be uninstalled: its leftovers are searched
    /// anyway and capped at Medium, so none is prechecked. Refused when its
    /// uninstaller exists: the official uninstaller must run first.
    pub fn scan_app_leftovers(
        &self,
        request: &AppLeftoversRequest,
        progress: &(dyn Fn(&ScanProgress) + Sync),
    ) -> CoreResult<ScanReport> {
        let _guard = self.begin()?;
        let mut apps = self.list_apps()?;
        let captured = self
            .removed_apps
            .lock()
            .map_err(|_| CoreError::InvalidInput("lock poisoned".into()))?
            .get(&request.app_id)
            .cloned();
        let app = captured
            .or_else(|| apps.iter().find(|a| a.id == request.app_id).cloned())
            .ok_or_else(|| CoreError::NotFound {
                location: request.app_id.clone(),
            })?;
        if request.uninstaller_missing {
            let missing = match build_command(&app, false) {
                Ok(command) => !program_exists(&command.program),
                Err(err) => matches!(err, CoreError::NoUninstaller { .. }),
            };
            if !missing {
                return Err(CoreError::InvalidInput(format!(
                    "the uninstaller of {} exists: run it first",
                    app.display_name
                )));
            }
            // Its own entry must not make its files "installed".
            apps.retain(|a| a.id != app.id);
        }
        let families = self.installed_families();
        let env = ScanEnv {
            fs: self.fs(),
            registry: self.registry(),
            folders: &self.folders,
            rules: &self.rules,
            apps: &apps,
            extra_executables: &[],
            cancel: &self.cancel,
            now: Utc::now(),
            installed_packages: families.as_deref(),
        };
        let mut report = scan_app_leftovers(&env, &app, progress);
        if request.uninstaller_missing {
            cap_at_medium(&mut report);
        }
        self.remember(&report);
        Ok(report)
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
    use crate::model::{
        AppLeftoversRequest, CleanPlan, PurgeRequest, RestoreRequest, UninstallRequest,
    };
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

    /// Fake uninstaller: records the command and deletes the Uninstall entry
    /// from the test registry, like a real uninstaller would.
    struct DeletingRunner {
        registry: WinRegistry,
        entry: crate::registry::RegKeyPath,
        calls: std::sync::Arc<Mutex<Vec<String>>>,
    }

    impl crate::uninstall::ProcessRunner for DeletingRunner {
        fn run(
            &self,
            command: &crate::uninstall::Command,
            _: &AtomicBool,
            _: std::time::Duration,
        ) -> CoreResult<crate::uninstall::RunOutcome> {
            self.calls.lock().unwrap().push(command.command_line());
            self.registry.delete_tree(&self.entry)?;
            Ok(crate::uninstall::RunOutcome {
                exit_code: Some(0),
                ..Default::default()
            })
        }
    }

    struct FakeRestorePoints {
        fail: bool,
        log: std::sync::Arc<Mutex<Vec<String>>>,
    }

    impl crate::uninstall::RestorePoints for FakeRestorePoints {
        fn begin(&self, description: &str) -> Result<i64, String> {
            self.log
                .lock()
                .unwrap()
                .push(format!("begin {description}"));
            if self.fail {
                Err("protection système désactivée".into())
            } else {
                Ok(42)
            }
        }
        fn end(&self, sequence: i64, success: bool) {
            self.log
                .lock()
                .unwrap()
                .push(format!("end {sequence} {success}"));
        }
    }

    #[test]
    fn uninstall_simulated_then_real_then_leftovers() {
        use crate::fs::strip_verbatim;
        use crate::inventory::{stable_id, UNINSTALL_KEY};
        use crate::model::{Origin, RegistryView, SessionStatus};
        use crate::registry::{Hive, KeyView, RegKeyPath};
        use std::sync::Arc;

        let parent =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\uninstall-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        let install = base.join(r"Program Files\OldApp");
        std::fs::create_dir_all(install.join("data")).unwrap();
        std::fs::write(install.join("unins000.exe"), b"MZ fake").unwrap();
        std::fs::write(install.join(r"data\cache.bin"), [0u8; 100]).unwrap();

        let area = TestRegistryKey::new();
        let entry = RegKeyPath::new(Hive::LocalMachine, UNINSTALL_KEY)
            .with_view(KeyView::Force64)
            .join("OldApp_is1");
        let key = area.create(&entry);
        key.set_value("DisplayName", &"OldApp").unwrap();
        key.set_value("InstallLocation", &install.to_string_lossy().as_ref())
            .unwrap();
        let uninstaller = format!("\"{}\" /SILENT", install.join("unins000.exe").display());
        key.set_value("UninstallString", &uninstaller.as_str())
            .unwrap();
        let app_id = stable_id(RegistryView::Hklm64, "OldApp_is1");

        let calls = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::new(Mutex::new(Vec::new()));
        let make = |dry_run: bool, fail_restore: bool| {
            let config = EngineConfig {
                dry_run,
                data_dir: Some(base.join("data-dir")),
                exclusions: vec![],
            };
            let mut engine = Engine::with_folders(
                config,
                Box::new(RealFileSystem),
                Box::new(area.registry()),
                KnownFolders::under(&base),
            );
            engine.set_uninstall_backends(
                Box::new(DeletingRunner {
                    registry: area.registry(),
                    entry: entry.clone(),
                    calls: Arc::clone(&calls),
                }),
                Box::new(FakeRestorePoints {
                    fail: fail_restore,
                    log: Arc::clone(&log),
                }),
            );
            engine
        };
        let request = |dry_run: bool| UninstallRequest {
            app_id: app_id.clone(),
            quiet: false,
            create_restore_point: true,
            dry_run,
        };

        // Simulation: the command is reported, nothing runs.
        let simulated = make(true, false).uninstall(&request(false)).unwrap();
        assert!(simulated.simulated && simulated.still_installed);
        assert!(
            simulated.command.contains("unins000.exe") && simulated.command.ends_with("/SILENT")
        );
        assert!(calls.lock().unwrap().is_empty());

        assert_eq!(
            make(true, false)
                .uninstall(&UninstallRequest {
                    app_id: "nope".into(),
                    ..request(true)
                })
                .unwrap_err()
                .code(),
            "NOT_FOUND"
        );

        // Real (fake process): restore point failure is reported, not blocking.
        let engine = make(false, true);
        let result = engine.uninstall(&request(false)).unwrap();
        assert!(!result.simulated);
        assert!(!result.still_installed);
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.restore_point_created);
        assert!(result.restore_point_error.is_some());
        assert_eq!(calls.lock().unwrap().len(), 1);
        assert_eq!(
            log.lock().unwrap()[0],
            "begin CleanSlate : désinstallation de OldApp"
        );

        // Leftovers: the old install folder, then quarantine with origin App.
        let report = engine
            .scan_app_leftovers(
                &AppLeftoversRequest {
                    app_id: app_id.clone(),
                    uninstaller_missing: false,
                },
                &|_| {},
            )
            .unwrap();
        let item = report.groups[0]
            .items
            .iter()
            .find(|i| i.location.ends_with("OldApp"))
            .expect("install folder");
        let mut plan = CleanPlan::new(
            vec![item.id.clone()],
            Origin::App {
                app_id: app_id.clone(),
            },
        );
        plan.dry_run = false;
        let session = engine.clean(&plan).unwrap();
        assert_eq!(
            session.status,
            SessionStatus::Completed,
            "{:?}",
            session.errors
        );
        assert!(!install.exists());
    }

    /// EF-04, D48: a Store application is listed, removed for the current
    /// user (fake store), and its data folder and keys go to quarantine and
    /// come back identically; another removed package is an orphan (Medium).
    #[test]
    fn store_application_full_cycle_with_fake_store() {
        use crate::fs::strip_verbatim;
        use crate::model::{Origin, SessionStatus};
        use crate::registry::{KeySnapshot, RegKeyPath};
        use crate::safety::PACKAGE_KEY_PARENTS;
        use crate::store::{tests::package, FakePackageStore, StorePackage};

        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\store-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        let folders = KnownFolders::under(&base);
        let packages = folders.local.join("Packages");
        let app_folder = packages.join("Vendor.App_abcdefghjk123");
        std::fs::create_dir_all(app_folder.join("LocalState")).unwrap();
        std::fs::write(app_folder.join(r"LocalState\settings.dat"), b"theme=dark").unwrap();
        std::fs::create_dir_all(packages.join("Vendor.Orphan_abcdefghjk123")).unwrap();
        std::fs::create_dir_all(packages.join("Vendor.Runtime_abcdefghjk123")).unwrap();

        let area = TestRegistryKey::new();
        let storage = RegKeyPath::parse(PACKAGE_KEY_PARENTS[0]).unwrap();
        let app_key = storage.join("vendor.app_abcdefghjk123");
        area.create(&app_key)
            .set_value("Moniker", &"vendor.app")
            .unwrap();

        let framework = StorePackage {
            is_framework: true,
            ..package("Vendor.Runtime_abcdefghjk123")
        };
        let config = EngineConfig {
            dry_run: false,
            data_dir: Some(base.join("data-dir")),
            exclusions: vec![],
        };
        let mut engine = Engine::with_folders(
            config,
            Box::new(RealFileSystem),
            Box::new(area.registry()),
            folders,
        );
        engine.set_package_store(Box::new(FakePackageStore::with(vec![
            package("Vendor.App_abcdefghjk123"),
            framework,
        ])));

        // Listed (frameworks are not), with its Store fields.
        let apps = engine.list_apps().unwrap();
        let app = apps
            .iter()
            .find(|a| a.kind == AppKind::Store)
            .unwrap()
            .clone();
        assert_eq!(apps.iter().filter(|a| a.kind == AppKind::Store).count(), 1);
        assert_eq!(
            app.package_family_name.as_deref(),
            Some("Vendor.App_abcdefghjk123")
        );

        // Orphan scan: the removed package's folder is Medium; installed ones never.
        let report = engine
            .scan_orphans(&ScanOptions::default(), &|_| {})
            .unwrap();
        let items: Vec<_> = report.groups.iter().flat_map(|g| &g.items).collect();
        let orphan = items
            .iter()
            .find(|i| i.location.ends_with("Vendor.Orphan_abcdefghjk123"))
            .expect("orphan package folder");
        assert_eq!(orphan.confidence, Confidence::Medium);
        assert!(!items
            .iter()
            .any(|i| i.location.contains("Vendor.App_") || i.location.contains("Vendor.Runtime_")));

        // Simulated, then real removal for the current user.
        let request = |dry_run| UninstallRequest {
            app_id: app.id.clone(),
            quiet: false,
            create_restore_point: true,
            dry_run,
        };
        let simulated = engine.uninstall(&request(true)).unwrap();
        assert!(simulated.simulated && simulated.still_installed);
        let removed = engine.uninstall(&request(false)).unwrap();
        assert!(!removed.simulated && !removed.still_installed);
        assert!(!removed.restore_point_created);

        // Its leftovers: the data folder and its key, High.
        let report = engine
            .scan_app_leftovers(
                &AppLeftoversRequest {
                    app_id: app.id.clone(),
                    uninstaller_missing: false,
                },
                &|_| {},
            )
            .unwrap();
        let leftovers: Vec<_> = report.groups.iter().flat_map(|g| &g.items).collect();
        assert_eq!(leftovers.len(), 2, "{leftovers:?}");
        assert!(leftovers.iter().all(|i| i.confidence == Confidence::High));
        let before = KeySnapshot::read(&area.registry(), &app_key).unwrap();
        let mut plan = CleanPlan::new(
            leftovers.iter().map(|i| i.id.clone()).collect(),
            Origin::App {
                app_id: app.id.clone(),
            },
        );
        plan.dry_run = false;
        let session = engine.clean(&plan).unwrap();
        assert_eq!(
            session.status,
            SessionStatus::Completed,
            "{:?}",
            session.errors
        );
        assert!(!app_folder.exists());
        assert!(!area.registry().key_exists(&app_key));

        let restored = engine
            .restore(&crate::model::RestoreRequest {
                session_id: session.id,
                item_ids: None,
                dry_run: false,
            })
            .unwrap();
        assert!(restored.errors.is_empty(), "{:?}", restored.errors);
        assert_eq!(
            std::fs::read(app_folder.join(r"LocalState\settings.dat")).unwrap(),
            b"theme=dark"
        );
        assert_eq!(
            KeySnapshot::read(&area.registry(), &app_key).unwrap(),
            before
        );
    }

    #[test]
    fn missing_uninstaller_program_is_reported() {
        use crate::inventory::UNINSTALL_KEY;
        use crate::registry::{Hive, KeyView, RegKeyPath};
        let area = TestRegistryKey::new();
        let entry = RegKeyPath::new(Hive::CurrentUser, UNINSTALL_KEY)
            .with_view(KeyView::Default)
            .join("Ghost");
        let key = area.create(&entry);
        key.set_value("DisplayName", &"Ghost").unwrap();
        key.set_value(
            "UninstallString",
            &r#""C:\definitely\missing\unins000.exe""#,
        )
        .unwrap();
        let engine = Engine::new(
            EngineConfig::default(),
            Box::new(FakeFileSystem::new()),
            Box::new(area.registry()),
        );
        let id = engine.list_apps().unwrap()[0].id.clone();
        let err = engine
            .uninstall(&UninstallRequest {
                app_id: id,
                quiet: false,
                create_restore_point: false,
                dry_run: true,
            })
            .unwrap_err();
        assert_eq!(err.code(), "NO_UNINSTALLER");
    }

    #[test]
    fn missing_uninstaller_leftovers_are_searched_but_never_high() {
        use crate::fs::strip_verbatim;
        use crate::inventory::{stable_id, UNINSTALL_KEY};
        use crate::model::RegistryView;
        use crate::registry::{Hive, KeyView, RegKeyPath};

        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\uninstall-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        let install = base.join(r"Program Files\BrokenApp");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("data.bin"), [0u8; 64]).unwrap();

        let area = TestRegistryKey::new();
        let entry = RegKeyPath::new(Hive::LocalMachine, UNINSTALL_KEY)
            .with_view(KeyView::Force64)
            .join("BrokenApp_is1");
        let key = area.create(&entry);
        key.set_value("DisplayName", &"BrokenApp").unwrap();
        key.set_value("InstallLocation", &install.to_string_lossy().as_ref())
            .unwrap();
        let uninstaller = format!("\"{}\"", install.join("unins000.exe").display());
        key.set_value("UninstallString", &uninstaller.as_str())
            .unwrap();
        let app_id = stable_id(RegistryView::Hklm64, "BrokenApp_is1");
        let engine = Engine::with_folders(
            EngineConfig::default(),
            Box::new(RealFileSystem),
            Box::new(area.registry()),
            KnownFolders::under(&base),
        );
        let request = |uninstaller_missing| AppLeftoversRequest {
            app_id: app_id.clone(),
            uninstaller_missing,
        };

        // Without the flag the application is still installed: nothing.
        assert!(engine
            .scan_app_leftovers(&request(false), &|_| {})
            .unwrap()
            .groups
            .is_empty());

        // With it: the install folder is proposed, at most Medium.
        let report = engine.scan_app_leftovers(&request(true), &|_| {}).unwrap();
        let items: Vec<&Leftover> = report.groups.iter().flat_map(|g| &g.items).collect();
        let folder = items
            .iter()
            .find(|i| i.location.ends_with("BrokenApp"))
            .expect("install folder");
        assert_eq!(folder.confidence, Confidence::Medium);
        assert!(folder.reasons.iter().any(|r| r.contains("introuvable")));
        assert!(items.iter().all(|i| i.confidence != Confidence::High));

        // The uninstaller is back: refused, it must run first.
        std::fs::write(install.join("unins000.exe"), b"MZ fake").unwrap();
        let err = engine
            .scan_app_leftovers(&request(true), &|_| {})
            .unwrap_err();
        assert_eq!(err.code(), "INVALID_INPUT");
        assert!(install.exists());
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
