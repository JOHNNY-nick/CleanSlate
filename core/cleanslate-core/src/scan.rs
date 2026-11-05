//! Scan of orphan leftovers without any application name (EF-05 to EF-11).
//!
//! **Read-only**: the scan lists, measures and scores candidates; it never
//! moves, deletes or writes anything. Every candidate goes through
//! [`SafetyRules`] and reparse points are never traversed (ENF-03).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use rayon::prelude::*;

use crate::error::CoreError;
use crate::evidence::Evidence;
use crate::fs::FileSystem;
use crate::inventory::fnv1a_hex;
use crate::matcher::{group, normalize, score, Facts, KnownSet};
use crate::model::{
    AppKind, Confidence, InstalledApp, ItemError, Leftover, LeftoverKind, ScanOptions, ScanPhase,
    ScanProgress, ScanReport,
};
use crate::registry::{Hive, KeyView, RegData, RegKeyPath, Registry};
use crate::safety::{KnownFolders, SafetyRules, PACKAGE_KEY_PARENTS};
use crate::shortcut::{self, Verdict};
use crate::store::family_label;
use crate::system::{dead_services, dead_tasks, DeadEntry};

/// Minimum delay between two progress events (ENF-06).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// At most one progress event per interval (D03), whatever the number of threads that ask:
/// the walks of the map and of the Gestionnaire ask after each file (L-08).
pub(crate) struct Throttle {
    start: Instant,
    interval_ms: u64,
    /// Milliseconds since `start` before which no event is sent.
    next_ms: AtomicU64,
}

impl Throttle {
    /// A throttle whose first event comes after `interval`.
    pub(crate) fn new(interval: Duration) -> Self {
        let interval_ms = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
        Throttle {
            start: Instant::now(),
            interval_ms,
            next_ms: AtomicU64::new(interval_ms),
        }
    }

    /// `true` for the one caller that may send an event now.
    pub(crate) fn ready(&self) -> bool {
        let now = u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX);
        let next = self.next_ms.load(Ordering::Relaxed);
        now >= next
            && self
                .next_ms
                .compare_exchange(
                    next,
                    now.saturating_add(self.interval_ms),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
    }
}
/// Largest `.lnk` file read.
const MAX_SHORTCUT_LEN: u64 = 1024 * 1024;
/// Depth of the search for shortcuts below each shortcut folder.
const SHORTCUT_DEPTH: usize = 4;
/// Maximum number of item errors kept in a report.
const MAX_ERRORS: usize = 500;

/// Everything a scan reads, injected so that tests use fakes.
pub struct ScanEnv<'a> {
    /// File system (read-only use).
    pub fs: &'a dyn FileSystem,
    /// Registry (read-only use).
    pub registry: &'a dyn Registry,
    /// Roots of the machine.
    pub folders: &'a KnownFolders,
    /// Safety rules.
    pub rules: &'a SafetyRules,
    /// Installed applications.
    pub apps: &'a [InstalledApp],
    /// Executables of services and running processes.
    pub extra_executables: &'a [String],
    /// Set by `cs_cancel`.
    pub cancel: &'a AtomicBool,
    /// Reference time for the age signals.
    pub now: DateTime<Utc>,
    /// Family names of every package of the current user, listed just
    /// before the scan; `None` (or empty) disables the Store leftovers.
    pub installed_packages: Option<&'a [String]>,
}

/// Receives progress events.
pub type ProgressFn<'a> = &'a (dyn Fn(&ScanProgress) + Sync);

/// Registry roots of EF-06, in their 64-bit view.
pub fn registry_roots() -> [RegKeyPath; 3] {
    [
        RegKeyPath::new(Hive::CurrentUser, "Software"),
        RegKeyPath::new(Hive::LocalMachine, "SOFTWARE").with_view(KeyView::Force64),
        RegKeyPath::new(Hive::LocalMachine, r"SOFTWARE\WOW6432Node").with_view(KeyView::Force64),
    ]
}

/// One item to analyze.
#[derive(Debug, Clone)]
struct Candidate {
    kind: LeftoverKind,
    name: String,
    location: String,
    path: Option<PathBuf>,
    key: Option<RegKeyPath>,
    publisher: Option<String>,
    personal: bool,
    in_program_files: bool,
    /// Target of a shortcut.
    target: Option<String>,
    /// Sub-folder of the Start menu holding nothing but dead shortcuts (D20).
    start_menu_folder: bool,
}

/// Emits throttled progress events (at most one per 250 ms).
struct Reporter<'a> {
    sink: ProgressFn<'a>,
    last: Mutex<Instant>,
    current: AtomicU64,
    total: u64,
}

impl<'a> Reporter<'a> {
    fn new(sink: ProgressFn<'a>, total: u64) -> Self {
        Reporter {
            sink,
            last: Mutex::new(Instant::now()),
            current: AtomicU64::new(0),
            total,
        }
    }

    fn emit(&self, phase: ScanPhase, path: &str) {
        (self.sink)(&ScanProgress {
            phase,
            current: self.current.load(Ordering::Relaxed),
            total: self.total,
            path: path.to_owned(),
        });
    }

    /// Called often while work advances: emits when 250 ms have elapsed.
    fn tick(&self, phase: ScanPhase, path: &str, advance: bool) {
        if advance {
            self.current.fetch_add(1, Ordering::Relaxed);
        }
        let Ok(mut last) = self.last.lock() else {
            return;
        };
        if last.elapsed() >= PROGRESS_INTERVAL {
            *last = Instant::now();
            drop(last);
            self.emit(phase, path);
        }
    }
}

/// Collects item errors, never stopping the scan (ENF-09).
#[derive(Default)]
struct Errors(Mutex<Vec<ItemError>>);

impl Errors {
    fn push(&self, err: &CoreError) {
        if let Ok(mut list) = self.0.lock() {
            if list.len() < MAX_ERRORS {
                list.push(ItemError::from(err));
            }
        }
    }

    fn into_vec(self) -> Vec<ItemError> {
        self.0.into_inner().unwrap_or_default()
    }
}

