//! The engine: configuration plus injected file system and registry.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// How long the inventory of the applications is reused (D84): the map, the scan and the
/// pages ask for it often, and reading it (registry, then Store packages) takes 1.5 to 2.5 s.
const APPS_FRESH_FOR: Duration = Duration::from_secs(60);

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::disk_map::{Categorizer, DiskMap, DiskMapEnv, DiskMapProgress, DiskMapRequest};
use crate::error::{CoreError, CoreResult};
use crate::fs::{FileSystem, RealFileSystem};
use crate::history::History;
use crate::inventory::is_guid;
use crate::manager::{
    self, ActionResult, ManagerProgress, ManagerRequest, NoSystemShell, SystemShell, WinShell,
};
use crate::model::{
    AppKind, AppLeftoversRequest, CleanPlan, CleanProgress, Confidence, InstalledApp, JobProcess,
    Leftover, PurgeRequest, PurgeResult, QuarantineStatus, RestoreRequest, ScanOptions,
    ScanProgress, ScanReport, Session, UninstallProgress, UninstallRequest, UninstallResult,
    UninstallStep,
};
use crate::msi::{ApiMsiUninstaller, MsiUninstaller, NoMsiUninstaller};
use crate::quarantine::{simulate, Quarantine};
use crate::registry::{Registry, WinRegistry};
use crate::safety::{KnownFolders, SafetyRules};
use crate::scan::{scan_app_leftovers, scan_orphans, ScanEnv};
use crate::services::{NoServiceControl, ScmServiceControl, ServiceControl};
use crate::store::{
    one_per_family, provisioned_families, NoPackageStore, PackageStore, StorePackage,
    WinRtPackageStore,
};
use crate::sysclean::{NoSystemCleaner, SystemCleaner, SystemOutcome, WinSystemCleaner};
use crate::tasks::{ComTaskStore, NoTaskStore, TaskStore};
use crate::uninstall::{
    build_command, program_exists, JobProcessRunner, NoProcessRunner, NoRestorePoints,
    ProcessRunner, RestorePoints, RunOutcome, SystemRestorePoints, UNINSTALL_TIMEOUT,
};
use crate::zones::Zones;

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
    /// Key that signs the manifests of the quarantine (D88), read on the first operation
    /// on the quarantine: building an engine writes nothing outside its data folder (L-09).
    manifest_key: OnceLock<Option<Vec<u8>>>,
    /// Last inventory of the applications and when it was read (see [`APPS_FRESH_FOR`]).
    apps_cache: Mutex<Option<(Instant, Vec<InstalledApp>)>>,
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
    /// Windows Installer API (never used on a test engine: `msiexec` fallback).
    msi: Box<dyn MsiUninstaller>,
    /// History database, opened on first use.
    history: Mutex<Option<History>>,
    /// Recycle Bin, deletion at restart, volumes (refused on a test engine; D79).
    shell: Box<dyn SystemShell>,
    /// Clean-ups of Windows (refused on a test engine; D79).
    cleaner: Box<dyn SystemCleaner>,
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
            apps_cache: Mutex::new(None),
            // A key of its own for a test engine; the real one is read on first use.
            manifest_key: OnceLock::from(crate::native::random_bytes(32).ok()),
            history: Mutex::new(None),
            removed_apps: Mutex::new(HashMap::new()),
            runner: Box::new(NoProcessRunner),
            restore_points: Box::new(NoRestorePoints),
            tasks: Box::new(NoTaskStore),
            services: Box::new(NoServiceControl),
            packages: Box::new(NoPackageStore),
            msi: Box::new(NoMsiUninstaller),
            shell: Box::new(NoSystemShell),
            cleaner: Box::new(NoSystemCleaner),
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
            crate::logging::init(&crate::logging::log_dir(&dir.join("logs")));
        }
        engine.manifest_key = OnceLock::new();
        engine.set_uninstall_backends(Box::new(JobProcessRunner), Box::new(SystemRestorePoints));
        engine.set_system_backends(Box::new(ComTaskStore), Box::new(ScmServiceControl));
        engine.set_package_store(Box::new(WinRtPackageStore));
        engine.msi = Box::new(ApiMsiUninstaller);
        engine.shell = Box::new(WinShell);
        engine.cleaner = Box::new(WinSystemCleaner {
            folders: engine.folders.clone(),
        });
        engine
    }

    /// « Sûr » to the quarantine when it lies on another drive than every path and has room
    /// for them (EF-24); `None` when the Recycle Bin must be used instead.
    fn quarantine_elsewhere(
        &self,
        zones: &Zones,
        request: &manager::DeleteRequest,
    ) -> CoreResult<Option<ActionResult>> {
        let Some(data) = self.data_dir() else {
            return Ok(None);
        };
        if !manager::other_drive(&data.to_string_lossy(), &request.paths) {
            return Ok(None);
        }
        let fs = self.fs.as_ref();
        let mut items = Vec::new();
        for path in &request.paths {
            let p = std::path::Path::new(path);
            // Sensitive paths never go to the quarantine: the Recycle Bin handles them. A
            // refused or unreadable path too: the normal deletion reports it on its line.
            if !matches!(
                zones.authorize_path(fs, p, &request.manual),
                Ok(crate::zones::Zone::Normal)
            ) {
                return Ok(None);
            }
            let Ok(meta) = fs.metadata(p) else {
                return Ok(None);
            };
            items.push(Leftover {
                id: format!("manager-{}", items.len()),
                kind: if meta.is_dir {
                    crate::model::LeftoverKind::Folder
                } else {
                    crate::model::LeftoverKind::File
                },
                location: path.clone(),
                size_bytes: manager::size_of(fs, p),
                last_modified: meta.modified,
                confidence: Confidence::High,
                reasons: vec!["Choisi dans le Gestionnaire".into()],
                guessed_app: None,
            });
        }
        let needed: u64 = items.iter().map(|i| i.size_bytes).sum();
        if crate::native::volume_space(&data).is_none_or(|v| v.free <= needed) {
            return Ok(None);
        }
        let session = match self
            .with_quarantine(|q| q.clean(crate::model::Origin::Manager, &items, None, Utc::now()))
        {
            Ok(session) => session,
            Err(CoreError::ConfirmationRequired { .. }) => return Ok(None), // too big: the Recycle Bin
            Err(other) => return Err(other),
        };
        if let Some(history) = self.history()?.as_mut() {
            history.save_session(&session)?;
        }
        let mut result = ActionResult::default();
        for item in &session.items {
            let moved = matches!(
                item.status,
                QuarantineStatus::Quarantined | QuarantineStatus::PendingReboot
            );
            if moved {
                result.freed_now_bytes += item.size_bytes;
            }
            result.items.push(manager::ActionOutcome {
                path: item.original_location.clone(),
                size_bytes: item.size_bytes,
                zone: Some(crate::zones::Zone::Normal),
                done: if moved {
                    "quarantine".into()
                } else {
                    "failed".into()
                },
                error: item.error.clone(),
            });
        }
        Ok(Some(result))
    }

    /// One request of the Gestionnaire (D79): analyses are read-only; actions are
    /// simulated unless both the engine and the request say otherwise (rule 5), and
    /// every path is authorized by the sensitive zones right before acting (EF-22).
    pub fn manager(
        &self,
        request: &ManagerRequest,
        progress: &(dyn Fn(&ManagerProgress) + Sync),
    ) -> CoreResult<serde_json::Value> {
        let zones = Zones::new(&self.folders);
        let fs = self.fs.as_ref();
        let value = match request {
            ManagerRequest::ListFolder { path, known } => serde_json::to_value(
                manager::list_folder(fs, &zones, std::path::Path::new(path), known)?,
            )?,
            ManagerRequest::Zone { path } => {
                serde_json::to_value(zones.path_zone(std::path::Path::new(path))?)?
            }
            ManagerRequest::Caches => serde_json::to_value(manager::caches(fs, &self.folders))?,
            ManagerRequest::RecycleBins => serde_json::to_value(self.shell.recycle_bins()?)?,
            ManagerRequest::Volumes => serde_json::to_value(self.shell.volumes()?)?,
            ManagerRequest::Disks => serde_json::to_value(self.shell.disks()?)?,
            ManagerRequest::PendingAtRestart => {
                serde_json::to_value(self.shell.pending_at_restart()?)?
            }
            ManagerRequest::SystemItems => serde_json::to_value(self.cleaner.inspect()?)?,
            ManagerRequest::Analyze(analyze) => {
                let _guard = self.begin()?;
                serde_json::to_value(manager::analyze(
                    fs,
                    &self.folders,
                    &zones,
                    analyze,
                    Utc::now(),
                    &self.cancel,
                    progress,
                ))?
            }
            ManagerRequest::Duplicates { candidates } => {
                let _guard = self.begin()?;
                let hash = |p: &std::path::Path| crate::native::Sha256::of_file(p);
                serde_json::to_value(manager::duplicates(
                    fs,
                    &self.folders,
                    candidates,
                    &hash,
                    &self.cancel,
                ))?
            }
            ManagerRequest::Delete(delete) => {
                let _guard = self.begin()?;
                let simulate = self.simulating(delete.dry_run);
                // EF-24: « Sûr » with the quarantine on another drive frees the space now
                // and stays restorable from the History.
                if !simulate && delete.mode == manager::DeleteMode::Safe {
                    if let Some(result) = self.quarantine_elsewhere(&zones, delete)? {
                        return Ok(serde_json::to_value(result)?);
                    }
                }
                serde_json::to_value(manager::delete(
                    fs,
                    &zones,
                    self.shell.as_ref(),
                    delete,
                    simulate,
                ))?
            }
            ManagerRequest::DeleteAtRestart(delete) => {
                let _guard = self.begin()?;
                let simulate = self.simulating(delete.dry_run);
                serde_json::to_value(manager::delete_at_restart(
                    fs,
                    &zones,
                    self.shell.as_ref(),
                    delete,
                    simulate,
                ))?
            }
            ManagerRequest::SecureDelete(delete) => {
                let _guard = self.begin()?;
                let simulate = self.simulating(delete.dry_run);
                serde_json::to_value(manager::secure_delete(fs, &zones, delete, simulate))?
            }
            ManagerRequest::Move(moving) => {
                let _guard = self.begin()?;
                let simulate = self.simulating(moving.dry_run);
                serde_json::to_value(manager::move_to(fs, &zones, moving, simulate))?
            }
            ManagerRequest::CleanCaches {
                paths,
                manual,
                dry_run,
            } => {
                let _guard = self.begin()?;
                serde_json::to_value(manager::clean_caches(
                    fs,
                    &zones,
                    paths,
                    manual,
                    self.simulating(*dry_run),
                ))?
            }
            ManagerRequest::EmptyRecycleBin { drive, dry_run } => {
                let _guard = self.begin()?;
                let freed = if self.simulating(*dry_run) {
                    0
                } else {
                    self.shell.empty_recycle_bin(drive.as_deref())?
                };
                serde_json::to_value(ActionResult {
                    freed_now_bytes: freed,
                    simulated: self.simulating(*dry_run),
                    ..ActionResult::default()
                })?
            }
            ManagerRequest::CancelAtRestart { path, dry_run } => {
                if !self.simulating(*dry_run) {
                    self.shell.cancel_at_restart(path)?;
                }
                serde_json::Value::Null
            }
            ManagerRequest::RecordMeasure(measure) => {
                if let Some(history) = self.history()?.as_ref() {
                    history.add_measure(measure)?;
                }
                serde_json::Value::Null
            }
            ManagerRequest::Measures { drive, limit } => match self.history()?.as_ref() {
                Some(history) => serde_json::to_value(history.measures(drive, *limit)?)?,
                None => serde_json::json!([]),
            },
            ManagerRequest::System(system) => {
                let _guard = self.begin()?;
                if self.simulating(system.dry_run) {
                    serde_json::to_value(SystemOutcome {
                        simulated: true,
                        ..SystemOutcome::default()
                    })?
                } else {
                    serde_json::to_value(self.cleaner.run(system.task, system.expert_mode)?)?
                }
            }
        };
        Ok(value)
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
        if let Ok(cache) = self.apps_cache.lock() {
            if let Some((read_at, apps)) = cache.as_ref() {
                if read_at.elapsed() < APPS_FRESH_FOR {
                    return Ok(apps.clone());
                }
            }
        }
        self.list_apps_fresh()
    }

    /// Reads the inventory again, whatever its age, and keeps it for [`Engine::list_apps`].
    pub fn list_apps_fresh(&self) -> CoreResult<Vec<InstalledApp>> {
        let started = Instant::now();
        let mut apps = crate::inventory::list_apps(self.registry())?;
        match self.packages.list() {
            Ok(packages) => {
                let provisioned = provisioned_families(self.registry());
                let listed = packages
                    .into_iter()
                    .filter(StorePackage::is_listed)
                    .collect();
                apps.extend(
                    one_per_family(listed)
                        .iter()
                        .map(|p| p.to_app(p.hidden_reason(&provisioned))),
                );
            }
            Err(err) => tracing::warn!(%err, "Store packages not listed"),
        }
        tracing::info!(
            apps = apps.len(),
            ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "applications listed"
        );
        if let Ok(mut cache) = self.apps_cache.lock() {
            *cache = Some((Instant::now(), apps.clone()));
        }
        Ok(apps)
    }

    /// Forgets the inventory kept: the next [`Engine::list_apps`] reads it again.
    pub fn forget_apps(&self) {
        if let Ok(mut cache) = self.apps_cache.lock() {
            *cache = None;
        }
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
        self.scan_orphans_unguarded(options, progress)
    }

    fn scan_orphans_unguarded(
        &self,
        options: &ScanOptions,
        progress: &(dyn Fn(&ScanProgress) + Sync),
    ) -> CoreResult<ScanReport> {
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

    /// Storage map of a drive, read-only (D73): sizes on disk, categories, the leftovers of
    /// the last orphan scan (one is run first, read-only, if none was made by this engine)
    /// and CleanSlate's data folder as the quarantine.
    pub fn disk_map(
        &self,
        request: &DiskMapRequest,
        progress: &(dyn Fn(&DiskMapProgress) + Sync),
    ) -> CoreResult<DiskMap> {
        let _guard = self.begin()?;
        let apps = self.list_apps()?;
        let has_report = self
            .last_report
            .lock()
            .map(|l| !l.is_empty())
            .unwrap_or(false);
        if !has_report && request.scan_leftovers {
            self.scan_orphans_unguarded(&ScanOptions::default(), &|_| {})?;
        }
        let leftovers: Vec<String> = self
            .last_report
            .lock()
            .map(|last| {
                last.values()
                    .filter(|i| {
                        matches!(
                            i.kind,
                            crate::model::LeftoverKind::Folder | crate::model::LeftoverKind::File
                        )
                    })
                    .map(|i| i.location.clone())
                    .collect()
            })
            .unwrap_or_default();
        let installs: Vec<std::path::PathBuf> = apps
            .iter()
            .filter_map(|a| a.install_location.clone())
            .collect();
        let data_dir = self.data_dir();
        let categorizer =
            Categorizer::new(&self.folders, &installs, &leftovers, data_dir.as_deref());
        let space = if self.probe_system {
            crate::native::volume_space(&request.root)
        } else {
            None
        };
        let env = DiskMapEnv {
            fs: self.fs(),
            categorizer: &categorizer,
            cluster: space.map_or(4096, |s| s.cluster.max(1)),
            cancel: &self.cancel,
        };
        let map = crate::disk_map::build(
            &env,
            request,
            space.map_or(0, |s| s.total),
            space.map_or(0, |s| s.free),
            progress,
        );
        tracing::info!(
            root = %request.root.display(),
            bytes = map.root.size_bytes,
            denied = map.denied,
            ms = map.duration_ms,
            cancelled = map.cancelled,
            "disk map done"
        );
        Ok(map)
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
        let key = self.manifest_key.get_or_init(|| match crate::native::manifest_key() {
            // Only the manifests CleanSlate writes with this key are trusted (D92).
            Ok((key, created)) => {
                if created {
                    tracing::info!("key of the manifests created");
                }
                Some(key)
            }
            Err(err) => {
                tracing::warn!(%err, "key of the manifests unavailable: sessions listed, not restored");
                None
            }
        });
        f(&self.quarantine(&root, key.as_deref()))
    }

    fn quarantine<'a>(
        &'a self,
        root: &'a std::path::Path,
        key: Option<&'a [u8]>,
    ) -> Quarantine<'a> {
        Quarantine {
            root,
            fs: self.fs(),
            registry: self.registry(),
            rules: &self.rules,
            tasks: self.tasks.as_ref(),
            services: self.services.as_ref(),
            packages: self.packages.as_ref(),
            key,
        }
    }

    /// `true` once the key of the manifests was read (tests of L-09).
    #[cfg(test)]
    pub(crate) fn manifest_key_loaded(&self) -> bool {
        self.manifest_key.get().is_some()
    }

    /// Replaces the Windows Installer API (tests inject a fake).
    pub fn set_msi_uninstaller(&mut self, msi: Box<dyn MsiUninstaller>) {
        self.msi = msi;
    }

    /// Replaces the Store package store (tests inject a fake).
    pub fn set_package_store(&mut self, packages: Box<dyn PackageStore>) {
        self.packages = packages;
        self.forget_apps();
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
        self.uninstall_with_progress(request, &|_| {})
    }

    /// [`Engine::uninstall`] reporting its progress (EF-21): each step, then
    /// every 250 ms while waiting. MSI products go through the Windows
    /// Installer API (real percentage and action), falling back to
    /// `msiexec /x` if the API fails; `.exe` uninstallers report the
    /// processes of their Job Object, never an invented percentage; Store
    /// packages report the percentage of the removal.
    ///
    /// Each uninstall ends with one log line: simulated, or the outcome with a
    /// summary of the progress events (count, steps, percentages, processes,
    /// duration), so that a fast uninstall can be checked afterwards (D57).
    pub fn uninstall_with_progress(
        &self,
        request: &UninstallRequest,
        progress: &(dyn Fn(&UninstallProgress) + Sync),
    ) -> CoreResult<UninstallResult> {
        let stats = Mutex::new(ProgressStats::default());
        let result = self.uninstall_observed(request, &|event| {
            if let Ok(mut stats) = stats.lock() {
                stats.observe(event);
            }
            progress(event);
        });
        let summary = stats.lock().map(|s| s.summary()).unwrap_or_default();
        // Whatever the path (Win32, MSI, Store): the next inventory is read again (D84).
        if !matches!(&result, Ok(r) if r.simulated) {
            self.forget_apps();
        }
        match &result {
            Ok(r) if r.simulated => tracing::info!(
                app = %r.display_name, command = %r.command, "uninstall simulated: nothing was run"
            ),
            Ok(r) => tracing::info!(
                app = %r.display_name,
                exit_code = ?r.exit_code,
                still_installed = r.still_installed,
                timed_out = r.timed_out,
                cancelled = r.cancelled,
                %summary,
                "uninstall finished"
            ),
            Err(err) => tracing::warn!(app = %request.app_id, %err, %summary, "uninstall failed"),
        }
        result
    }

    fn uninstall_observed(
        &self,
        request: &UninstallRequest,
        progress: &(dyn Fn(&UninstallProgress) + Sync),
    ) -> CoreResult<UninstallResult> {
        let _guard = self.begin()?;
        let started = Instant::now();
        let emit = |step, percent, action, processes| {
            progress(&UninstallProgress {
                step,
                elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                percent,
                action,
                processes,
            });
        };
        let app = self
            .list_apps()?
            .into_iter()
            .find(|a| a.id == request.app_id)
            .ok_or_else(|| CoreError::NotFound {
                location: request.app_id.clone(),
            })?;
        if app.kind == AppKind::Store {
            // Microsoft's or preinstalled: never removed by CleanSlate (D52).
            if app.hidden_reason.is_some() {
                return Err(CoreError::Protected {
                    location: app.display_name,
                });
            }
            return self.uninstall_package(&app, request, &emit);
        }
        crate::uninstall::check_uninstallable(&app)?;
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
            emit(UninstallStep::RestorePoint, None, None, Vec::new());
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
        emit(UninstallStep::Launching, None, None, Vec::new());
        // The Windows Installer API runs in this process: for an application of the
        // administrator; one of this account goes through msiexec with the rights of the user.
        let product_code = app
            .product_code
            .as_deref()
            .filter(|c| app.is_msi && is_guid(c) && !command.as_user);
        let through_api = product_code.and_then(|code| {
            tracing::info!(app = %app.display_name, code, "uninstalling through Windows Installer");
            emit(UninstallStep::Waiting, None, None, Vec::new());
            match self.msi.uninstall(code, &self.cancel, &|percent, action| {
                emit(UninstallStep::Waiting, percent, action, Vec::new())
            }) {
                Ok(code) => Some(code),
                Err(err) => {
                    tracing::warn!(%err, "Windows Installer API failed: falling back to msiexec");
                    None
                }
            }
        });
        let outcome = match through_api {
            Some(code) => {
                result.command = format!(
                    "MsiConfigureProductEx {} (absent)",
                    product_code.unwrap_or_default()
                );
                Ok(RunOutcome {
                    exit_code: Some(code),
                    timed_out: false,
                    cancelled: code == 1602,
                })
            }
            None => {
                tracing::info!(app = %app.display_name, command = %result.command, as_user = command.as_user, "running uninstaller");
                emit(UninstallStep::Waiting, None, None, Vec::new());
                self.runner
                    .run(&command, &self.cancel, UNINSTALL_TIMEOUT, &|processes| {
                        emit(UninstallStep::Waiting, None, None, processes.to_vec())
                    })
            }
        };
        if let Some(seq) = sequence {
            self.restore_points.end(seq, outcome.is_ok());
        }
        let outcome = outcome?;
        result.exit_code = outcome.exit_code;
        result.timed_out = outcome.timed_out;
        result.cancelled = outcome.cancelled;
        emit(UninstallStep::Verifying, None, None, Vec::new());
        // After an uninstall the inventory is read again, never the one kept (D84).
        self.forget_apps();
        result.still_installed = self.list_apps_fresh()?.iter().any(|a| a.id == app.id);
        emit(UninstallStep::Done, None, None, Vec::new());
        Ok(result)
    }

    /// Removes a Store application for the current user only (EF-04), with
    /// the progress reported by Windows. No restore point: it does not cover
    /// packages, and the removal is not run through an uninstaller.
    fn uninstall_package(
        &self,
        app: &InstalledApp,
        request: &UninstallRequest,
        emit: &(dyn Fn(UninstallStep, Option<f64>, Option<String>, Vec<JobProcess>) + Sync),
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
        emit(UninstallStep::Launching, None, None, Vec::new());
        self.packages.remove(&full_name, &|percent| {
            emit(
                UninstallStep::Waiting,
                Some(f64::from(percent)),
                None,
                Vec::new(),
            )
        })?;
        result.exit_code = Some(0);
        emit(UninstallStep::Verifying, None, None, Vec::new());
        result.still_installed = self
            .packages
            .list()?
            .iter()
            .any(|p| p.full_name == full_name);
        emit(UninstallStep::Done, None, None, Vec::new());
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
        self.clean_with_progress(plan, &|_| {})
    }

    /// [`Engine::clean`] reporting its progress, item by item (task 2 of the
    /// owner: a progress bar while the leftovers go to quarantine). In
    /// simulation nothing moves: one final event only.
    pub fn clean_with_progress(
        &self,
        plan: &CleanPlan,
        progress: &(dyn Fn(&CleanProgress) + Sync),
    ) -> CoreResult<Session> {
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
            let bytes: u64 = items.iter().map(|i| i.size_bytes).sum();
            progress(&CleanProgress {
                done: items.len() as u64,
                total: items.len() as u64,
                bytes_done: bytes,
                bytes_total: bytes,
                current: String::new(),
            });
            return Ok(simulate(plan.origin.clone(), &items, Utc::now()));
        }
        let session = self.with_quarantine(|q| {
            q.clean_with_progress(
                plan.origin.clone(),
                &items,
                plan.confirmation.as_deref(),
                Utc::now(),
                progress,
            )
        })?;
        if let Some(history) = self.history()?.as_mut() {
            history.save_session(&session)?;
        }
        // Only moved items leave the report: an item left in place stays on screen
        // and can be tried again (D67).
        if let Ok(mut last) = self.last_report.lock() {
            for item in session.items.iter().filter(|i| {
                matches!(
                    i.status,
                    QuarantineStatus::Quarantined | QuarantineStatus::PendingReboot
                )
            }) {
                last.remove(&item.id);
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

    /// Deletes for good the sessions older than the given age (EF-19), or the one session named (D66).
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
                    .filter(|s| match request.session_id {
                        Some(id) => s.id == id,
                        None => s.created_at <= limit,
                    })
                    .filter(|s| s.status != crate::model::SessionStatus::Purged)
                    .map(|s| s.freed_bytes)
                    .sum())
            })?;
            return Ok(PurgeResult {
                freed_bytes: freed,
                simulated: true,
            });
        }
        let freed = self.with_quarantine(|q| match request.session_id {
            Some(id) => q.purge_session(id),
            None => q.purge(request.older_than_days, Utc::now()),
        })?;
        Ok(PurgeResult {
            freed_bytes: freed,
            simulated: false,
        })
    }
}

