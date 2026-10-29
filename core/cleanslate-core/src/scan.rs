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
use crate::fs::FileSystem;
use crate::inventory::fnv1a_hex;
use crate::matcher::{group, normalize, score, Facts, KnownSet};
use crate::model::{
    InstalledApp, ItemError, Leftover, LeftoverKind, ScanOptions, ScanPhase, ScanProgress,
    ScanReport,
};
use crate::registry::{Hive, KeyView, RegData, RegKeyPath, Registry};
use crate::safety::{KnownFolders, SafetyRules};

/// Minimum delay between two progress events (ENF-06).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
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
    let known = KnownSet::new(env.apps, env.extra_executables);
    let errors = Errors::default();
    let listing = Reporter::new(progress, 0);
    listing.emit(ScanPhase::Listing, "");

    let mut candidates = Vec::new();
    let personal = [env.folders.documents.clone()];
    let mut roots = env.folders.scan_roots();
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
        for folder in &env.folders.shortcut_folders {
            list_shortcuts(env, folder, 0, &mut candidates, &errors);
        }
    }
    candidates.sort_by_key(|c| c.location.to_lowercase());
    candidates.dedup_by_key(|c| c.location.to_lowercase());

    let reporter = Reporter::new(progress, candidates.len() as u64);
    let found: Vec<(Option<String>, Leftover)> = candidates
        .par_iter()
        .filter_map(|candidate| {
            if env.cancel.load(Ordering::Relaxed) {
                return None;
            }
            let result = analyze(env, &known, candidate, &reporter, &errors);
            reporter.tick(ScanPhase::Analyzing, &candidate.location, true);
            result.map(|leftover| (candidate.publisher.clone(), leftover))
        })
        .collect();

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
    }
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
    }
}

/// Dead `.lnk` shortcuts below a shortcut folder (EF-07).
fn list_shortcuts(
    env: &ScanEnv,
    folder: &Path,
    depth: usize,
    out: &mut Vec<Candidate>,
    errors: &Errors,
) {
    if depth > SHORTCUT_DEPTH {
        return;
    }
    for entry in read_dir_or_record(env.fs, folder, errors) {
        if entry.metadata.is_reparse_point {
            continue;
        }
        if entry.metadata.is_dir {
            list_shortcuts(env, &entry.path, depth + 1, out, errors);
            continue;
        }
        let is_lnk = entry
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
        if !is_lnk || env.rules.check_path(&entry.path).is_err() {
            continue;
        }
        let Ok(bytes) = env.fs.read_file(&entry.path, MAX_SHORTCUT_LEN) else {
            continue;
        };
        let Some(target) = shortcut_target(&bytes) else {
            continue;
        };
        let local = target.as_bytes().get(1) == Some(&b':');
        if local && !env.fs.exists(Path::new(&target)) {
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
            });
        }
    }
}

/// Measures, attaches and scores one candidate; `None` when it belongs to
/// an installed application or cannot be validated.
fn analyze(
    env: &ScanEnv,
    known: &KnownSet,
    candidate: &Candidate,
    reporter: &Reporter,
    errors: &Errors,
) -> Option<Leftover> {
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
        in_program_files: candidate.in_program_files,
        dead_shortcut: candidate.kind == LeftoverKind::Shortcut,
        ..Facts::default()
    };
    let size_bytes = match (&candidate.path, &candidate.key) {
        (Some(path), _) => {
            // Final safety check before proposing (reparse points, canonical path).
            if let Err(err) = env.rules.validate_path(env.fs, path) {
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
        RegData::Binary(b) => b.len() as u64,
    }
}

/// What [`measure`] found below a folder.
#[derive(Default)]
struct Stats {
    size: u64,
    last_modified: Option<DateTime<Utc>>,
    executables: Vec<String>,
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

/// Target path of a Windows shortcut (MS-SHLLINK: `LinkInfo` →
/// `LocalBasePath` + `CommonPathSuffix`). `None` for shortcuts without a
/// local file target (URLs, shell items).
pub fn shortcut_target(bytes: &[u8]) -> Option<String> {
    let u32_at = |offset: usize| -> Option<u32> {
        bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let u16_at = |offset: usize| -> Option<u16> {
        bytes
            .get(offset..offset + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let c_string = |start: usize| -> Option<String> {
        let rest = bytes.get(start..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        Some(rest[..end].iter().map(|&b| char::from(b)).collect())
    };

    if u32_at(0)? != 0x4C {
        return None;
    }
    let flags = u32_at(0x14)?;
    let mut offset = 0x4C;
    if flags & 0x1 != 0 {
        offset += 2 + usize::from(u16_at(offset)?); // LinkTargetIDList
    }
    if flags & 0x2 == 0 {
        return None; // no LinkInfo
    }
    let info = offset;
    let info_flags = u32_at(info + 8)?;
    if info_flags & 0x1 == 0 {
        return None; // no VolumeIDAndLocalBasePath (network target)
    }
    let base = c_string(info + usize::try_from(u32_at(info + 0x10)?).ok()?)?;
    let suffix = c_string(info + usize::try_from(u32_at(info + 0x18)?).ok()?).unwrap_or_default();
    let target = if suffix.is_empty() {
        base
    } else {
        format!(r"{}\{suffix}", base.trim_end_matches('\\'))
    };
    (!target.is_empty()).then_some(target)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use crate::model::{AppKind, Confidence, InstallerKind, RegistryView};
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
            .add_file(format!(r"{l}\Mozilla\Firefox\cache.bin"), 9_000, recent);
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
        };
        scan_orphans(&env, &ScanOptions::default(), progress)
    }

    fn locations(report: &ScanReport) -> Vec<String> {
        report
            .groups
            .iter()
            .flat_map(|g| g.items.iter().map(|i| i.location.clone()))
            .collect()
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
            r"\documents\thesis",
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
            r"roaming\mozilla",
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
        assert_eq!(item(r"\documents\thesis").confidence, Confidence::Low);
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