/// Scans the whole machine for orphan leftovers (specs § 6.1).
pub fn scan_orphans(env: &ScanEnv, options: &ScanOptions, progress: ProgressFn) -> ScanReport {
    let started = Instant::now();
    // D78: what Windows still knows of the programs, read once.
    let evidence = Evidence::gather(env.fs, env.registry, env.folders, env.cancel);
    let mut known = KnownSet::new(env.apps, env.extra_executables);
    let errors = Errors::default();
    let listing = Reporter::new(progress, 0);
    listing.emit(ScanPhase::Listing, "");
    for name in present_programs(env, &errors)
        .into_iter()
        .chain(path_programs(env))
    {
        known.add_present_program(&name);
    }

    let mut candidates = Vec::new();
    let personal = [env.folders.documents.clone()];
    let mut roots = env.folders.scan_roots();
    // D68: Documents holds the user's own folders; only the leftovers of an
    // application being uninstalled are looked for there (scan_app_leftovers).
    roots.retain(|r| *r != env.folders.documents);
    roots.sort();
    roots.dedup();
    for root in &roots {
        list_folder_root(
            env,
            &known,
            root,
            personal.contains(root),
            &mut candidates,
            &errors,
        );
        listing.tick(ScanPhase::Listing, &root.to_string_lossy(), false);
    }
    if options.include_registry {
        for root in registry_roots() {
            list_registry_root(env, &known, &root, &mut candidates, &errors);
        }
    }
    if options.include_shortcuts {
        for folder in shortcut_roots(env.folders) {
            list_shortcuts(env, &folder, 0, &mut candidates, &errors);
        }
    }
    candidates.sort_by_key(|c| c.location.to_lowercase());
    candidates.dedup_by_key(|c| c.location.to_lowercase());

    let reporter = Reporter::new(progress, candidates.len() as u64);
    let mut found: Vec<(Option<String>, Leftover)> = candidates
        .par_iter()
        .filter_map(|candidate| {
            if env.cancel.load(Ordering::Relaxed) {
                return None;
            }
            let result = analyze(env, &known, &evidence, candidate, &reporter, &errors);
            reporter.tick(ScanPhase::Analyzing, &candidate.location, true);
            result.map(|leftover| (candidate.publisher.clone(), leftover))
        })
        .collect();
    if options.include_tasks_and_services && !env.cancel.load(Ordering::Relaxed) {
        let dead = dead_tasks(env.fs, env.folders)
            .into_iter()
            .chain(dead_services(env.fs, env.registry, env.folders));
        found.extend(dead.map(|entry| dead_entry_leftover(env, &known, &entry)));
    }

    if let Some(installed) = env.installed_packages.filter(|i| !i.is_empty()) {
        if !env.cancel.load(Ordering::Relaxed) {
            found.extend(package_leftovers(env, installed, None, &reporter, &errors));
        }
    }

    let cancelled = env.cancel.load(Ordering::Relaxed);
    reporter.emit(ScanPhase::Done, "");
    tracing::info!(
        candidates = candidates.len(),
        orphans = found.len(),
        cancelled,
        "scan done"
    );
    ScanReport {
        groups: group(found),
        errors: errors.into_vec(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cancelled,
    }
}

/// Depth 1 of a folder root, and depth 2 below the folder of an installed
/// publisher (e.g. `Roaming\Mozilla\SeaMonkey`).
fn list_folder_root(
    env: &ScanEnv,
    known: &KnownSet,
    root: &Path,
    personal: bool,
    out: &mut Vec<Candidate>,
    errors: &Errors,
) {
    let in_program_files =
        root == env.folders.program_files || root == env.folders.program_files_x86;
    for entry in read_dir_or_record(env.fs, root, errors) {
        if env.cancel.load(Ordering::Relaxed) {
            return;
        }
        if !entry.metadata.is_dir || env.rules.check_path(&entry.path).is_err() {
            continue; // files, reparse points, protected folders
        }
        let name = entry.name.to_string_lossy().into_owned();
        if known.is_installed_publisher(&normalize(&name)) {
            for child in read_dir_or_record(env.fs, &entry.path, errors) {
                if child.metadata.is_dir && env.rules.check_path(&child.path).is_ok() {
                    out.push(folder_candidate(
                        &child.path,
                        Some(name.clone()),
                        personal,
                        in_program_files,
                    ));
                }
            }
        } else {
            out.push(folder_candidate(
                &entry.path,
                None,
                personal,
                in_program_files,
            ));
        }
    }
}

fn folder_candidate(
    path: &Path,
    publisher: Option<String>,
    personal: bool,
    in_program_files: bool,
) -> Candidate {
    Candidate {
        kind: LeftoverKind::Folder,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        location: path.display().to_string(),
        path: Some(path.to_path_buf()),
        key: None,
        publisher,
        personal,
        in_program_files,
        target: None,
        start_menu_folder: false,
    }
}

/// Depth searched for executables when deciding whether a program folder
/// is still present.
const PRESENT_PROGRAM_DEPTH: usize = 3;

/// Names of program folders that still contain executables in `Program
/// Files`, `Program Files (x86)` and `Local\Programs`: software present on
/// disk even without an Uninstall entry (portable or per-user installs).
/// Their data folders must not be proposed.
fn present_programs(env: &ScanEnv, errors: &Errors) -> Vec<String> {
    let roots = [
        &env.folders.program_files,
        &env.folders.program_files_x86,
        &env.folders.local_programs,
    ];
    let mut names = Vec::new();
    for root in roots {
        for entry in read_dir_or_record(env.fs, root, errors) {
            if entry.metadata.is_dir
                && contains_executable(env.fs, &entry.path, PRESENT_PROGRAM_DEPTH)
            {
                names.push(entry.name.to_string_lossy().into_owned());
            }
        }
    }
    names
}

/// Most entries read in one `PATH` folder; beyond, its commands are ignored.
const PATH_DIR_LIMIT: usize = 400;

/// Programs reached through the `PATH` (D68), outside `%WINDIR%`: the folder
/// a `PATH` entry belongs to below a scan root (`Roaming\npm`, `Local\MyCESAM`),
/// and the commands it offers (`pnpm.cmd`, `uv.exe`). Tools installed by npm,
/// pip, uv, cargo or an SDK have no Uninstall entry; their data folders must
/// not be proposed.
fn path_programs(env: &ScanEnv) -> Vec<String> {
    let windir = env.folders.windir.to_string_lossy().to_lowercase();
    let roots = env.folders.scan_roots();
    let mut names = Vec::new();
    for dir in &env.folders.path_dirs {
        let text = dir.to_string_lossy().to_lowercase();
        if text.starts_with(&windir) {
            continue;
        }
        for root in &roots {
            if let Ok(rest) = dir.strip_prefix(root) {
                if let Some(first) = rest.components().next() {
                    names.push(first.as_os_str().to_string_lossy().into_owned());
                }
            }
        }
        let Ok(entries) = env.fs.read_dir(dir) else {
            continue;
        };
        for entry in entries.into_iter().take(PATH_DIR_LIMIT) {
            let is_command = entry.path.extension().is_some_and(|e| {
                ["exe", "cmd", "bat", "ps1"]
                    .iter()
                    .any(|c| e.eq_ignore_ascii_case(c))
            });
            if !entry.metadata.is_dir && is_command {
                if let Some(stem) = entry.path.file_stem() {
                    names.push(stem.to_string_lossy().into_owned());
                }
            }
        }
    }
    names
}

/// `true` if a `.exe` exists below `dir` within `depth` levels (reparse
/// points are not traversed).
fn contains_executable(fs: &dyn FileSystem, dir: &Path, depth: usize) -> bool {
    let Ok(entries) = fs.read_dir(dir) else {
        return false;
    };
    entries.iter().any(|e| {
        if e.metadata.is_reparse_point {
            false
        } else if e.metadata.is_dir {
            depth > 1 && contains_executable(fs, &e.path, depth - 1)
        } else {
            e.path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("exe"))
        }
    })
}

fn read_dir_or_record(
    fs: &dyn FileSystem,
    path: &Path,
    errors: &Errors,
) -> Vec<crate::fs::DirEntry> {
    match fs.read_dir(path) {
        Ok(entries) => entries,
        Err(CoreError::NotFound { .. }) => Vec::new(),
        Err(err) => {
            errors.push(&err);
            Vec::new()
        }
    }
}

/// Depth 1 of a registry root, and depth 2 below an installed publisher.
fn list_registry_root(
    env: &ScanEnv,
    known: &KnownSet,
    root: &RegKeyPath,
    out: &mut Vec<Candidate>,
    errors: &Errors,
) {
    let names = match env.registry.subkeys(root) {
        Ok(names) => names,
        Err(CoreError::NotFound { .. }) => return,
        Err(err) => {
            errors.push(&err);
            return;
        }
    };
    for name in names {
        let key = root.join(&name);
        if env.rules.validate_key(&key).is_err() {
            continue;
        }
        if known.is_installed_publisher(&normalize(&name)) {
            for child in env.registry.subkeys(&key).unwrap_or_default() {
                let child_key = key.join(&child);
                if env.rules.validate_key(&child_key).is_ok() {
                    out.push(key_candidate(child_key, &child, Some(name.clone())));
                }
            }
        } else {
            out.push(key_candidate(key, &name, None));
        }
    }
}

fn key_candidate(key: RegKeyPath, name: &str, publisher: Option<String>) -> Candidate {
    Candidate {
        kind: LeftoverKind::RegistryKey,
        name: name.to_owned(),
        location: key.to_string(),
        path: None,
        key: Some(key),
        publisher,
        personal: false,
        in_program_files: false,
        target: None,
        start_menu_folder: false,
    }
}

/// Folders searched for dead shortcuts: the desktops and the two Start menu
/// roots (D20), each once.
fn shortcut_roots(folders: &KnownFolders) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for folder in folders.shortcut_folders.iter().chain(&folders.start_menu) {
        let text = folder.to_string_lossy().to_lowercase();
        if !roots
            .iter()
            .any(|r| r.to_string_lossy().to_lowercase() == text)
        {
            roots.push(folder.clone());
        }
    }
    roots
}