/// Summary of the progress events of one uninstall, for the log line that
/// ends it (D57): a fast uninstall leaves a checkable trace.
#[derive(Debug, Default)]
struct ProgressStats {
    events: usize,
    steps: Vec<UninstallStep>,
    min_percent: Option<f64>,
    max_percent: Option<f64>,
    actions: usize,
    processes: Vec<String>,
    last_elapsed_ms: u64,
}

impl ProgressStats {
    fn observe(&mut self, event: &UninstallProgress) {
        self.events += 1;
        if self.steps.last() != Some(&event.step) {
            self.steps.push(event.step);
        }
        if let Some(p) = event.percent {
            self.min_percent = Some(self.min_percent.map_or(p, |m| m.min(p)));
            self.max_percent = Some(self.max_percent.map_or(p, |m| m.max(p)));
        }
        if event.action.is_some() {
            self.actions += 1;
        }
        for process in &event.processes {
            if !self.processes.contains(&process.name) {
                self.processes.push(process.name.clone());
            }
        }
        self.last_elapsed_ms = event.elapsed_ms;
    }

    fn summary(&self) -> String {
        let steps: Vec<String> = self.steps.iter().map(|s| format!("{s:?}")).collect();
        let percent = match (self.min_percent, self.max_percent) {
            (Some(min), Some(max)) => format!("{min:.0}..{max:.0}"),
            _ => "none".to_owned(),
        };
        let processes = if self.processes.is_empty() {
            "none".to_owned()
        } else {
            self.processes.join(",")
        };
        format!(
            "events={} steps={} percent={percent} actions={} processes={processes} duration_ms={}",
            self.events,
            steps.join(">"),
            self.actions,
            self.last_elapsed_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L-09: building the real engine reads no key of the manifests (nothing is written to
    /// `HKLM`); a test engine has its own key from the start.
    #[test]
    fn building_the_real_engine_reads_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::with_system(EngineConfig {
            data_dir: Some(dir.path().to_path_buf()),
            ..EngineConfig::default()
        });
        assert!(!engine.manifest_key_loaded());
        let area = TestRegistryKey::new();
        let fake = Engine::new(
            EngineConfig::default(),
            Box::new(FakeFileSystem::new()),
            Box::new(area.registry()),
        );
        assert!(fake.manifest_key_loaded());
    }
    use crate::fs::FakeFileSystem;
    use crate::model::{
        AppLeftoversRequest, CleanPlan, PurgeRequest, RestoreRequest, UninstallRequest,
    };
    use crate::registry::TestRegistryKey;
    use std::path::Path;

    #[test]
    fn progress_stats_summarize_steps_percentages_and_processes() {
        let event =
            |step, elapsed_ms, percent, action: Option<&str>, names: &[&str]| UninstallProgress {
                step,
                elapsed_ms,
                percent,
                action: action.map(str::to_owned),
                processes: names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| crate::model::JobProcess {
                        pid: 100 + i as u32,
                        name: (*n).to_owned(),
                    })
                    .collect(),
            };
        let mut stats = ProgressStats::default();
        assert_eq!(
            stats.summary(),
            "events=0 steps= percent=none actions=0 processes=none duration_ms=0"
        );
        for e in [
            event(UninstallStep::Launching, 5, None, None, &[]),
            event(UninstallStep::Waiting, 10, Some(0.0), None, &[]),
            event(
                UninstallStep::Waiting,
                400,
                Some(42.4),
                Some("Removing files"),
                &["unins000.exe"],
            ),
            event(
                UninstallStep::Waiting,
                650,
                Some(100.0),
                None,
                &["unins000.exe", "_iu14D2N.tmp"],
            ),
            event(UninstallStep::Verifying, 700, None, None, &[]),
            event(UninstallStep::Done, 720, None, None, &[]),
        ] {
            stats.observe(&e);
        }
        assert_eq!(
            stats.summary(),
            "events=6 steps=Launching>Waiting>Verifying>Done percent=0..100 actions=1 \
             processes=unins000.exe,_iu14D2N.tmp duration_ms=720"
        );
    }

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

    /// D84: the inventory is reused for a minute (the map and the pages ask for it often),
    /// and read again on demand and after an uninstall.
    #[test]
    fn the_inventory_is_kept_a_minute_then_read_again_on_demand() {
        use crate::store::{tests::package, FakePackageStore};
        let area = TestRegistryKey::new();
        let mut engine = Engine::new(
            EngineConfig::default(),
            Box::new(FakeFileSystem::new()),
            Box::new(area.registry()),
        );
        engine.set_package_store(Box::new(FakePackageStore::with(vec![package(
            "Vendor.Notes_abcdefghjk123",
        )])));
        let first = engine.list_apps().unwrap().len();
        // A package installed meanwhile: the inventory kept is used...
        engine.packages = Box::new(FakePackageStore::with(vec![
            package("Vendor.Notes_abcdefghjk123"),
            package("Vendor.Paint_abcdefghjk123"),
        ]));
        assert_eq!(engine.list_apps().unwrap().len(), first);
        // ...until it is read again.
        assert_eq!(engine.list_apps_fresh().unwrap().len(), first + 1);
        engine.forget_apps();
        assert!(engine.apps_cache.lock().unwrap().is_none());
    }

    /// D83: a level of the map computed on a click never runs the long orphan scan first.
    #[test]
    fn a_level_computed_on_a_click_does_not_scan_the_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("VirtualBox VMs")).unwrap();
        std::fs::write(
            dir.path().join("VirtualBox VMs").join("disk.vdi"),
            vec![0u8; 4096],
        )
        .unwrap();
        let area = TestRegistryKey::new();
        let engine = Engine::with_folders(
            EngineConfig::default(),
            Box::new(RealFileSystem),
            Box::new(area.registry()),
            KnownFolders::under(dir.path()),
        );
        let request = DiskMapRequest {
            root: dir.path().join("VirtualBox VMs"),
            max_depth: 3,
            scan_leftovers: false,
        };
        let map = engine.disk_map(&request, &|_| {}).unwrap();
        assert!(map.root.size_bytes >= 4096);
        assert!(
            engine.last_report.lock().unwrap().is_empty(),
            "no orphan scan for a level"
        );
    }

    /// D73: the storage map finds the leftovers by itself (read-only orphan scan) and shows
    /// them as recoverable; a map after a clean sees the folder gone.
    #[test]
    fn disk_map_shows_leftovers_and_follows_a_clean() {
        use crate::disk_map::StorageCategory;
        use crate::fs::strip_verbatim;
        use crate::model::Origin;

        let parent =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\engine-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        let tool = base.join(r"Users\Ana\AppData\Roaming\OldTool");
        std::fs::create_dir_all(&tool).unwrap();
        std::fs::write(tool.join("cache.bin"), vec![0u8; 50_000]).unwrap();
        let area = TestRegistryKey::new();
        let engine = Engine::with_folders(
            EngineConfig {
                dry_run: false,
                data_dir: Some(base.join("data")),
                exclusions: vec![],
            },
            Box::new(RealFileSystem),
            Box::new(area.registry()),
            KnownFolders::under(&base),
        );
        let request = DiskMapRequest {
            root: base.clone(),
            max_depth: 3,
            scan_leftovers: true,
        };
        fn find<'a>(
            n: &'a crate::disk_map::DiskMapNode,
            path: &std::path::Path,
        ) -> Option<&'a crate::disk_map::DiskMapNode> {
            if std::path::Path::new(&n.path) == path {
                return Some(n);
            }
            n.children.iter().find_map(|c| find(c, path))
        }

        let map = engine.disk_map(&request, &|_| {}).unwrap();
        let leftover = find(&map.root, &tool).expect("leftover on the map");
        assert_eq!(leftover.category, StorageCategory::Leftovers);
        assert!(leftover.size_bytes >= 50_000);

        // Quarantine it, then map again: the leftover is gone, the data folder holds it.
        let report = engine
            .scan_orphans(&ScanOptions::default(), &|_| {})
            .unwrap();
        let id = report.groups[0].items[0].id.clone();
        let mut plan = CleanPlan::new(vec![id], Origin::Orphans);
        plan.dry_run = false;
        engine.clean(&plan).unwrap();
        let after = engine.disk_map(&request, &|_| {}).unwrap();
        assert!(find(&after.root, &tool).is_none());
        let data = find(&after.root, &base.join("data")).expect("data folder on the map");
        assert_eq!(data.category, StorageCategory::Quarantine);
        assert!(data.size_bytes >= 50_000);
    }

    /// D67, Installer run of 31 Oct. 2026: an item left in place stays on screen; trying it
    /// again must not fail with « unknown item » (only moved items leave the last report).
    #[test]
    fn an_item_left_in_place_can_be_tried_again() {
        use crate::fs::strip_verbatim;
        use crate::model::{Origin, QuarantineStatus};

        let parent =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\engine-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        let tool = base.join(r"Users\Ana\AppData\Roaming\OldTool");
        std::fs::create_dir_all(&tool).unwrap();
        std::fs::write(tool.join("settings.ini"), b"[x]").unwrap();
        let area = TestRegistryKey::new();
        let engine = Engine::with_folders(
            EngineConfig {
                dry_run: false,
                data_dir: Some(base.join("data")),
                exclusions: vec![],
            },
            Box::new(RealFileSystem),
            Box::new(area.registry()),
            KnownFolders::under(&base),
        );
        let report = engine
            .scan_orphans(&ScanOptions::default(), &|_| {})
            .unwrap();
        let mut plan = CleanPlan::new(vec![report.groups[0].items[0].id.clone()], Origin::Orphans);
        plan.dry_run = false;

        // Gone between the scan and the quarantine: left in place (Failed).
        std::fs::remove_dir_all(&tool).unwrap();
        let first = engine.clean(&plan).unwrap();
        assert_eq!(first.items[0].status, QuarantineStatus::Failed);

        // Back again: the same item is still known and is now moved.
        std::fs::create_dir_all(&tool).unwrap();
        std::fs::write(tool.join("settings.ini"), b"[x]").unwrap();
        let second = engine.clean(&plan).unwrap();
        assert_eq!(second.items[0].status, QuarantineStatus::Quarantined);
        assert!(!tool.exists());

        // Once moved, it is gone from the report.
        assert_eq!(engine.clean(&plan).unwrap_err().code(), "INVALID_INPUT");
    }

    /// Task 2 of the owner: a progress bar while the leftovers go to quarantine. One
    /// event before each item (with the item), then one at the end (all done).
    #[test]
    fn quarantine_reports_its_progress_item_by_item() {
        use crate::fs::strip_verbatim;
        use crate::model::Origin;

        let parent =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\engine-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = strip_verbatim(&dir.path().canonicalize().unwrap());
        for name in ["OldTool", "Gone2019"] {
            let tool = base.join(r"Users\Ana\AppData\Roaming").join(name);
            std::fs::create_dir_all(&tool).unwrap();
            std::fs::write(tool.join("settings.ini"), b"[x]").unwrap();
        }
        let area = TestRegistryKey::new();
        let engine = Engine::with_folders(
            EngineConfig {
                dry_run: false,
                data_dir: Some(base.join("data")),
                exclusions: vec![],
            },
            Box::new(RealFileSystem),
            Box::new(area.registry()),
            KnownFolders::under(&base),
        );
        let report = engine
            .scan_orphans(&ScanOptions::default(), &|_| {})
            .unwrap();
        let ids: Vec<String> = report
            .groups
            .iter()
            .flat_map(|g| g.items.iter().map(|i| i.id.clone()))
            .collect();
        assert!(ids.len() >= 2);
        let mut plan = CleanPlan::new(ids.clone(), Origin::Orphans);
        plan.dry_run = false;

        let events = std::sync::Mutex::new(Vec::new());
        engine
            .clean_with_progress(&plan, &|p| events.lock().unwrap().push(p.clone()))
            .unwrap();
        let events = events.into_inner().unwrap();
        assert_eq!(events.len(), ids.len() + 1);
        for (i, event) in events.iter().take(ids.len()).enumerate() {
            assert_eq!(event.done, i as u64);
            assert_eq!(event.total, ids.len() as u64);
            assert!(!event.current.is_empty());
        }
        let last = events.last().unwrap();
        assert_eq!(last.done, last.total);
        assert_eq!(last.bytes_done, last.bytes_total);
        assert!(last.current.is_empty());
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
                session_id: None,
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
            on_tick: &dyn Fn(&[JobProcess]),
        ) -> CoreResult<crate::uninstall::RunOutcome> {
            on_tick(&[JobProcess {
                pid: 4242,
                name: "unins000.exe".into(),
            }]);
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
        let events = Mutex::new(Vec::new());
        let result = engine
            .uninstall_with_progress(&request(false), &|p| events.lock().unwrap().push(p.clone()))
            .unwrap();
        // EF-21: every step in order, the Job Object processes, no invented percentage.
        let events = events.into_inner().unwrap();
        let mut steps: Vec<UninstallStep> = events.iter().map(|e| e.step).collect();
        steps.dedup();
        assert_eq!(
            steps,
            [
                UninstallStep::RestorePoint,
                UninstallStep::Launching,
                UninstallStep::Waiting,
                UninstallStep::Verifying,
                UninstallStep::Done
            ]
        );
        assert!(events.iter().any(|e| e.processes
            == [JobProcess {
                pid: 4242,
                name: "unins000.exe".into()
            }]));
        assert!(events.iter().all(|e| e.percent.is_none()));
        assert!(events
            .windows(2)
            .all(|w| w[0].elapsed_ms <= w[1].elapsed_ms));
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
        let percents = Mutex::new(Vec::new());
        let removed = engine
            .uninstall_with_progress(&request(false), &|p| {
                if let Some(pct) = p.percent {
                    percents.lock().unwrap().push(pct);
                }
            })
            .unwrap();
        assert!(!removed.simulated && !removed.still_installed);
        // Real percentage of the removal (EF-21).
        assert_eq!(percents.into_inner().unwrap(), [0.0, 40.0, 100.0]);
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

    /// Fake Windows Installer API: reports progress and removes the entry,
    /// or fails (the engine then falls back to `msiexec /x`).
    struct FakeMsi {
        registry: WinRegistry,
        entry: crate::registry::RegKeyPath,
        fail: bool,
    }

    impl MsiUninstaller for FakeMsi {
        fn uninstall(
            &self,
            product_code: &str,
            _: &AtomicBool,
            report: &dyn Fn(Option<f64>, Option<String>),
        ) -> CoreResult<u32> {
            if self.fail {
                return Err(CoreError::InvalidInput(format!(
                    "api failed: {product_code}"
                )));
            }
            report(Some(0.0), Some("Suppression des fichiers".into()));
            report(Some(50.0), Some("Suppression des fichiers".into()));
            self.registry.delete_tree(&self.entry)?;
            Ok(0)
        }
    }

    /// EF-21: an MSI product reports the real percentage and action through
    /// the API; if the API fails, `msiexec /x` runs instead.
    #[test]
    fn msi_progress_through_the_api_or_msiexec_fallback() {
        use crate::inventory::{stable_id, UNINSTALL_KEY};
        use crate::model::RegistryView;
        use crate::registry::{Hive, KeyView, RegKeyPath};
        use std::sync::Arc;

        let code = "{12345678-1234-1234-1234-123456789ABC}";
        for fail in [false, true] {
            let area = TestRegistryKey::new();
            let entry = RegKeyPath::new(Hive::LocalMachine, UNINSTALL_KEY)
                .with_view(KeyView::Force64)
                .join(code);
            let key = area.create(&entry);
            key.set_value("DisplayName", &"MsiApp").unwrap();
            key.set_value("WindowsInstaller", &1u32).unwrap();
            key.set_value("UninstallString", &format!("MsiExec.exe /X{code}").as_str())
                .unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let config = EngineConfig {
                dry_run: false,
                data_dir: None,
                exclusions: vec![],
            };
            let mut engine = Engine::new(
                config,
                Box::new(FakeFileSystem::new()),
                Box::new(area.registry()),
            );
            engine.set_uninstall_backends(
                Box::new(DeletingRunner {
                    registry: area.registry(),
                    entry: entry.clone(),
                    calls: Arc::clone(&calls),
                }),
                Box::new(NoRestorePoints),
            );
            engine.set_msi_uninstaller(Box::new(FakeMsi {
                registry: area.registry(),
                entry: entry.clone(),
                fail,
            }));
            let events = Mutex::new(Vec::new());
            let result = engine
                .uninstall_with_progress(
                    &UninstallRequest {
                        app_id: stable_id(RegistryView::Hklm64, code),
                        quiet: false,
                        create_restore_point: false,
                        dry_run: false,
                    },
                    &|p| events.lock().unwrap().push(p.clone()),
                )
                .unwrap();
            let events = events.into_inner().unwrap();
            assert!(!result.still_installed);
            if fail {
                assert!(
                    result.command.to_lowercase().contains("msiexec.exe"),
                    "{}",
                    result.command
                );
                assert_eq!(calls.lock().unwrap().len(), 1);
                assert!(events.iter().all(|e| e.percent.is_none()));
            } else {
                assert!(result.command.starts_with("MsiConfigureProductEx"));
                assert!(calls.lock().unwrap().is_empty(), "msiexec never started");
                assert!(events.iter().any(|e| e.percent == Some(50.0)
                    && e.action.as_deref() == Some("Suppression des fichiers")));
            }
        }
    }

    /// D52: a Microsoft or preinstalled Store application is listed hidden
    /// and the engine refuses to remove it, even if asked directly.
    #[test]
    fn microsoft_and_preinstalled_store_apps_are_never_removed() {
        use crate::model::HiddenReason;
        use crate::store::{tests::package, FakePackageStore, PROVISIONED_KEY};
        let area = TestRegistryKey::new();
        area.create(
            &crate::registry::RegKeyPath::new(crate::registry::Hive::LocalMachine, PROVISIONED_KEY)
                .join("AD2F1837.HPSystemInformation_2.0.0.0_x64__v10z8vjag6ke6"),
        );
        let config = EngineConfig {
            dry_run: false,
            data_dir: None,
            exclusions: vec![],
        };
        let mut engine = Engine::new(
            config,
            Box::new(FakeFileSystem::new()),
            Box::new(area.registry()),
        );
        engine.set_package_store(Box::new(FakePackageStore::with(vec![
            package("Microsoft.People_8wekyb3d8bbwe"),
            package("AD2F1837.HPSystemInformation_v10z8vjag6ke6"),
            package("Vendor.Notes_abcdefghjk123"),
        ])));
        let apps = engine.list_apps().unwrap();
        let reason = |family: &str| {
            apps.iter()
                .find(|a| a.package_family_name.as_deref() == Some(family))
                .unwrap()
                .hidden_reason
        };
        assert_eq!(
            reason("Microsoft.People_8wekyb3d8bbwe"),
            Some(HiddenReason::MicrosoftApp)
        );
        assert_eq!(
            reason("AD2F1837.HPSystemInformation_v10z8vjag6ke6"),
            Some(HiddenReason::Preinstalled)
        );
        assert_eq!(reason("Vendor.Notes_abcdefghjk123"), None);
        for app in apps.iter().filter(|a| a.hidden_reason.is_some()) {
            let err = engine
                .uninstall(&UninstallRequest {
                    app_id: app.id.clone(),
                    quiet: false,
                    create_restore_point: false,
                    dry_run: false,
                })
                .unwrap_err();
            assert_eq!(err.code(), "PROTECTED", "{}", app.display_name);
        }
        // Nothing was removed from the store.
        assert_eq!(engine.packages.list().unwrap().len(), 3);
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
                    session_id: None,
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