/// Target of `path` if it is a dead shortcut ([`shortcut::verdict`]).
fn dead_target(env: &ScanEnv, path: &Path) -> Option<String> {
    let bytes = env.fs.read_file(path, MAX_SHORTCUT_LEN).ok()?;
    match shortcut::verdict(&shortcut::parse(&bytes)?, env.fs, env.folders) {
        Verdict::Dead(target) => Some(target),
        Verdict::Alive | Verdict::NotJudged(_) => None,
    }
}

/// Dead `.lnk` shortcuts below a shortcut folder (EF-07). Below a Start
/// menu root only the narrow exception of D20 applies; a sub-folder there
/// that holds nothing but dead shortcuts is proposed too, on its own.
/// Returns `true` when `folder` holds nothing but dead shortcuts.
fn list_shortcuts(
    env: &ScanEnv,
    folder: &Path,
    depth: usize,
    out: &mut Vec<Candidate>,
    errors: &Errors,
) -> bool {
    if depth > SHORTCUT_DEPTH {
        return false;
    }
    let mut only_dead = true;
    for entry in read_dir_or_record(env.fs, folder, errors) {
        if entry.metadata.is_reparse_point {
            only_dead = false;
            continue;
        }
        if entry.metadata.is_dir {
            if list_shortcuts(env, &entry.path, depth + 1, out, errors)
                && env.rules.check_start_menu_folder(&entry.path).is_ok()
            {
                out.push(Candidate {
                    start_menu_folder: true,
                    ..folder_candidate(&entry.path, None, false, false)
                });
            }
            // Its removal is a separate choice: the parent is not emptied by it.
            only_dead = false;
            continue;
        }
        let is_lnk = entry
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
        let allowed = env.rules.check_path(&entry.path).is_ok()
            || env.rules.check_start_menu_shortcut(&entry.path).is_ok();
        let target = (is_lnk && allowed)
            .then(|| dead_target(env, &entry.path))
            .flatten();
        let Some(target) = target else {
            only_dead = false;
            continue;
        };
        out.push(Candidate {
            kind: LeftoverKind::Shortcut,
            name: entry
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            location: entry.path.display().to_string(),
            path: Some(entry.path.clone()),
            key: None,
            publisher: None,
            personal: false,
            in_program_files: false,
            target: Some(target),
            start_menu_folder: false,
        });
    }
    only_dead
}

/// A Start menu sub-folder emptied of its dead shortcuts (D20): proposed
/// on its own, at most Medium; it is quarantined only once empty.
fn start_menu_folder_leftover(candidate: &Candidate) -> Leftover {
    Leftover {
        id: fnv1a_hex(&candidate.location.to_lowercase()),
        kind: LeftoverKind::Folder,
        location: candidate.location.clone(),
        size_bytes: 0,
        last_modified: None,
        confidence: Confidence::Medium,
        reasons: vec![
            "Dossier du menu Démarrer qui ne contient que des raccourcis morts".to_owned(),
            "Mis en quarantaine seulement une fois vide".to_owned(),
        ],
        guessed_app: Some(candidate.name.clone()),
    }
}
/// Data of Store packages that are no longer installed (EF-04, D48): the
/// folder `%LOCALAPPDATA%\Packages\<family>` and its two keys, each passing
/// the narrow exception of `safety.rs` against `installed`. With `only`,
/// the leftovers of that one family just uninstalled (High); otherwise every
/// orphan family (Medium, never prechecked).
fn package_leftovers(
    env: &ScanEnv,
    installed: &[String],
    only: Option<&str>,
    reporter: &Reporter,
    errors: &Errors,
) -> Vec<(Option<String>, Leftover)> {
    let wanted = |family: &str| only.is_none_or(|f| f.eq_ignore_ascii_case(family));
    let (confidence, reason) = match only {
        Some(_) => (
            Confidence::High,
            "Données de l'application du Microsoft Store désinstallée",
        ),
        None => (
            Confidence::Medium,
            "Données d'une application du Microsoft Store qui n'est plus installée",
        ),
    };
    let leftover = |kind, location: String, family: &str, size_bytes, last_modified| {
        let id = fnv1a_hex(&location.to_lowercase());
        (
            None,
            Leftover {
                id,
                kind,
                location,
                size_bytes,
                last_modified,
                confidence,
                reasons: vec![reason.to_owned(), format!("Paquet {family}")],
                guessed_app: Some(family_label(family).to_owned()),
            },
        )
    };
    let mut found = Vec::new();
    let packages = env.folders.local.join("Packages");
    for entry in read_dir_or_record(env.fs, &packages, errors) {
        let family = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        let Some(family) = family.filter(|f| wanted(f)) else {
            continue;
        };
        if env
            .rules
            .validate_package_folder(env.fs, &entry.path, installed)
            .is_err()
        {
            continue;
        }
        let stats = measure(env, &entry.path, reporter, errors);
        found.push(leftover(
            LeftoverKind::Folder,
            entry.path.display().to_string(),
            &family,
            stats.size,
            stats.last_modified,
        ));
    }
    for parent in PACKAGE_KEY_PARENTS {
        let Some(parent) = RegKeyPath::parse(parent) else {
            continue;
        };
        for name in env.registry.subkeys(&parent).unwrap_or_default() {
            let key = parent.join(&name);
            if !wanted(&name) || env.rules.validate_package_key(&key, installed).is_err() {
                continue;
            }
            let size = env
                .registry
                .values(&key)
                .map(|values| {
                    values
                        .iter()
                        .map(|(n, v)| (n.len() * 2) as u64 + data_len(v))
                        .sum()
                })
                .unwrap_or(0);
            found.push(leftover(
                LeftoverKind::RegistryKey,
                key.to_string(),
                &name,
                size,
                None,
            ));
        }
    }
    found
}

/// Folder names too generic to name an application (`…\Vendor\App\bin\x.exe`).
const GENERIC_FOLDERS: [&str; 6] = ["bin", "x64", "x86", "app", "current", "service"];

/// Days a program must have been missing for a task to reach High (D47).
const TASK_ABSENT_DAYS: i64 = 30;

/// Latest time the program could have disappeared: the modification time
/// of its nearest parent folder still present (removing a file or a folder
/// updates its parent). `None` when it cannot be told (drive root, no time).
/// Measured this way, an error can only make the absence look more recent.
fn absent_since(fs: &dyn FileSystem, program: &Path) -> Option<DateTime<Utc>> {
    let parent = program
        .ancestors()
        .skip(1)
        .find(|dir| dir.parent().is_some() && fs.exists(dir))?;
    fs.metadata(parent).ok()?.modified
}

/// A dead scheduled task or service (EF-07, D47) as an item of the report,
/// with its missing program. A task reaches High only when its program has
/// been missing for more than 30 days; a service never goes above Medium
/// (never prechecked); either is Low if its folder belongs to an installed
/// application.
fn dead_entry_leftover(
    env: &ScanEnv,
    known: &KnownSet,
    entry: &DeadEntry,
) -> (Option<String>, Leftover) {
    let program = entry.program.to_string_lossy().into_owned();
    let app = guess_app(&entry.program, env.folders)
        .unwrap_or_else(|| entry.name.trim_start_matches('\\').to_owned());
    let installed = known.attach(&app, Some(&program), &[]).is_some();
    let mut reasons = vec![format!("Exécutable introuvable : {program}")];
    let since = absent_since(env.fs, &entry.program);
    let old = since.is_some_and(|t| env.now - t > chrono::Duration::days(TASK_ABSENT_DAYS));
    if let Some(t) = since.filter(|_| old) {
        reasons.push(format!(
            "Absent depuis au moins {} jours",
            (env.now - t).num_days()
        ));
    }
    let confidence = if installed {
        reasons.push("Appartient peut-être à une application installée".to_owned());
        Confidence::Low
    } else if entry.kind == LeftoverKind::ScheduledTask && old {
        Confidence::High
    } else {
        Confidence::Medium
    };
    reasons.push(
        if entry.kind == LeftoverKind::ScheduledTask {
            "Sa définition XML complète est enregistrée avant la suppression"
        } else {
            "Jamais précoché : configuration et clé enregistrées avant la suppression"
        }
        .to_owned(),
    );
    let leftover = Leftover {
        id: fnv1a_hex(&entry.location.to_lowercase()),
        kind: entry.kind,
        location: entry.location.clone(),
        size_bytes: 0,
        last_modified: since,
        confidence,
        reasons,
        guessed_app: Some(app),
    };
    (None, leftover)
}
/// Application deduced from the folder of a program: the first non-generic
/// folder name below the install roots, else the program's name.
fn guess_app(program: &Path, folders: &KnownFolders) -> Option<String> {
    let roots = [
        &folders.program_files,
        &folders.program_files_x86,
        &folders.local_programs,
        &folders.program_data,
        &folders.roaming,
        &folders.local,
    ];
    for dir in program.ancestors().skip(1) {
        if roots.iter().any(|r| r.as_path() == dir) || dir.parent().is_none() {
            break;
        }
        let name = dir.file_name()?.to_string_lossy();
        if !GENERIC_FOLDERS.iter().any(|g| name.eq_ignore_ascii_case(g)) {
            return Some(name.into_owned());
        }
    }
    program
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
}

/// Similarity above which a candidate carries the name of the uninstalled
/// application (specs § 6.4).
pub const APP_NAME_SIMILARITY: f64 = 0.85;

/// Leftovers of one application just uninstalled (EF-13, specs § 6.4).
///
/// `app` is the record captured before uninstalling. Items attached to
/// another installed application are never proposed. Returns an empty
/// report if `app` is still installed (its files are not leftovers).
pub fn scan_app_leftovers(env: &ScanEnv, app: &InstalledApp, progress: ProgressFn) -> ScanReport {
    let started = Instant::now();
    let errors = Errors::default();
    let reporter = Reporter::new(progress, 0);
    reporter.emit(ScanPhase::Listing, "");
    if env.apps.iter().any(|a| a.id == app.id) {
        reporter.emit(ScanPhase::Done, "");
        return ScanReport::default();
    }
    // A Store application leaves only its package data (EF-04, D48).
    if app.kind == AppKind::Store {
        let found = match (
            env.installed_packages.filter(|i| !i.is_empty()),
            app.package_family_name.as_deref(),
        ) {
            (Some(installed), Some(family)) => {
                package_leftovers(env, installed, Some(family), &reporter, &errors)
            }
            _ => Vec::new(),
        };
        reporter.emit(ScanPhase::Done, "");
        return ScanReport {
            groups: group(found),
            errors: errors.into_vec(),
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            cancelled: env.cancel.load(Ordering::Relaxed),
        };
    }
    let app_name = normalize(&app.display_name);
    let install = app
        .install_location
        .as_ref()
        .map(|p| p.to_string_lossy().trim_end_matches('\\').to_lowercase())
        .filter(|p| !p.is_empty());
    let within_install = |path: &str| {
        install.as_ref().is_some_and(|root| {
            let path = path.to_lowercase();
            path == *root || path.starts_with(&format!(r"{root}\"))
        })
    };
    let carries_name = |name: &str| {
        let normalized = normalize(name);
        !normalized.is_empty()
            && !app_name.is_empty()
            && strsim::jaro_winkler(&normalized, &app_name) >= APP_NAME_SIMILARITY
    };

    let mut known = KnownSet::new(env.apps, env.extra_executables);
    for name in present_programs(env, &errors) {
        // The app's own folder may still hold an executable: not "present".
        if !carries_name(&name) {
            known.add_present_program(&name);
        }
    }
    let mut candidates = Vec::new();
    let mut roots = env.folders.scan_roots();
    roots.sort();
    roots.dedup();
    let personal = env.folders.documents.clone();
    for root in &roots {
        list_folder_root(
            env,
            &known,
            root,
            *root == personal,
            &mut candidates,
            &errors,
        );
    }
    for root in registry_roots() {
        list_registry_root(env, &known, &root, &mut candidates, &errors);
    }
    for folder in shortcut_roots(env.folders) {
        list_shortcuts(env, &folder, 0, &mut candidates, &errors);
    }
    if let Some(location) = &app.install_location {
        if env.fs.exists(location) && env.rules.check_path(location).is_ok() {
            candidates.push(folder_candidate(location, None, false, false));
        }
    }
    candidates.sort_by_key(|c| c.location.to_lowercase());
    candidates.dedup_by_key(|c| c.location.to_lowercase());

    let publisher = app
        .publisher
        .as_deref()
        .map(normalize)
        .filter(|p| !p.is_empty());
    let reporter = Reporter::new(progress, candidates.len() as u64);
    let found: Vec<(Option<String>, Leftover)> = candidates
        .iter()
        .filter_map(|c| {
            reporter.tick(ScanPhase::Analyzing, &c.location, true);
            if env.cancel.load(Ordering::Relaxed) {
                return None;
            }
            if c.start_menu_folder {
                let mut leftover = start_menu_folder_leftover(c);
                leftover.guessed_app = Some(app.display_name.clone());
                return Some((app.publisher.clone(), leftover));
            }
            let path_text = c.path.as_ref().map(|p| p.to_string_lossy().into_owned());
            let in_install = path_text.as_deref().is_some_and(within_install);
            let (mut confidence, reason) = if in_install {
                (
                    Confidence::High,
                    "Ancien dossier d'installation de l'application",
                )
            } else if c.kind == LeftoverKind::RegistryKey
                && app
                    .product_code
                    .as_deref()
                    .is_some_and(|code| c.name.eq_ignore_ascii_case(code))
            {
                (
                    Confidence::High,
                    "Clé portant le code produit de l'application",
                )
            } else if c.target.as_deref().is_some_and(within_install) {
                (
                    Confidence::High,
                    "Raccourci vers l'ancien dossier de l'application",
                )
            } else if carries_name(&c.name) {
                (
                    Confidence::High,
                    "Porte le nom de l'application désinstallée",
                )
            } else if publisher.as_ref().is_some_and(|p| {
                strsim::jaro_winkler(&normalize(&c.name), p) >= crate::matcher::ATTACH_SIMILARITY
            }) && !known.is_installed_publisher(&normalize(&c.name))
            {
                (
                    Confidence::Medium,
                    "Dossier de l'éditeur, sans autre application installée",
                )
            } else {
                return None;
            };
            // Never something that belongs to another installed application.
            if known.attach(&c.name, path_text.as_deref(), &[]).is_some() {
                return None;
            }
            let mut reasons = vec![reason.to_owned()];
            if c.personal {
                confidence = Confidence::Low;
                reasons.push("Dossier personnel possible : jamais précoché".to_owned());
            }
            let (size_bytes, last_modified) = match (&c.path, &c.key) {
                (Some(path), _) => {
                    env.rules.validate_item_path(env.fs, c.kind, path).ok()?;
                    let stats = measure(env, path, &reporter, &errors);
                    if known.attach(&c.name, None, &stats.executables).is_some() {
                        return None;
                    }
                    (stats.size, stats.last_modified)
                }
                (None, Some(key)) => {
                    let values = env.registry.values(key).ok()?;
                    (
                        values
                            .iter()
                            .map(|(n, v)| (n.len() * 2) as u64 + data_len(v))
                            .sum(),
                        None,
                    )
                }
                (None, None) => return None,
            };
            Some((
                app.publisher.clone(),
                Leftover {
                    id: fnv1a_hex(&c.location.to_lowercase()),
                    kind: c.kind,
                    location: c.location.clone(),
                    size_bytes,
                    last_modified,
                    confidence,
                    reasons,
                    guessed_app: Some(app.display_name.clone()),
                },
            ))
        })
        .collect();
    reporter.emit(ScanPhase::Done, "");
    ScanReport {
        groups: group(found),
        errors: errors.into_vec(),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cancelled: env.cancel.load(Ordering::Relaxed),
    }
}

/// Measures, attaches and scores one candidate; `None` when it belongs to
/// an installed application or cannot be validated.
fn analyze(
    env: &ScanEnv,
    known: &KnownSet,
    evidence: &Evidence,
    candidate: &Candidate,
    reporter: &Reporter,
    errors: &Errors,
) -> Option<Leftover> {
    if candidate.start_menu_folder {
        return Some(start_menu_folder_leftover(candidate));
    }
    let path_text = candidate
        .path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    // Cheap rules first: install locations, known executables, names.
    if known
        .attach(&candidate.name, path_text.as_deref(), &[])
        .is_some()
    {
        return None;
    }

    let mut facts = Facts {
        personal_folder: candidate.personal,
        under_installed_publisher: candidate.publisher.is_some(),
        in_program_files: candidate.in_program_files,
        dead_shortcut: candidate.kind == LeftoverKind::Shortcut,
        still_referenced: path_text.as_deref().and_then(|p| evidence.referenced_in(p)),
        package_manager: evidence.package_of(&candidate.name),
        ..Facts::default()
    };
    let size_bytes = match (&candidate.path, &candidate.key) {
        (Some(path), _) => {
            // Final safety check before proposing (reparse points, canonical
            // path); a Start menu shortcut passes only through D20.
            if let Err(err) = env.rules.validate_item_path(env.fs, candidate.kind, path) {
                tracing::debug!(%err, "candidate refused by safety");
                return None;
            }
            let stats = measure(env, path, reporter, errors);
            if known
                .attach(&candidate.name, None, &stats.executables)
                .is_some()
            {
                return None;
            }
            facts.last_modified = stats.last_modified;
            facts.has_executable = !stats.executables.is_empty();
            facts.last_launch = evidence.last_launch(&stats.executables);
            facts.contains_data = stats.has_data;
            stats.size
        }
        (None, Some(key)) => {
            let values = env.registry.values(key).map_err(|e| errors.push(&e)).ok()?;
            let subkeys = env.registry.subkeys(key).map(|s| s.len()).unwrap_or(0);
            facts.registry_counts = Some((values.len(), subkeys));
            values
                .iter()
                .map(|(n, v)| (n.len() * 2) as u64 + data_len(v))
                .sum()
        }
        (None, None) => return None,
    };

    let similarity = known.best_similarity(&normalize(&candidate.name));
    let (_, confidence, reasons) =
        score(candidate.kind, &candidate.name, similarity, &facts, env.now);
    Some(Leftover {
        id: fnv1a_hex(&candidate.location.to_lowercase()),
        kind: candidate.kind,
        location: candidate.location.clone(),
        size_bytes,
        last_modified: facts.last_modified,
        confidence,
        reasons,
        guessed_app: Some(candidate.name.clone()),
    })
}

/// Size of a registry value's data, in bytes.
fn data_len(value: &RegData) -> u64 {
    match value {
        RegData::String(s) | RegData::ExpandString(s) => (s.len() as u64 + 1) * 2,
        RegData::MultiString(v) => v.iter().map(|s| (s.len() as u64 + 1) * 2).sum::<u64>() + 2,
        RegData::Dword(_) => 4,
        RegData::Qword(_) => 8,
        RegData::Binary(b) | RegData::Raw { bytes: b, .. } => b.len() as u64,
    }
}

/// What [`measure`] found below a folder.
#[derive(Default)]
struct Stats {
    size: u64,
    last_modified: Option<DateTime<Utc>>,
    executables: Vec<String>,
    /// A database, a password vault, a mailbox or a virtual disk (D68).
    has_data: bool,
}

/// Extensions of files holding the user's data: their folder is never pre-checked (D68).
const DATA_EXTENSIONS: &[&str] = &[
    "mdf", "ldf", "ndf", "sqlite", "sqlite3", "ibd", "myd", "accdb", "mdb", "kdbx", "pst", "ost",
    "vmdk", "vhd", "vhdx", "qcow2", "vdi",
];

/// `PG_VERSION` marks a PostgreSQL data folder (D68).
fn is_data_file(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("PG_VERSION"))
        || path
            .extension()
            .is_some_and(|e| DATA_EXTENSIONS.iter().any(|d| e.eq_ignore_ascii_case(d)))
}

/// Walks a folder through the [`FileSystem`] trait without traversing
/// reparse points: total size, latest modification, executables.
fn measure(env: &ScanEnv, root: &Path, reporter: &Reporter, errors: &Errors) -> Stats {
    let mut stats = Stats::default();
    if let Ok(meta) = env.fs.metadata(root) {
        if !meta.is_dir {
            stats.size = meta.len;
            stats.last_modified = meta.modified;
            return stats;
        }
    }
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if env.cancel.load(Ordering::Relaxed) {
            break;
        }
        reporter.tick(ScanPhase::Analyzing, &dir.to_string_lossy(), false);
        for entry in read_dir_or_record(env.fs, &dir, errors) {
            let meta = &entry.metadata;
            if meta.is_reparse_point {
                continue; // never traversed, never counted (ENF-03)
            }
            stats.last_modified = stats.last_modified.max(meta.modified);
            if meta.is_dir {
                pending.push(entry.path);
                continue;
            }
            stats.size += meta.len;
            stats.has_data |= is_data_file(&entry.path);
            let is_exe = entry
                .path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"));
            if is_exe {
                stats
                    .executables
                    .push(entry.path.to_string_lossy().into_owned());
            }
        }
    }
    stats
}

/// Local target path of a Windows shortcut (`LinkInfo`), `None` for
/// shortcuts without one (URLs, shell items). See [`shortcut::parse`].
pub fn shortcut_target(bytes: &[u8]) -> Option<String> {
    shortcut::parse(bytes)?.local_target
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use crate::model::{AppKind, InstallerKind, RegistryView};
    use crate::registry::TestRegistryKey;
    use chrono::{Duration as Days, TimeZone};

    pub(crate) fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
    }

    pub(crate) fn app(name: &str, publisher: &str, location: Option<&str>) -> InstalledApp {
        InstalledApp {
            id: name.into(),
            display_name: name.into(),
            publisher: Some(publisher.into()),
            version: None,
            install_date: None,
            install_location: location.map(PathBuf::from),
            estimated_size_kb: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            is_msi: false,
            product_code: None,
            registry_view: RegistryView::Hklm64,
            kind: AppKind::Win32,
            hidden_reason: None,
            installer: InstallerKind::Unknown,
            package_full_name: None,
            package_family_name: None,
            caution: None,
        }
    }

    /// Minimal shortcut file pointing to `target` (MS-SHLLINK layout).
    pub(crate) fn lnk(target: &str) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x4C];
        bytes[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        bytes[0x14..0x18].copy_from_slice(&0x2u32.to_le_bytes()); // HasLinkInfo
        let header = 0x1Cu32;
        let base_offset = header + 4; // after an empty VolumeID placeholder
        let base_len = target.len() as u32 + 1;
        let suffix_offset = base_offset + base_len;
        let total = suffix_offset + 1;
        let mut info = Vec::new();
        for v in [total, header, 1, header, base_offset, 0, suffix_offset] {
            info.extend_from_slice(&v.to_le_bytes());
        }
        info.extend_from_slice(&[0, 0, 0, 0]);
        info.extend_from_slice(target.as_bytes());
        info.push(0);
        info.push(0);
        bytes.extend(info);
        bytes
    }

    /// Fake machine: 5 installed apps (2 from Mozilla) and the leftovers of
    /// 5 uninstalled ones (J3 acceptance fixture).
    pub(crate) fn fixture() -> (FakeFileSystem, Vec<InstalledApp>) {
        let old = Some(now() - Days::days(200));
        let recent = Some(now() - Days::days(1));
        let mut fs = FakeFileSystem::new();
        let r = r"C:\Users\Ana\AppData\Roaming";
        let l = r"C:\Users\Ana\AppData\Local";
        let pf = r"C:\Program Files";
        // Installed applications and their data.
        fs.add_file(format!(r"{pf}\GIMP 2\bin\gimp-2.10.exe"), 5_000_000, old)
            .add_file(format!(r"{r}\GIMP\2.10\gimprc"), 2_000, recent)
            .add_file(format!(r"{pf}\VideoLAN\VLC\vlc.exe"), 3_000_000, old)
            .add_file(format!(r"{r}\vlc\vlcrc"), 1_000, recent)
            .add_file(format!(r"{pf}\7-Zip\7z.exe"), 500_000, old)
            .add_file(format!(r"{r}\Mozilla\Firefox\profiles.ini"), 100, recent)
            .add_file(
                format!(r"{r}\Mozilla\Thunderbird\profiles.ini"),
                100,
                recent,
            )
            .add_file(format!(r"{l}\Mozilla\Firefox\cache.bin"), 9_000, recent)
            // Per-user install without an Uninstall entry: present on disk.
            .add_file(format!(r"{l}\Programs\Obsidian\Obsidian.exe"), 90_000, old)
            .add_file(format!(r"{r}\obsidian\obsidian.json"), 300, old);
        // Leftovers of uninstalled applications.
        fs.add_file(format!(r"{r}\audacity\audacity.cfg"), 4_000, old)
            .add_file(format!(r"{pf}\Audacity\Nyquist\init.lsp"), 40_000, old)
            .add_file(format!(r"{r}\Notepad++\config.xml"), 3_000, old)
            .add_file(format!(r"{r}\obs-studio\global.ini"), 2_000, old)
            .add_file(
                format!(r"{r}\Blender Foundation\Blender\4.2\config\userpref.blend"),
                800_000,
                old,
            )
            .add_file(format!(r"{r}\Mozilla\SeaMonkey\profiles.ini"), 100, old)
            .add_file(format!(r"{l}\HandBrake\logs\activity.txt"), 7_000, old);
        // Protected and personal folders.
        fs.add_file(format!(r"{r}\Microsoft\Teams\x.json"), 1, old)
            .add_file(format!(r"{l}\Temp\junk.tmp"), 1, old)
            .add_file(r"C:\Users\Ana\Documents\Thesis\chapter1.docx", 10_000, old)
            .add_reparse_point(format!(r"{r}\TrapToWindows"));
        // Shortcuts: one dead, one alive.
        fs.add_file_with_content(
            r"C:\Users\Ana\Desktop\Audacity.lnk",
            lnk(r"C:\Program Files\Audacity\audacity.exe"),
            old,
        )
        .add_file_with_content(
            r"C:\Users\Ana\Desktop\GIMP.lnk",
            lnk(r"C:\Program Files\GIMP 2\bin\gimp-2.10.exe"),
            old,
        );
        let apps = vec![
            app(
                "GIMP 2.10.38",
                "The GIMP Team",
                Some(r"C:\Program Files\GIMP 2"),
            ),
            app(
                "VLC media player",
                "VideoLAN",
                Some(r"C:\Program Files\VideoLAN\VLC"),
            ),
            app(
                "7-Zip 24.08",
                "Igor Pavlov",
                Some(r"C:\Program Files\7-Zip"),
            ),
            app(
                "Mozilla Firefox",
                "Mozilla",
                Some(r"C:\Program Files\Mozilla Firefox"),
            ),
            app(
                "Mozilla Thunderbird",
                "Mozilla",
                Some(r"C:\Program Files\Mozilla Thunderbird"),
            ),
        ];
        (fs, apps)
    }

    pub(crate) fn run(
        fs: &FakeFileSystem,
        registry: &dyn Registry,
        apps: &[InstalledApp],
        cancel: &AtomicBool,
        progress: ProgressFn,
    ) -> ScanReport {
        let folders = KnownFolders::under(r"C:\");
        let rules = SafetyRules::new(&folders, &[]);
        let env = ScanEnv {
            fs,
            registry,
            folders: &folders,
            rules: &rules,
            apps,
            extra_executables: &[],
            cancel,
            now: now(),
            installed_packages: None,
        };
        scan_orphans(&env, &ScanOptions::default(), progress)
    }

    /// D68, read-only scan of the owner's PC on 31 Oct. 2026: tools reached through the
    /// PATH (pnpm installed by npm, uv in `.local\bin`, Flutter in an SDK folder) were
    /// proposed and pre-checked; so was the database PostgreSQL left behind.
    #[test]
    fn programs_on_the_path_and_data_folders() {
        let old = Some(now() - Days::days(200));
        let r = r"C:\Users\Ana\AppData\Roaming";
        let l = r"C:\Users\Ana\AppData\Local";
        let mut fs = FakeFileSystem::new();
        fs.add_file(format!(r"{r}\npm\pnpm.cmd"), 300, old)
            .add_file(format!(r"{l}\pnpm-cache\metadata.json"), 300, old)
            .add_file(format!(r"{l}\pnpm-state\state.json"), 300, old)
            .add_file(r"C:\Users\Ana\.local\bin\uv.exe", 9_000, old)
            .add_file(format!(r"{r}\uv\python\list.json"), 300, old)
            .add_file(format!(r"{l}\MyCESAM\tools\flutter\bin\dart.bat"), 300, old)
            .add_file(r"C:\Program Files\PostgreSQL\15\data\PG_VERSION", 3, old)
            .add_file(
                r"C:\Program Files\PostgreSQL\15\data\base\1\1259",
                8_000,
                old,
            )
            .add_file(format!(r"{r}\OldTool\settings.ini"), 300, old)
            .add_file(r"C:\Users\Ana\Documents\Projets\notes.txt", 300, old);
        let mut folders = KnownFolders::under(r"C:\");
        folders.path_dirs = vec![
            PathBuf::from(format!(r"{r}\npm")),
            PathBuf::from(r"C:\Users\Ana\.local\bin"),
            PathBuf::from(format!(r"{l}\MyCESAM\tools\flutter\bin")),
            PathBuf::from(r"C:\Windows\System32"),
        ];
        let rules = SafetyRules::new(&folders, &[]);
        let area = TestRegistryKey::new();
        let registry = area.registry();
        let cancel = AtomicBool::new(false);
        let env = ScanEnv {
            fs: &fs,
            registry: &registry,
            folders: &folders,
            rules: &rules,
            apps: &[],
            extra_executables: &[],
            cancel: &cancel,
            now: now(),
            installed_packages: None,
        };
        let report = scan_orphans(&env, &ScanOptions::default(), &|_| {});
        let items: Vec<&Leftover> = report.groups.iter().flat_map(|g| &g.items).collect();
        let lower: Vec<String> = items.iter().map(|i| i.location.to_lowercase()).collect();
        for forbidden in [r"\npm", "pnpm", r"roaming\uv", "mycesam", "documents"] {
            assert!(
                !lower.iter().any(|l| l.contains(forbidden)),
                "{forbidden} must not be proposed: {lower:?}"
            );
        }
        let at = |suffix: &str| {
            items
                .iter()
                .find(|i| i.location.to_lowercase().ends_with(suffix))
                .unwrap_or_else(|| panic!("{suffix} missing: {lower:?}"))
        };
        assert_eq!(at(r"roaming\oldtool").confidence, Confidence::High);
        let database = at(r"program files\postgresql");
        assert_eq!(database.confidence, Confidence::Medium);
        assert!(
            database.reasons.iter().any(|r| r.contains("données")),
            "{:?}",
            database.reasons
        );
    }

    fn locations(report: &ScanReport) -> Vec<String> {
        report
            .groups
            .iter()
            .flat_map(|g| g.items.iter().map(|i| i.location.clone()))
            .collect()
    }

    /// D20: dead shortcuts of both Start menu roots, and the sub-folders
    /// holding nothing else; Windows tools and not-judged shortcuts stay out.
    #[test]
    fn start_menu_dead_shortcuts_and_emptied_folders() {
        use crate::shortcut::tests::Lnk;
        let (mut fs, apps) = fixture();
        let user = r"C:\Users\Ana\AppData\Roaming\Microsoft\Windows\Start Menu\Programs";
        let common = r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs";
        let dead = Lnk::local(r"C:\Program Files\OldTool\old.exe").build();
        let advertised = Lnk {
            darwin: true,
            ..Lnk::local(r"C:\Program Files\OldTool\old.exe")
        }
        .build();
        fs.add_file_with_content(format!(r"{user}\OldTool\OldTool.lnk"), dead.clone(), None)
            .add_file_with_content(format!(r"{user}\OldTool\Uninstall.lnk"), dead.clone(), None)
            .add_file_with_content(format!(r"{user}\Mixed\Dead.lnk"), dead.clone(), None)
            .add_file_with_content(
                format!(r"{user}\Mixed\Alive.lnk"),
                Lnk::local(r"C:\Program Files\7-Zip\7z.exe").build(),
                None,
            )
            .add_file_with_content(format!(r"{user}\Advertised.lnk"), advertised, None)
            .add_file_with_content(
                format!(r"{common}\Administrative Tools\Old.lnk"),
                dead.clone(),
                None,
            )
            .add_file_with_content(format!(r"{common}\Old Common.lnk"), dead, None)
            .add_dir(format!(r"{user}\Empty"));
        let area = TestRegistryKey::new();
        let report = run(
            &fs,
            &area.registry(),
            &apps,
            &AtomicBool::new(false),
            &|_| {},
        );
        let items: Vec<&Leftover> = report.groups.iter().flat_map(|g| &g.items).collect();
        let at = |path: String| items.iter().find(|i| i.location == path).copied();

        for path in [
            format!(r"{user}\OldTool\OldTool.lnk"),
            format!(r"{user}\OldTool\Uninstall.lnk"),
            format!(r"{user}\Mixed\Dead.lnk"),
            format!(r"{common}\Old Common.lnk"),
        ] {
            assert_eq!(
                at(path.clone()).map(|i| i.kind),
                Some(LeftoverKind::Shortcut),
                "{path}"
            );
        }
        for folder in [format!(r"{user}\OldTool"), format!(r"{user}\Empty")] {
            let item = at(folder.clone()).unwrap_or_else(|| panic!("{folder}"));
            assert_eq!(item.kind, LeftoverKind::Folder);
            assert_eq!(item.confidence, Confidence::Medium);
        }
        for refused in [
            format!(r"{user}\Mixed"),          // still holds a live shortcut
            format!(r"{user}\Advertised.lnk"), // MSI advertised
            format!(r"{common}\Administrative Tools\Old.lnk"),
            format!(r"{common}\Administrative Tools"),
            user.to_owned(),
            common.to_owned(),
        ] {
            assert!(at(refused.clone()).is_none(), "{refused}");
        }
    }

    #[test]
    fn dead_tasks_and_services_confidence() {
        use crate::system::SERVICES_KEY;
        let (mut fs, apps) = fixture();
        let tasks = r"C:\Windows\System32\Tasks";
        let task = |command: &str| {
            let xml = format!(
                "<Task><Actions><Exec><Command>{command}</Command></Exec></Actions></Task>"
            );
            let mut bytes = vec![0xFF, 0xFE];
            bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
            bytes
        };
        fs.add_file_with_content(
            format!(r"{tasks}\AudacityUpdate"),
            task(r"C:\Program Files\Audacity\audacity.exe"),
            None,
        )
        .add_file_with_content(
            format!(r"{tasks}\VLC Helper"),
            task(r"C:\Program Files\VideoLAN\VLC\gone.exe"),
            None,
        )
        // Folders left by removed programs: one untouched for 60 days,
        // one changed 5 days ago.
        .add_dir_modified(r"C:\Program Files\LongGone", now() - Days::days(60))
        .add_dir_modified(r"C:\Program Files\JustGone", now() - Days::days(5))
        .add_file_with_content(
            format!(r"{tasks}\LongGone"),
            task(r"C:\Program Files\LongGone\bin\agent.exe"),
            None,
        )
        .add_file_with_content(
            format!(r"{tasks}\JustGone"),
            task(r"C:\Program Files\JustGone\agent.exe"),
            None,
        );
        let area = TestRegistryKey::new();
        let service = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY).join("OldSvc");
        area.create(&service)
            .set_value("ImagePath", &r#""C:\Program Files\OldVendor\svc.exe" -run"#)
            .unwrap();

        let report = run(
            &fs,
            &area.registry(),
            &apps,
            &AtomicBool::new(false),
            &|_| {},
        );
        let items: Vec<&Leftover> = report.groups.iter().flat_map(|g| &g.items).collect();
        let find = |kind: LeftoverKind, end: &str| {
            items
                .iter()
                .find(|i| i.kind == kind && i.location.ends_with(end))
                .copied()
                .unwrap_or_else(|| panic!("{end} not found"))
        };

        // Orphan task: grouped with the other leftovers of Audacity.
        let audacity = find(LeftoverKind::ScheduledTask, "AudacityUpdate");
        assert_eq!(audacity.confidence, Confidence::Medium);
        assert!(audacity.reasons[0].contains(r"Audacity\audacity.exe"));
        let group = report
            .groups
            .iter()
            .find(|g| g.items.iter().any(|i| i.id == audacity.id))
            .unwrap();
        assert!(group.items.iter().any(|i| i.kind == LeftoverKind::Folder));

        // Task in the folder of an installed application: Low, and said so.
        let vlc = find(LeftoverKind::ScheduledTask, "VLC Helper");
        assert_eq!(vlc.confidence, Confidence::Low);
        assert!(vlc.reasons.iter().any(|r| r.contains("installée")));

        let svc = find(LeftoverKind::Service, r"Services\OldSvc");
        assert_eq!(svc.guessed_app.as_deref(), Some("OldVendor"));
        assert!(svc.reasons.iter().any(|r| r.contains("Jamais précoché")));
        assert_eq!(svc.confidence, Confidence::Medium);

        // A task reaches High only after 30 days of absence.
        let long = find(LeftoverKind::ScheduledTask, r"\LongGone");
        assert_eq!(long.confidence, Confidence::High);
        assert!(long.reasons.iter().any(|r| r.contains("60 jours")));
        let just = find(LeftoverKind::ScheduledTask, r"\JustGone");
        assert_eq!(just.confidence, Confidence::Medium);

        // A service is never High, even long gone; and both can be switched off.
        assert!(items
            .iter()
            .filter(|i| i.kind == LeftoverKind::Service)
            .all(|i| i.confidence != Confidence::High));
        let folders = KnownFolders::under(r"C:\");
        let rules = SafetyRules::new(&folders, &[]);
        let registry = area.registry();
        let env = ScanEnv {
            fs: &fs,
            registry: &registry,
            folders: &folders,
            rules: &rules,
            apps: &apps,
            extra_executables: &[],
            cancel: &AtomicBool::new(false),
            now: now(),
            installed_packages: None,
        };
        let options = ScanOptions {
            include_tasks_and_services: false,
            ..ScanOptions::default()
        };
        let without = scan_orphans(&env, &options, &|_| {});
        assert!(without
            .groups
            .iter()
            .flat_map(|g| &g.items)
            .all(|i| !matches!(i.kind, LeftoverKind::ScheduledTask | LeftoverKind::Service)));
    }

    #[test]
    fn fixture_finds_leftovers_and_never_installed_apps() {
        let (fs, apps) = fixture();
        let area = TestRegistryKey::new();
        let software = RegKeyPath::new(Hive::CurrentUser, "Software");
        area.create(&software.join("Audacity"))
            .set_value("Path", &"x")
            .unwrap();
        area.create(&software.join(r"VideoLAN\VLC"))
            .set_value("Lang", &"fr")
            .unwrap();
        area.create(&software.join(r"Microsoft\Office"))
            .set_value("x", &"y")
            .unwrap();

        let report = run(
            &fs,
            &area.registry(),
            &apps,
            &AtomicBool::new(false),
            &|_| {},
        );
        let found = locations(&report);
        let lower: Vec<String> = found.iter().map(|l| l.to_lowercase()).collect();

        for expected in [
            r"\roaming\audacity",
            r"\program files\audacity",
            r"\roaming\notepad++",
            r"\roaming\obs-studio",
            r"\roaming\blender foundation",
            r"\roaming\mozilla\seamonkey",
            r"\local\handbrake",
            r"hkcu\software\audacity",
            r"\desktop\audacity.lnk",
        ] {
            assert!(
                lower.iter().any(|l| l.ends_with(expected)),
                "missing {expected}: {found:?}"
            );
        }
        for forbidden in [
            "gimp",
            "vlc",
            "videolan",
            "7-zip",
            "firefox",
            "thunderbird",
            "microsoft",
            "temp",
            "trap",
            "obsidian",
            r"roaming\mozilla",
            // D68: personal folders are never proposed by the orphan scan.
            "documents",
        ] {
            assert!(
                !lower
                    .iter()
                    .any(|l| l.contains(forbidden) && !l.contains("seamonkey")),
                "{forbidden} must not be proposed: {found:?}"
            );
        }
        assert!(!report.cancelled);

        let item = |suffix: &str| {
            report
                .groups
                .iter()
                .flat_map(|g| &g.items)
                .find(|i| i.location.to_lowercase().ends_with(suffix))
                .unwrap()
                .clone()
        };
        assert_eq!(item(r"\roaming\audacity").confidence, Confidence::High);

        assert_eq!(item(r"\desktop\audacity.lnk").confidence, Confidence::High);
        assert_eq!(item(r"\roaming\audacity").size_bytes, 4_000);
        // Audacity folder, Program Files folder, key and shortcut share a group.
        let audacity = report
            .groups
            .iter()
            .find(|g| g.label.eq_ignore_ascii_case("audacity"))
            .unwrap();
        assert_eq!(audacity.items.len(), 4);
    }

    #[test]
    fn leftovers_of_a_just_uninstalled_app() {
        let old = Some(now() - Days::days(100));
        let r = r"C:\Users\Ana\AppData\Roaming";
        let mut fs = FakeFileSystem::new();
        fs.add_file(
            r"C:\Program Files\GIMP 2\share\gimp\2.0\tips.xml",
            5_000,
            old,
        )
        .add_file(format!(r"{r}\GIMP\2.10\gimprc"), 2_000, old)
        .add_file(format!(r"{r}\The GIMP Team\prefs.ini"), 10, old)
        .add_file(r"C:\Users\Ana\Documents\GIMP\photo.xcf", 90_000, old)
        .add_file_with_content(
            r"C:\Users\Ana\Desktop\Mon éditeur.lnk",
            lnk(r"C:\Program Files\GIMP 2\bin\gimp-2.10.exe"),
            old,
        )
        .add_file(format!(r"{r}\vlc\vlcrc"), 1_000, old)
        .add_file(r"C:\Program Files\VideoLAN\VLC\vlc.exe", 3_000_000, old);
        let area = TestRegistryKey::new();
        let software = RegKeyPath::new(Hive::CurrentUser, "Software");
        area.create(&software.join("GIMP"))
            .set_value("Lang", &"fr")
            .unwrap();
        area.create(&software.join("VideoLAN"))
            .set_value("x", &"y")
            .unwrap();

        let gimp = app(
            "GIMP 2.10.38",
            "The GIMP Team",
            Some(r"C:\Program Files\GIMP 2"),
        );
        let others = vec![app(
            "VLC media player",
            "VideoLAN",
            Some(r"C:\Program Files\VideoLAN\VLC"),
        )];
        let folders = KnownFolders::under(r"C:\");
        let rules = SafetyRules::new(&folders, &[]);
        let registry = area.registry();
        let cancel = AtomicBool::new(false);
        let env = ScanEnv {
            fs: &fs,
            registry: &registry,
            folders: &folders,
            rules: &rules,
            apps: &others,
            extra_executables: &[],
            cancel: &cancel,
            now: now(),
            installed_packages: None,
        };
        let report = scan_app_leftovers(&env, &gimp, &|_| {});
        let items: Vec<&Leftover> = report.groups.iter().flat_map(|g| &g.items).collect();
        let find = |suffix: &str| {
            items
                .iter()
                .find(|i| i.location.to_lowercase().ends_with(suffix))
                .unwrap_or_else(|| panic!("missing {suffix}: {items:#?}"))
        };
        assert_eq!(find(r"program files\gimp 2").confidence, Confidence::High);
        assert_eq!(find(r"roaming\gimp").confidence, Confidence::High);
        assert_eq!(find(r"hkcu\software\gimp").confidence, Confidence::High);
        assert_eq!(
            find(r"mon éditeur.lnk").reasons[0],
            "Raccourci vers l'ancien dossier de l'application"
        );
        assert_eq!(
            find(r"roaming\the gimp team").confidence,
            Confidence::Medium
        );
        assert_eq!(find(r"documents\gimp").confidence, Confidence::Low);
        assert!(!items
            .iter()
            .any(|i| i.location.to_lowercase().contains("vlc")
                || i.location.to_lowercase().contains("videolan")));
        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].label, "GIMP 2.10.38");

        // Still installed (uninstaller cancelled): nothing is proposed.
        let with_gimp = vec![gimp.clone()];
        let env = ScanEnv {
            apps: &with_gimp,
            ..env
        };
        assert!(scan_app_leftovers(&env, &gimp, &|_| {}).groups.is_empty());
    }

    #[test]
    fn cancelled_scan_returns_partial_flagged_report() {
        let (fs, apps) = fixture();
        let area = TestRegistryKey::new();
        let report = run(
            &fs,
            &area.registry(),
            &apps,
            &AtomicBool::new(true),
            &|_| {},
        );
        assert!(report.cancelled);
        assert!(report.groups.is_empty());
    }

    /// L-08: one event per interval at most, even when four threads ask at once.
    #[test]
    fn the_throttle_lets_one_event_through_per_interval() {
        let throttle = Throttle::new(Duration::from_millis(40));
        assert!(!throttle.ready(), "nothing before the first interval");
        let sent = AtomicU64::new(0);
        let started = Instant::now();
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    while started.elapsed() < Duration::from_millis(210) {
                        if throttle.ready() {
                            sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        let sent = sent.load(Ordering::Relaxed);
        assert!((1..=5).contains(&sent), "{sent} events in 210 ms");
        assert!(Throttle::new(Duration::ZERO).ready());
    }

    #[test]
    fn progress_starts_with_listing_and_ends_with_done() {
        let (fs, apps) = fixture();
        let area = TestRegistryKey::new();
        let events = Mutex::new(Vec::new());
        run(
            &fs,
            &area.registry(),
            &apps,
            &AtomicBool::new(false),
            &|p| events.lock().unwrap().push(p.clone()),
        );
        let events = events.into_inner().unwrap();
        assert_eq!(events.first().unwrap().phase, ScanPhase::Listing);
        let last = events.last().unwrap();
        assert_eq!(last.phase, ScanPhase::Done);
        assert!(last.total > 0);
        assert_eq!(last.current, last.total);
    }

    #[test]
    fn reporter_throttles_to_250_ms() {
        let count = AtomicU64::new(0);
        let sink = |_: &ScanProgress| {
            count.fetch_add(1, Ordering::Relaxed);
        };
        let reporter = Reporter::new(&sink, 1000);
        for _ in 0..1000 {
            reporter.tick(ScanPhase::Analyzing, "x", true);
        }
        assert_eq!(count.load(Ordering::Relaxed), 0, "no event before 250 ms");
        *reporter.last.lock().unwrap() -= PROGRESS_INTERVAL;
        reporter.tick(ScanPhase::Analyzing, "x", false);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn shortcut_parsing() {
        assert_eq!(
            shortcut_target(&lnk(r"C:\A\b.exe")).as_deref(),
            Some(r"C:\A\b.exe")
        );
        assert_eq!(shortcut_target(b"not a link"), None);
        let mut no_info = lnk(r"C:\x.exe");
        no_info[0x14] = 0;
        assert_eq!(shortcut_target(&no_info), None);
        let mut with_idlist = lnk(r"C:\y.exe");
        with_idlist[0x14] = 0x3;
        with_idlist.splice(0x4C..0x4C, [2u8, 0, 0xAA, 0xBB]);
        assert_eq!(shortcut_target(&with_idlist).as_deref(), Some(r"C:\y.exe"));
    }

    #[test]
    fn registry_value_sizes() {
        assert_eq!(data_len(&RegData::String("ab".into())), 6);
        assert_eq!(data_len(&RegData::Dword(1)), 4);
        assert_eq!(data_len(&RegData::Qword(1)), 8);
        assert_eq!(data_len(&RegData::Binary(vec![1, 2, 3])), 3);
        assert_eq!(data_len(&RegData::MultiString(vec!["a".into()])), 6);
    }
}
