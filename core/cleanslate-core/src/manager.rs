//! Storage manager (D79, EF-22 to EF-35): understand and free the space of the
//! whole machine, personal files included.
//!
//! - **Analyses**, read-only: largest files, categories, old files, empty
//!   folders, duplicates (size then SHA-256), regenerable caches, Recycle Bins,
//!   deletions pending a restart, volumes. Links and junctions are never
//!   followed (nothing counted twice), access-denied folders are skipped and
//!   counted, a walk can be cancelled.
//! - **Actions**, each one authorized by [`Zones`] right before acting (a sensitive
//!   zone needs the expert mode and « CONFIRMER »): delete in mode
//!   [`DeleteMode::Safe`] (Recycle Bin, restorable) or [`DeleteMode::Immediate`]
//!   (for good), move to another drive (space checked, copy, SHA-256 compared,
//!   then the original removed), delete at the next restart, secure deletion,
//!   cleaning of caches (never a file in use).
//!
//! Every request has `dry_run: true` by default (rule 5): nothing changes until
//! the application passes `false` after the confirmation. The Recycle Bin, the
//! deletion at restart and the volumes go through [`SystemShell`], which refuses
//! everything on a test engine ([`NoSystemShell`]).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::fs::FileSystem;
use crate::model::ItemError;
use crate::native::{self, Sha256};
use crate::safety::KnownFolders;
use crate::zones::{ManualAction, Zone, Zones};

fn default_true() -> bool {
    true
}

/// How a deletion is done (EF-23).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeleteMode {
    /// « Sûr » (default): to the Recycle Bin, restorable.
    #[default]
    Safe,
    /// « Immédiat »: deleted for good, the space is free at once.
    Immediate,
}

/// What a file is, for the « Catégories » tab (EF-30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    /// Videos.
    Videos,
    /// Photos and pictures.
    Photos,
    /// Music.
    Music,
    /// Documents.
    Documents,
    /// Games.
    Games,
    /// Developer tools, sources and package caches.
    DevTools,
    /// Virtual machines and disks.
    VirtualMachines,
    /// Phone backups.
    PhoneBackups,
    /// Installers downloaded.
    Installers,
    /// Archives (zip, 7z, iso…).
    Archives,
    /// Programs.
    Applications,
    /// Windows and system.
    System,
    /// Regenerable caches.
    Caches,
    /// Anything else.
    Other,
}

/// One file found by an analysis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Full path.
    pub path: String,
    /// Bytes taken on the disk (compressed and cloud files counted as stored, D73).
    pub size_bytes: u64,
    /// Last modification.
    pub modified: Option<DateTime<Utc>>,
    /// Category.
    pub category: FileCategory,
    /// Sensitive zone: never proposed, by hand in expert mode only (EF-22).
    pub zone: Zone,
}

/// Size of a category and its largest files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryTotal {
    /// Category.
    pub category: FileCategory,
    /// Bytes.
    pub size_bytes: u64,
    /// Number of files.
    pub files: u64,
    /// Largest files of the category, largest first.
    pub largest: Vec<FileEntry>,
}

/// Request of [`analyze`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyzeRequest {
    /// Folder or drive walked (`C:\`).
    pub root: String,
    /// Largest files kept (« Gros fichiers », 100 by default).
    pub largest: usize,
    /// Files untouched for more days than this are « anciens ».
    pub old_after_days: i64,
    /// Downloads untouched for more days than this are « anciens ».
    pub old_downloads_after_days: i64,
    /// Files smaller than this are not candidates for duplicates.
    pub duplicate_min_bytes: u64,
}

impl Default for AnalyzeRequest {
    fn default() -> Self {
        AnalyzeRequest {
            root: r"C:\".into(),
            largest: 100,
            old_after_days: 365,
            old_downloads_after_days: 90,
            duplicate_min_bytes: 1024 * 1024,
        }
    }
}

/// Result of [`analyze`] (read-only).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageAnalysis {
    /// Root walked.
    pub root: String,
    /// Largest files, largest first.
    pub largest: Vec<FileEntry>,
    /// Every category, largest first.
    pub categories: Vec<CategoryTotal>,
    /// Old files (old downloads first, then the others), largest first, at most 2 000.
    pub old_files: Vec<FileEntry>,
    /// Empty folders outside the sensitive zones.
    pub empty_folders: Vec<String>,
    /// Groups of files of the same size (candidates for duplicates, hashed by [`duplicates`]).
    pub duplicate_candidates: Vec<Vec<String>>,
    /// Files counted.
    pub files: u64,
    /// Bytes counted.
    pub size_bytes: u64,
    /// Folders that could not be read (skipped).
    pub denied: u64,
    /// The walk was cancelled: partial result.
    pub cancelled: bool,
}

/// Progress of a walk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagerProgress {
    /// Files counted so far.
    pub files: u64,
    /// Bytes counted so far.
    pub bytes: u64,
    /// Folder being read.
    pub current: String,
}

/// A group of identical files: same size, same SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateGroup {
    /// Size of one copy.
    pub size_bytes: u64,
    /// SHA-256 of the content.
    pub sha256: String,
    /// The copy proposed to keep: the oldest, or the one outside Downloads.
    pub keep: String,
    /// Every copy, the one to keep first.
    pub files: Vec<FileEntry>,
}

/// A regenerable cache (EF-25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheItem {
    /// Name shown, « Fichiers temporaires de l'utilisateur ».
    pub name: String,
    /// Folder.
    pub path: String,
    /// Bytes inside.
    pub size_bytes: u64,
    /// Files inside.
    pub files: u64,
    /// Browser that must be closed first, if any (« Chrome »).
    pub browser: Option<String>,
    /// Sensitive zone (Windows temporary files): never in « Tout libérer ».
    pub zone: Zone,
}

/// The Recycle Bin of one drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecycleBinInfo {
    /// « C: ».
    pub drive: String,
    /// Bytes.
    pub size_bytes: u64,
    /// Items.
    pub items: u64,
}

/// A drive or partition (display only, EF-34).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeInfo {
    /// « C:\ ».
    pub root: String,
    /// Label.
    pub label: String,
    /// File system, « NTFS ».
    pub file_system: String,
    /// Kind: « fixed », « removable », « network », « cdrom ».
    pub kind: String,
    /// Size.
    pub total_bytes: u64,
    /// Free space.
    pub free_bytes: u64,
}

/// A physical disk: its size and the space no partition uses (display only, EF-34).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Number of the disk.
    pub number: u32,
    /// Name, « Samsung SSD 980 ».
    pub name: String,
    /// Size.
    pub size_bytes: u64,
    /// Space outside every partition.
    pub unallocated_bytes: u64,
}

/// Disks of `Get-Disk | Select Number, FriendlyName, Size, AllocatedSize | ConvertTo-Json`
/// (one object or a list).
pub fn parse_disks(json: &str) -> Vec<DiskInfo> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Raw {
        number: u32,
        friendly_name: Option<String>,
        size: u64,
        allocated_size: u64,
    }
    let list: Vec<Raw> = serde_json::from_str::<Vec<Raw>>(json)
        .or_else(|_| serde_json::from_str::<Raw>(json).map(|r| vec![r]))
        .unwrap_or_default();
    list.into_iter()
        .map(|r| DiskInfo {
            number: r.number,
            name: r.friendly_name.unwrap_or_default(),
            size_bytes: r.size,
            unallocated_bytes: r.size.saturating_sub(r.allocated_size),
        })
        .collect()
}

/// Deletion of paths chosen by the user (EF-23, EF-27, EF-28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeleteRequest {
    /// Files or folders.
    pub paths: Vec<String>,
    /// « Sûr » or « Immédiat ».
    pub mode: DeleteMode,
    /// Expert mode and word typed, for the sensitive zones.
    pub manual: ManualAction,
    /// Simulation (default): nothing is deleted.
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

impl Default for DeleteRequest {
    fn default() -> Self {
        DeleteRequest {
            paths: Vec::new(),
            mode: DeleteMode::Safe,
            manual: ManualAction::default(),
            dry_run: true,
        }
    }
}

/// Outcome of one path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutcome {
    /// Path.
    pub path: String,
    /// Bytes concerned.
    pub size_bytes: u64,
    /// Zone of the path.
    pub zone: Option<Zone>,
    /// What happened: « recycle_bin », « deleted », « moved », « at_restart »,
    /// « overwritten », « simulated », « failed ».
    pub done: String,
    /// Why it failed.
    pub error: Option<ItemError>,
}

/// Result of an action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResult {
    /// One line per path.
    pub items: Vec<ActionOutcome>,
    /// Space freed now (deleted for good, moved to another drive).
    pub freed_now_bytes: u64,
    /// Space freed only when the Recycle Bin is emptied.
    pub freed_later_bytes: u64,
    /// Nothing changed: simulation.
    pub simulated: bool,
}

/// Move of paths to another drive, checked (EF-27).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoveRequest {
    /// Files or folders.
    pub paths: Vec<String>,
    /// Destination folder (another drive).
    pub destination: String,
    /// Sensitive zones.
    pub manual: ManualAction,
    /// Simulation (default).
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

impl Default for MoveRequest {
    fn default() -> Self {
        MoveRequest {
            paths: Vec::new(),
            destination: String::new(),
            manual: ManualAction::default(),
            dry_run: true,
        }
    }
}

/// One request of the Gestionnaire, by `op` (D79). Every action has `dry_run: true`
/// by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ManagerRequest {
    /// Largest files, categories, old files, empty folders, candidates for duplicates.
    Analyze(AnalyzeRequest),
    /// Duplicates among the candidates of an analysis (SHA-256).
    Duplicates {
        /// Groups of files of the same size.
        candidates: Vec<Vec<String>>,
    },
    /// Regenerable caches.
    Caches,
    /// Recycle Bins, by drive.
    RecycleBins,
    /// Drives and partitions.
    Volumes,
    /// Physical disks and their unallocated space.
    Disks,
    /// Deletions waiting for the next restart.
    PendingAtRestart,
    /// Items of the « Système » tab.
    SystemItems,
    /// Zone of a path.
    Zone {
        /// Path.
        path: String,
    },
    /// Deletion in mode « Sûr » or « Immédiat ».
    Delete(DeleteRequest),
    /// Deletion at the next restart.
    DeleteAtRestart(DeleteRequest),
    /// Secure deletion (overwritten, then deleted).
    SecureDelete(DeleteRequest),
    /// Move to another drive, checked.
    Move(MoveRequest),
    /// Emptying of caches.
    CleanCaches {
        /// Cache folders.
        paths: Vec<String>,
        /// Sensitive zones.
        #[serde(default)]
        manual: ManualAction,
        /// Simulation (default).
        #[serde(default = "default_true")]
        dry_run: bool,
    },
    /// Emptying of a Recycle Bin (« C: ») or of all.
    EmptyRecycleBin {
        /// Drive, or every drive.
        #[serde(default)]
        drive: Option<String>,
        /// Simulation (default).
        #[serde(default = "default_true")]
        dry_run: bool,
    },
    /// A clean-up of Windows.
    System(crate::sysclean::SystemRequest),
    /// Cancels a deletion at the next restart.
    CancelAtRestart {
        /// Path waiting.
        path: String,
        /// Simulation (default).
        #[serde(default = "default_true")]
        dry_run: bool,
    },
    /// Records a measure of a drive (« Évolution », EF-33).
    RecordMeasure(crate::history::DriveMeasure),
    /// Measures of a drive, oldest first.
    Measures {
        /// « C: ».
        drive: String,
        /// Most recent measures returned.
        #[serde(default = "default_measures")]
        limit: usize,
    },
}

fn default_measures() -> usize {
    365
}

/// What the Windows shell does for the manager; refused on a test engine.
pub trait SystemShell: Send + Sync {
    /// Sends a file or a folder to the Recycle Bin.
    fn recycle(&self, path: &Path) -> CoreResult<()>;
    /// Recycle Bin of each fixed drive.
    fn recycle_bins(&self) -> CoreResult<Vec<RecycleBinInfo>>;
    /// Empties the Recycle Bin of one drive (« C: ») or of all; returns the bytes freed.
    fn empty_recycle_bin(&self, drive: Option<&str>) -> CoreResult<u64>;
    /// Deletes `path` at the next restart (`MoveFileExW`, documented by Windows).
    fn delete_at_restart(&self, path: &Path) -> CoreResult<()>;
    /// Paths waiting for the next restart.
    fn pending_at_restart(&self) -> CoreResult<Vec<String>>;
    /// Cancels the deletion of `path` at the next restart (EF-28).
    fn cancel_at_restart(&self, path: &str) -> CoreResult<()>;
    /// Drives and partitions.
    fn volumes(&self) -> CoreResult<Vec<VolumeInfo>>;
    /// Physical disks and their unallocated space.
    fn disks(&self) -> CoreResult<Vec<DiskInfo>>;
}

/// The shell of a test engine: refuses everything.
pub struct NoSystemShell;

impl SystemShell for NoSystemShell {
    fn recycle(&self, path: &Path) -> CoreResult<()> {
        Err(not_here(path.display().to_string()))
    }
    fn recycle_bins(&self) -> CoreResult<Vec<RecycleBinInfo>> {
        Err(not_here("recycle bin".into()))
    }
    fn empty_recycle_bin(&self, _: Option<&str>) -> CoreResult<u64> {
        Err(not_here("recycle bin".into()))
    }
    fn delete_at_restart(&self, path: &Path) -> CoreResult<()> {
        Err(not_here(path.display().to_string()))
    }
    fn pending_at_restart(&self) -> CoreResult<Vec<String>> {
        Err(not_here("pending deletions".into()))
    }
    fn cancel_at_restart(&self, path: &str) -> CoreResult<()> {
        Err(not_here(path.to_owned()))
    }
    fn volumes(&self) -> CoreResult<Vec<VolumeInfo>> {
        Err(not_here("volumes".into()))
    }
    fn disks(&self) -> CoreResult<Vec<DiskInfo>> {
        Err(not_here("disks".into()))
    }
}

fn not_here(what: String) -> CoreError {
    CoreError::InvalidInput(format!("system shell not available on this engine: {what}"))
}

/// Category of a file, from its path and extension.
pub fn category_of(path: &Path, folders: &KnownFolders) -> FileCategory {
    let text = path.to_string_lossy().to_lowercase();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let under = |p: &Path| text.starts_with(&format!("{}\\", p.to_string_lossy().to_lowercase()));
    if under(&folders.windir) {
        return FileCategory::System;
    }
    if [
        "\\cache\\",
        "\\cache2\\",
        "\\temp\\",
        "\\npm-cache\\",
        "\\pip\\cache\\",
        "\\.gradle\\caches\\",
    ]
    .iter()
    .any(|c| text.contains(c))
    {
        return FileCategory::Caches;
    }
    if text.contains("\\apple computer\\mobilesync\\backup\\")
        || text.contains("\\mobilesync\\backup\\")
    {
        return FileCategory::PhoneBackups;
    }
    if [
        "\\steamapps\\",
        "\\epic games\\",
        "\\gog galaxy\\",
        "\\riot games\\",
        "\\xboxgames\\",
    ]
    .iter()
    .any(|g| text.contains(g))
    {
        return FileCategory::Games;
    }
    match ext.as_str() {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "m4v" | "flv" | "mpg" | "mpeg" => {
            FileCategory::Videos
        }
        "jpg" | "jpeg" | "png" | "gif" | "heic" | "raw" | "cr2" | "nef" | "arw" | "dng" | "bmp"
        | "tif" | "tiff" | "webp" | "psd" => FileCategory::Photos,
        "mp3" | "flac" | "wav" | "aac" | "m4a" | "ogg" | "wma" | "opus" => FileCategory::Music,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "txt"
        | "rtf" | "epub" | "md" => FileCategory::Documents,
        "vhd" | "vhdx" | "vmdk" | "vdi" | "qcow2" | "ova" | "avhdx" => {
            FileCategory::VirtualMachines
        }
        "msi" | "msix" | "msixbundle" | "appx" => FileCategory::Installers,
        "exe" if under(&folders.user_profile.join("Downloads")) => FileCategory::Installers,
        "zip" | "7z" | "rar" | "iso" | "tar" | "gz" | "xz" | "bz2" | "cab" => {
            FileCategory::Archives
        }
        "exe" | "dll" | "sys" | "pak" => FileCategory::Applications,
        "rs" | "cs" | "js" | "ts" | "py" | "java" | "c" | "cpp" | "h" | "pdb" | "obj" | "lib"
        | "rlib" | "nupkg" | "jar" | "whl" => FileCategory::DevTools,
        _ if text.contains("\\node_modules\\")
            || text.contains("\\.cargo\\")
            || text.contains("\\.nuget\\")
            || text.contains("\\target\\debug\\") =>
        {
            FileCategory::DevTools
        }
        _ if under(&folders.program_files) || under(&folders.program_files_x86) => {
            FileCategory::Applications
        }
        _ => FileCategory::Other,
    }
}

/// Walks `root` read-only: largest files, categories, old files, empty folders,
/// candidates for duplicates (EF-29, EF-30). Never follows a link or junction.
pub fn analyze(
    fs: &dyn FileSystem,
    folders: &KnownFolders,
    zones: &Zones,
    request: &AnalyzeRequest,
    now: DateTime<Utc>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(&ManagerProgress) + Sync),
) -> StorageAnalysis {
    let mut out = StorageAnalysis {
        root: request.root.clone(),
        ..StorageAnalysis::default()
    };
    let downloads = folders
        .user_profile
        .join("Downloads")
        .to_string_lossy()
        .to_lowercase();
    let mut categories: BTreeMap<FileCategory, CategoryTotal> = BTreeMap::new();
    let mut by_size: HashMap<u64, Vec<String>> = HashMap::new();
    let mut old: Vec<(bool, FileEntry)> = Vec::new();
    let mut stack = vec![PathBuf::from(&request.root)];
    while let Some(folder) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let Ok(entries) = fs.read_dir(&folder) else {
            out.denied += 1;
            continue;
        };
        if entries.is_empty()
            && folder.parent().is_some()
            && matches!(zones.path_zone(&folder), Ok(Zone::Normal))
        {
            out.empty_folders
                .push(folder.to_string_lossy().into_owned());
        }
        for entry in entries {
            if entry.metadata.is_reparse_point {
                continue; // never followed: nothing counted twice
            }
            if entry.metadata.is_dir {
                stack.push(entry.path);
                continue;
            }
            let size = entry.metadata.on_disk;
            out.files += 1;
            out.size_bytes += size;
            let file = FileEntry {
                path: entry.path.to_string_lossy().into_owned(),
                size_bytes: size,
                modified: entry.metadata.modified,
                category: category_of(&entry.path, folders),
                zone: zones.path_zone(&entry.path).unwrap_or(Zone::Sensitive),
            };
            let total = categories
                .entry(file.category)
                .or_insert_with(|| CategoryTotal {
                    category: file.category,
                    size_bytes: 0,
                    files: 0,
                    largest: Vec::new(),
                });
            total.size_bytes += size;
            total.files += 1;
            keep_largest(&mut total.largest, file.clone(), 50);
            if size >= request.duplicate_min_bytes && file.zone == Zone::Normal {
                by_size.entry(size).or_default().push(file.path.clone());
            }
            if let Some(modified) = file.modified {
                let in_downloads = file
                    .path
                    .to_lowercase()
                    .starts_with(&format!("{downloads}\\"));
                let limit = if in_downloads {
                    request.old_downloads_after_days
                } else {
                    request.old_after_days
                };
                if file.zone == Zone::Normal && now - modified > Duration::days(limit) {
                    old.push((in_downloads, file.clone()));
                }
            }
            keep_largest(&mut out.largest, file, request.largest);
        }
        if out.files.is_multiple_of(2_000) {
            progress(&ManagerProgress {
                files: out.files,
                bytes: out.size_bytes,
                current: folder.to_string_lossy().into_owned(),
            });
        }
    }
    let mut categories: Vec<CategoryTotal> = categories.into_values().collect();
    categories.sort_by_key(|c| std::cmp::Reverse(c.size_bytes));
    out.categories = categories;
    old.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.size_bytes.cmp(&a.1.size_bytes)));
    out.old_files = old.into_iter().map(|(_, f)| f).take(2_000).collect();
    let mut groups: Vec<(u64, Vec<String>)> =
        by_size.into_iter().filter(|(_, v)| v.len() > 1).collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.0 * g.1.len() as u64));
    out.duplicate_candidates = groups.into_iter().take(500).map(|(_, v)| v).collect();
    out.empty_folders.sort();
    progress(&ManagerProgress {
        files: out.files,
        bytes: out.size_bytes,
        current: String::new(),
    });
    out
}

// Keeps the `max` largest files, largest first.
fn keep_largest(list: &mut Vec<FileEntry>, file: FileEntry, max: usize) {
    if max == 0 {
        return;
    }
    if list.len() == max && list.last().is_some_and(|l| l.size_bytes >= file.size_bytes) {
        return;
    }
    let at = list.partition_point(|f| f.size_bytes >= file.size_bytes);
    list.insert(at, file);
    list.truncate(max);
}

/// Hashes the candidates of [`analyze`] and returns the real duplicates (EF-30). The
/// copy to keep is the oldest, or the one outside Downloads; the others are the copies
/// to act on — never every copy of a group (EF-27).
pub fn duplicates(
    fs: &dyn FileSystem,
    folders: &KnownFolders,
    candidates: &[Vec<String>],
    hash: &dyn Fn(&Path) -> CoreResult<String>,
    cancel: &AtomicBool,
) -> Vec<DuplicateGroup> {
    let downloads = folders
        .user_profile
        .join("Downloads")
        .to_string_lossy()
        .to_lowercase();
    let zones = Zones::new(folders);
    let mut out = Vec::new();
    for group in candidates {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut by_hash: HashMap<String, Vec<FileEntry>> = HashMap::new();
        for path in group {
            let p = Path::new(path);
            let (Ok(meta), Ok(digest)) = (fs.metadata(p), hash(p)) else {
                continue;
            };
            by_hash.entry(digest).or_default().push(FileEntry {
                path: path.clone(),
                size_bytes: meta.on_disk,
                modified: meta.modified,
                category: category_of(p, folders),
                zone: zones.path_zone(p).unwrap_or(Zone::Sensitive),
            });
        }
        for (sha256, mut files) in by_hash.into_iter().filter(|(_, f)| f.len() > 1) {
            files.sort_by(|a, b| {
                let in_dl =
                    |f: &FileEntry| f.path.to_lowercase().starts_with(&format!("{downloads}\\"));
                in_dl(a)
                    .cmp(&in_dl(b))
                    .then(a.modified.cmp(&b.modified))
                    .then(a.path.cmp(&b.path))
            });
            out.push(DuplicateGroup {
                size_bytes: files[0].size_bytes,
                sha256,
                keep: files[0].path.clone(),
                files,
            });
        }
    }
    out.sort_by(|a, b| {
        (b.size_bytes * b.files.len() as u64).cmp(&(a.size_bytes * a.files.len() as u64))
    });
    out
}

/// Regenerable caches of the machine and of the user (EF-25), with their size.
pub fn caches(fs: &dyn FileSystem, folders: &KnownFolders) -> Vec<CacheItem> {
    let zones = Zones::new(folders);
    let local = &folders.local;
    let mut places: Vec<(String, PathBuf, Option<&str>)> = vec![
        (
            "Fichiers temporaires de l'utilisateur".into(),
            local.join("Temp"),
            None,
        ),
        (
            "Fichiers temporaires de Windows".into(),
            folders.windir.join("Temp"),
            None,
        ),
        (
            "Miniatures de l'Explorateur".into(),
            local.join(r"Microsoft\Windows\Explorer"),
            None,
        ),
        (
            "Rapports d'erreurs".into(),
            local.join(r"Microsoft\Windows\WER"),
            None,
        ),
        ("Vidages mémoire".into(), local.join("CrashDumps"), None),
        ("Cache npm".into(), local.join("npm-cache"), None),
        ("Cache pip".into(), local.join(r"pip\cache"), None),
        ("Cache Yarn".into(), local.join(r"Yarn\Cache"), None),
        ("Cache NuGet".into(), local.join(r"NuGet\v3-cache"), None),
        (
            "Cache Cargo".into(),
            folders.user_profile.join(r".cargo\registry\cache"),
            None,
        ),
        (
            "Cache Gradle".into(),
            folders.user_profile.join(r".gradle\caches"),
            None,
        ),
    ];
    for (browser, root) in [
        ("Chrome", local.join(r"Google\Chrome\User Data")),
        ("Edge", local.join(r"Microsoft\Edge\User Data")),
        (
            "Brave",
            local.join(r"BraveSoftware\Brave-Browser\User Data"),
        ),
    ] {
        for profile in fs
            .read_dir(&root)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.metadata.is_dir && !e.metadata.is_reparse_point)
        {
            for sub in ["Cache", "Code Cache", "GPUCache"] {
                let dir = profile.path.join(sub);
                if fs.exists(&dir) {
                    let name = profile
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    places.push((
                        format!("Cache de {browser} ({name}, {sub})"),
                        dir,
                        Some(browser),
                    ));
                }
            }
        }
    }
    let firefox = local.join(r"Mozilla\Firefox\Profiles");
    for profile in fs
        .read_dir(&firefox)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.metadata.is_dir && !e.metadata.is_reparse_point)
    {
        let dir = profile.path.join("cache2");
        if fs.exists(&dir) {
            places.push(("Cache de Firefox".into(), dir, Some("Firefox")));
        }
    }
    places
        .into_iter()
        .filter(|(_, p, _)| fs.exists(p))
        .map(|(name, path, browser)| {
            let user_cache = is_user_cache(&path, folders);
            let thumbnails = name.starts_with("Miniatures");
            let (size, files) = folder_size(fs, &path, |f| {
                !thumbnails || f.to_string_lossy().to_lowercase().contains("thumbcache_")
            });
            CacheItem {
                name,
                zone: if user_cache {
                    Zone::Normal
                } else {
                    zones.path_zone(&path).unwrap_or(Zone::Sensitive)
                },
                path: path.to_string_lossy().into_owned(),
                size_bytes: size,
                files,
                browser: browser.map(Into::into),
            }
        })
        .collect()
}

/// Narrow exception of D79 (like D20, D47, D48): a regenerable cache of the user's own
/// profile (temporary files, browsers, thumbnails, error reports, package caches) is a
/// normal zone although the general rules protect any folder named « Temp » (D33). The
/// caches of Windows (`C:\Windows\Temp`) stay sensitive.
pub fn is_user_cache(path: &Path, folders: &KnownFolders) -> bool {
    let text = path.to_string_lossy().trim_end_matches('\\').to_lowercase();
    let local = folders.local.to_string_lossy().to_lowercase();
    let profile = folders.user_profile.to_string_lossy().to_lowercase();
    let below = |base: &str, rest: &str| format!("{base}\\{rest}");
    let exact = [
        below(&local, "temp"),
        below(&local, r"microsoft\windows\explorer"),
        below(&local, r"microsoft\windows\wer"),
        below(&local, "crashdumps"),
        below(&local, "npm-cache"),
        below(&local, r"pip\cache"),
        below(&local, r"yarn\cache"),
        below(&local, r"nuget\v3-cache"),
        below(&profile, r".cargo\registry\cache"),
        below(&profile, r".gradle\caches"),
    ];
    let browser = |root: &str, leaves: &[&str]| {
        text.starts_with(&below(&local, &format!("{root}\\")))
            && leaves
                .iter()
                .any(|leaf| text.ends_with(&format!("\\{leaf}")))
    };
    let chromium = ["cache", "code cache", "gpucache"];
    exact.contains(&text)
        || browser(r"google\chrome\user data", &chromium)
        || browser(r"microsoft\edge\user data", &chromium)
        || browser(r"bravesoftware\brave-browser\user data", &chromium)
        || browser(r"mozilla\firefox\profiles", &["cache2"])
}

/// Bytes and files below `root` whose path passes `keep` (links not followed).
pub fn folder_size(fs: &dyn FileSystem, root: &Path, keep: impl Fn(&Path) -> bool) -> (u64, u64) {
    let (mut size, mut files) = (0, 0);
    let mut stack = vec![root.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for entry in fs.read_dir(&folder).unwrap_or_default() {
            if entry.metadata.is_reparse_point {
                continue;
            }
            if entry.metadata.is_dir {
                stack.push(entry.path);
            } else if keep(&entry.path) {
                size += entry.metadata.on_disk;
                files += 1;
            }
        }
    }
    (size, files)
}

/// Size of a file or folder (links not followed).
pub fn size_of(fs: &dyn FileSystem, path: &Path) -> u64 {
    match fs.metadata(path) {
        Ok(meta) if meta.is_dir => folder_size(fs, path, |_| true).0,
        Ok(meta) => meta.on_disk,
        Err(_) => 0,
    }
}

/// Deletes the paths of `request` (EF-23): each one authorized by `zones` right before
/// acting; « Sûr » sends it to the Recycle Bin, « Immédiat » deletes it for good. A
/// failure on one path never stops the others.
pub fn delete(
    fs: &dyn FileSystem,
    zones: &Zones,
    shell: &dyn SystemShell,
    request: &DeleteRequest,
    simulate: bool,
) -> ActionResult {
    let mut result = ActionResult {
        simulated: simulate,
        ..ActionResult::default()
    };
    for text in &request.paths {
        let path = Path::new(text);
        let mut outcome = ActionOutcome {
            path: text.clone(),
            size_bytes: size_of(fs, path),
            zone: None,
            done: "failed".into(),
            error: None,
        };
        let done = zones
            .authorize_path(fs, path, &request.manual)
            .and_then(|zone| {
                outcome.zone = Some(zone);
                if simulate {
                    return Ok("simulated");
                }
                match request.mode {
                    DeleteMode::Safe => shell.recycle(path).map(|()| "recycle_bin"),
                    DeleteMode::Immediate => remove(path).map(|()| "deleted"),
                }
            });
        match done {
            Ok(how) => {
                outcome.done = how.into();
                match how {
                    "deleted" => result.freed_now_bytes += outcome.size_bytes,
                    "recycle_bin" => result.freed_later_bytes += outcome.size_bytes,
                    _ => {}
                }
            }
            Err(err) => outcome.error = Some(item_error(&err, text)),
        }
        result.items.push(outcome);
    }
    tracing::info!(paths = request.paths.len(), mode = ?request.mode, simulate, freed = result.freed_now_bytes, "manager delete");
    result
}

/// Deletes the paths at the next restart (EF-28): files in use, removed by Windows itself.
pub fn delete_at_restart(
    fs: &dyn FileSystem,
    zones: &Zones,
    shell: &dyn SystemShell,
    request: &DeleteRequest,
    simulate: bool,
) -> ActionResult {
    let mut result = ActionResult {
        simulated: simulate,
        ..ActionResult::default()
    };
    for text in &request.paths {
        let path = Path::new(text);
        let mut outcome = ActionOutcome {
            path: text.clone(),
            size_bytes: size_of(fs, path),
            zone: None,
            done: "failed".into(),
            error: None,
        };
        match zones
            .authorize_path(fs, path, &request.manual)
            .and_then(|zone| {
                outcome.zone = Some(zone);
                if simulate {
                    Ok("simulated")
                } else {
                    shell.delete_at_restart(path).map(|()| "at_restart")
                }
            }) {
            Ok(how) => outcome.done = how.into(),
            Err(err) => outcome.error = Some(item_error(&err, text)),
        }
        result.items.push(outcome);
    }
    result
}

/// Moves the paths to another drive (EF-27): space checked first, copy, every file
/// compared by SHA-256, then the original removed. Nothing is removed if a copy differs.
pub fn move_to(
    fs: &dyn FileSystem,
    zones: &Zones,
    request: &MoveRequest,
    simulate: bool,
) -> ActionResult {
    let mut result = ActionResult {
        simulated: simulate,
        ..ActionResult::default()
    };
    let destination = Path::new(&request.destination);
    let needed: u64 = request
        .paths
        .iter()
        .map(|p| size_of(fs, Path::new(p)))
        .sum();
    let space = native::volume_space(destination).map(|v| v.free);
    for text in &request.paths {
        let path = Path::new(text);
        let mut outcome = ActionOutcome {
            path: text.clone(),
            size_bytes: size_of(fs, path),
            zone: None,
            done: "failed".into(),
            error: None,
        };
        let done = zones
            .authorize_path(fs, path, &request.manual)
            .and_then(|zone| {
                outcome.zone = Some(zone);
                if space.is_some_and(|free| free < needed) {
                    return Err(CoreError::InvalidInput(format!(
                        "not enough space on {}: {needed} bytes needed",
                        request.destination
                    )));
                }
                if same_drive(path, destination) {
                    return Err(CoreError::InvalidInput(
                        "the destination is on the same drive".into(),
                    ));
                }
                if simulate {
                    return Ok("simulated");
                }
                let target = destination.join(
                    path.file_name()
                        .ok_or_else(|| CoreError::InvalidInput(text.clone()))?,
                );
                if target.exists() {
                    return Err(CoreError::InvalidInput(format!(
                        "already exists: {}",
                        target.display()
                    )));
                }
                copy_verified(path, &target)?;
                remove(path).map(|()| "moved")
            });
        match done {
            Ok(how) => {
                if how == "moved" {
                    result.freed_now_bytes += outcome.size_bytes;
                }
                outcome.done = how.into();
            }
            Err(err) => outcome.error = Some(item_error(&err, text)),
        }
        result.items.push(outcome);
    }
    result
}

/// Overwrites each file once with zeros, then deletes it (EF-28). On an SSD this does not
/// guarantee the physical erasure (wear levelling, TRIM): the interface says so and
/// recommends BitLocker.
pub fn secure_delete(
    fs: &dyn FileSystem,
    zones: &Zones,
    request: &DeleteRequest,
    simulate: bool,
) -> ActionResult {
    let mut result = ActionResult {
        simulated: simulate,
        ..ActionResult::default()
    };
    for text in &request.paths {
        let path = Path::new(text);
        let mut outcome = ActionOutcome {
            path: text.clone(),
            size_bytes: size_of(fs, path),
            zone: None,
            done: "failed".into(),
            error: None,
        };
        let done = zones
            .authorize_path(fs, path, &request.manual)
            .and_then(|zone| {
                outcome.zone = Some(zone);
                if simulate {
                    return Ok("simulated");
                }
                overwrite_tree(path)?;
                remove(path).map(|()| "overwritten")
            });
        match done {
            Ok(how) => {
                if how == "overwritten" {
                    result.freed_now_bytes += outcome.size_bytes;
                }
                outcome.done = how.into();
            }
            Err(err) => outcome.error = Some(item_error(&err, text)),
        }
        result.items.push(outcome);
    }
    result
}

/// Empties the caches given (EF-25): the files inside are deleted for good (they come
/// back by themselves); a file in use is skipped, never forced. Sensitive caches need
/// the expert mode like any other manual action.
pub fn clean_caches(
    fs: &dyn FileSystem,
    zones: &Zones,
    paths: &[String],
    manual: &ManualAction,
    simulate: bool,
) -> ActionResult {
    let mut result = ActionResult {
        simulated: simulate,
        ..ActionResult::default()
    };
    for text in paths {
        let root = Path::new(text);
        let mut outcome = ActionOutcome {
            path: text.clone(),
            size_bytes: size_of(fs, root),
            zone: None,
            done: "failed".into(),
            error: None,
        };
        // A user cache (exception of D79) only needs a well-formed path without link.
        let authorized = if is_user_cache(root, &zones.folders) {
            zones
                .authorize_path(fs, root, &ManualAction::default())
                .or_else(|e| match e {
                    CoreError::ExpertRequired { .. } | CoreError::WordRequired { .. } => {
                        Ok(Zone::Normal)
                    }
                    other => Err(other),
                })
        } else {
            zones.authorize_path(fs, root, manual)
        };
        match authorized {
            Ok(zone) => {
                outcome.zone = Some(zone);
                if simulate {
                    outcome.done = "simulated".into();
                } else {
                    let freed = empty_folder_content(fs, root);
                    result.freed_now_bytes += freed;
                    outcome.size_bytes = freed;
                    outcome.done = "deleted".into();
                }
            }
            Err(err) => outcome.error = Some(item_error(&err, text)),
        }
        result.items.push(outcome);
    }
    result
}

// Deletes what is inside `root` (not `root` itself), skipping files in use and links.
fn empty_folder_content(fs: &dyn FileSystem, root: &Path) -> u64 {
    let mut freed = 0;
    for entry in fs.read_dir(root).unwrap_or_default() {
        if entry.metadata.is_reparse_point {
            continue;
        }
        if entry.metadata.is_dir {
            freed += empty_folder_content(fs, &entry.path);
            let _ = std::fs::remove_dir(&entry.path); // only when empty now
        } else if std::fs::remove_file(&entry.path).is_ok() {
            freed += entry.metadata.on_disk;
        }
    }
    freed
}

fn remove(path: &Path) -> CoreResult<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| CoreError::from_io(path, e))?;
    // remove_dir_all never follows a link or junction inside the tree.
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|e| CoreError::from_io(path, e))
}

fn copy_verified(from: &Path, to: &Path) -> CoreResult<()> {
    let meta = std::fs::symlink_metadata(from).map_err(|e| CoreError::from_io(from, e))?;
    if meta.file_type().is_symlink() {
        return Err(CoreError::Protected {
            location: from.display().to_string(),
        });
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to).map_err(|e| CoreError::from_io(to, e))?;
        for entry in std::fs::read_dir(from).map_err(|e| CoreError::from_io(from, e))? {
            let entry = entry.map_err(|e| CoreError::from_io(from, e))?;
            copy_verified(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| CoreError::from_io(to, e))?;
    if Sha256::of_file(from)? != Sha256::of_file(to)? {
        let _ = std::fs::remove_file(to);
        return Err(CoreError::InvalidInput(format!(
            "copy differs from the original: {}",
            from.display()
        )));
    }
    Ok(())
}

fn overwrite_tree(path: &Path) -> CoreResult<()> {
    use std::io::Write;
    let meta = std::fs::symlink_metadata(path).map_err(|e| CoreError::from_io(path, e))?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| CoreError::from_io(path, e))? {
            overwrite_tree(&entry.map_err(|e| CoreError::from_io(path, e))?.path())?;
        }
        return Ok(());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| CoreError::from_io(path, e))?;
    let zeros = vec![0u8; 1 << 20];
    let mut left = meta.len();
    while left > 0 {
        let n = left.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..n])
            .map_err(|e| CoreError::from_io(path, e))?;
        left -= n as u64;
    }
    file.sync_all().map_err(|e| CoreError::from_io(path, e))
}

fn same_drive(a: &Path, b: &Path) -> bool {
    let drive = |p: &Path| p.to_string_lossy().get(..2).map(str::to_lowercase);
    drive(a) == drive(b)
}

fn item_error(err: &CoreError, path: &str) -> ItemError {
    ItemError {
        code: err.code().into(),
        message: err.to_string(),
        path: Some(path.into()),
    }
}

/// `PendingFileRenameOperations` without the deletion of `path` (pairs source, target;
/// an empty target deletes). `None` when `path` was not waiting.
pub fn without_pending(list: &[String], path: &str) -> Option<Vec<String>> {
    let wanted = path.trim_start_matches(r"\??\").to_lowercase();
    let mut kept = Vec::with_capacity(list.len());
    let mut found = false;
    for pair in list.chunks(2) {
        let deletes = pair.get(1).is_none_or(|t| t.is_empty());
        if deletes && pair[0].trim_start_matches(r"\??\").to_lowercase() == wanted {
            found = true;
        } else {
            kept.extend(pair.iter().cloned());
        }
    }
    found.then_some(kept)
}

/// True when every path lies on another drive than the quarantine (EF-24): moving them
/// there frees their drive at once, and they stay restorable.
pub fn other_drive(quarantine: &str, paths: &[String]) -> bool {
    let drive = |p: &str| p.get(..2).map(str::to_lowercase);
    let target = drive(quarantine);
    target.is_some()
        && !paths.is_empty()
        && paths
            .iter()
            .all(|p| drive(p).is_some() && drive(p) != target)
}

/// The shell of Windows (Recycle Bin, deletion at restart, volumes).
pub struct WinShell;

impl SystemShell for WinShell {
    fn recycle(&self, path: &Path) -> CoreResult<()> {
        win::recycle(path)
    }
    fn recycle_bins(&self) -> CoreResult<Vec<RecycleBinInfo>> {
        Ok(win::fixed_drives()
            .into_iter()
            .filter_map(|root| {
                win::query_recycle_bin(&root).map(|(size, items)| RecycleBinInfo {
                    drive: root.trim_end_matches('\\').to_owned(),
                    size_bytes: size,
                    items,
                })
            })
            .collect())
    }
    fn empty_recycle_bin(&self, drive: Option<&str>) -> CoreResult<u64> {
        let before: u64 = self
            .recycle_bins()?
            .iter()
            .filter(|b| {
                drive.is_none_or(|d| b.drive.eq_ignore_ascii_case(d.trim_end_matches('\\')))
            })
            .map(|b| b.size_bytes)
            .sum();
        win::empty_recycle_bin(drive)?;
        Ok(before)
    }
    fn delete_at_restart(&self, path: &Path) -> CoreResult<()> {
        native::delete_at_reboot(path)
    }
    fn pending_at_restart(&self) -> CoreResult<Vec<String>> {
        Ok(win::pending_renames())
    }
    fn cancel_at_restart(&self, path: &str) -> CoreResult<()> {
        win::cancel_rename(path)
    }
    fn volumes(&self) -> CoreResult<Vec<VolumeInfo>> {
        Ok(win::volumes())
    }
    fn disks(&self) -> CoreResult<Vec<DiskInfo>> {
        // Read-only query of the Storage module of Windows, without a command interpreter.
        let windir =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let output = std::process::Command::new(windir.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-Disk | Select-Object Number, FriendlyName, Size, AllocatedSize | ConvertTo-Json -Compress",
            ])
            .output()
            .map_err(|e| CoreError::from_io(&windir, e))?;
        Ok(parse_disks(&String::from_utf8_lossy(&output.stdout)))
    }
}

mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };
    use windows::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHFileOperationW, SHQueryRecycleBinW, FOF_ALLOWUNDO,
        FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHERB_NOCONFIRMATION,
        SHERB_NOPROGRESSUI, SHERB_NOSOUND, SHFILEOPSTRUCTW, SHQUERYRBINFO,
    };

    use super::VolumeInfo;
    use crate::error::{CoreError, CoreResult};
    use crate::registry::{Hive, RegData, RegKeyPath, Registry, WinRegistry};

    fn wide(text: &str) -> Vec<u16> {
        std::ffi::OsStr::new(text)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    pub fn recycle(path: &Path) -> CoreResult<()> {
        // Double NUL-terminated list of one path.
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
            ..Default::default()
        };
        // SAFETY: `op` points to `from`, which lives for the whole call.
        let code = unsafe { SHFileOperationW(&mut op) };
        if code != 0 || op.fAnyOperationsAborted.as_bool() {
            return Err(CoreError::InvalidInput(format!(
                "Recycle Bin refused {} (code {code})",
                path.display()
            )));
        }
        Ok(())
    }

    pub fn query_recycle_bin(root: &str) -> Option<(u64, u64)> {
        let root = wide(root);
        let mut info = SHQUERYRBINFO {
            cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: NUL-terminated root and a sized structure, both alive for the call.
        unsafe { SHQueryRecycleBinW(PCWSTR(root.as_ptr()), &mut info) }.ok()?;
        Some((
            u64::try_from(info.i64Size).unwrap_or(0),
            u64::try_from(info.i64NumItems).unwrap_or(0),
        ))
    }

    pub fn empty_recycle_bin(drive: Option<&str>) -> CoreResult<()> {
        let root = drive.map(|d| wide(&format!("{}\\", d.trim_end_matches('\\'))));
        let ptr = root.as_ref().map_or(PCWSTR::null(), |r| PCWSTR(r.as_ptr()));
        // SAFETY: no window; the root (or null for every drive) lives for the call.
        unsafe {
            SHEmptyRecycleBinW(
                None,
                ptr,
                SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND,
            )
        }
        .map_err(|e| CoreError::InvalidInput(format!("Recycle Bin not emptied: {e}")))
    }

    fn drive_roots() -> Vec<String> {
        let mut buffer = vec![0u16; 512];
        // SAFETY: the buffer is writable for its whole length.
        let len = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) } as usize;
        String::from_utf16_lossy(&buffer[..len.min(buffer.len())])
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn drive_type(root: &str) -> u32 {
        let w = wide(root);
        // SAFETY: NUL-terminated root alive for the call.
        unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) }
    }

    pub fn fixed_drives() -> Vec<String> {
        drive_roots()
            .into_iter()
            .filter(|r| drive_type(r) == 3)
            .collect()
    }

    pub fn volumes() -> Vec<VolumeInfo> {
        drive_roots()
            .into_iter()
            .map(|root| {
                let w = wide(&root);
                let (mut label, mut fs_name) = (vec![0u16; 261], vec![0u16; 261]);
                // SAFETY: NUL-terminated root; both buffers are writable for their length.
                let ok = unsafe {
                    GetVolumeInformationW(
                        PCWSTR(w.as_ptr()),
                        Some(&mut label),
                        None,
                        None,
                        None,
                        Some(&mut fs_name),
                    )
                }
                .is_ok();
                let text = |b: &[u16]| {
                    String::from_utf16_lossy(
                        &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())],
                    )
                };
                let space = crate::native::volume_space(Path::new(&root));
                VolumeInfo {
                    kind: match drive_type(&root) {
                        2 => "removable",
                        3 => "fixed",
                        4 => "network",
                        5 => "cdrom",
                        _ => "other",
                    }
                    .into(),
                    label: if ok { text(&label) } else { String::new() },
                    file_system: if ok { text(&fs_name) } else { String::new() },
                    total_bytes: space.as_ref().map_or(0, |s| s.total),
                    free_bytes: space.as_ref().map_or(0, |s| s.free),
                    root,
                }
            })
            .collect()
    }

    /// Removes the deletion of `path` from `PendingFileRenameOperations`: the documented
    /// list read by Windows at the next start (D79).
    pub fn cancel_rename(path: &str) -> CoreResult<()> {
        let key = RegKeyPath::new(
            Hive::LocalMachine,
            r"SYSTEM\CurrentControlSet\Control\Session Manager",
        );
        let registry = WinRegistry::system();
        let list = registry
            .values(&key)?
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("PendingFileRenameOperations"))
            .and_then(|(_, data)| match data {
                RegData::MultiString(list) => Some(list),
                _ => None,
            })
            .unwrap_or_default();
        let kept = super::without_pending(&list, path).ok_or_else(|| CoreError::NotFound {
            location: path.to_owned(),
        })?;
        registry.set_value(
            &key,
            "PendingFileRenameOperations",
            &RegData::MultiString(kept),
        )
    }

    // `PendingFileRenameOperations`: pairs (source, target); an empty target deletes.
    pub fn pending_renames() -> Vec<String> {
        let key = RegKeyPath::new(
            Hive::LocalMachine,
            r"SYSTEM\CurrentControlSet\Control\Session Manager",
        );
        let values = WinRegistry::system().values(&key).unwrap_or_default();
        values
            .into_iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("PendingFileRenameOperations"))
            .flat_map(|(_, data)| match data {
                RegData::MultiString(list) => list
                    .chunks(2)
                    .filter(|pair| pair.get(1).is_none_or(|t| t.is_empty()))
                    .map(|pair| pair[0].trim_start_matches(r"\??\").to_owned())
                    .collect(),
                _ => Vec::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FakeFileSystem, RealFileSystem};
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap()
    }

    fn machine() -> (FakeFileSystem, KnownFolders) {
        let folders = KnownFolders::under(r"C:\");
        let old = Some(now() - Duration::days(400));
        let recent = Some(now() - Duration::days(2));
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Users\Ana\Videos\film.mp4", 4_000_000_000, recent)
            .add_file(r"C:\Users\Ana\Videos\copie\film.mp4", 4_000_000_000, old)
            .add_file(
                r"C:\Users\Ana\Downloads\setup.exe",
                80_000_000,
                Some(now() - Duration::days(120)),
            )
            .add_file(r"C:\Users\Ana\Pictures\photo.jpg", 3_000_000, old)
            .add_file(r"C:\Users\Ana\Documents\thesis.docx", 2_000_000, recent)
            .add_file(r"C:\Windows\System32\big.dll", 9_000_000_000, old)
            .add_dir(r"C:\Users\Ana\Desktop\Vide")
            .add_reparse_point(r"C:\Users\Ana\Link");
        (fs, folders)
    }

    #[test]
    fn the_analysis_finds_largest_categories_old_files_empty_folders_and_candidates() {
        let (fs, folders) = machine();
        let zones = Zones::new(&folders);
        let request = AnalyzeRequest::default();
        let analysis = analyze(
            &fs,
            &folders,
            &zones,
            &request,
            now(),
            &AtomicBool::new(false),
            &|_| {},
        );

        assert_eq!(analysis.largest[0].path, r"C:\Windows\System32\big.dll");
        assert_eq!(analysis.largest[0].zone, Zone::Sensitive);
        assert_eq!(analysis.files, 6);
        let videos = analysis
            .categories
            .iter()
            .find(|c| c.category == FileCategory::Videos)
            .unwrap();
        assert_eq!(videos.size_bytes, 8_000_000_000);
        assert!(analysis
            .categories
            .iter()
            .any(|c| c.category == FileCategory::Installers));
        assert!(analysis
            .categories
            .iter()
            .any(|c| c.category == FileCategory::System));
        // Old: the download first (90 days), then the others (a year); never Windows.
        assert_eq!(
            analysis.old_files[0].path,
            r"C:\Users\Ana\Downloads\setup.exe"
        );
        assert!(analysis.old_files.iter().all(|f| f.zone == Zone::Normal));
        assert!(analysis
            .old_files
            .iter()
            .any(|f| f.path.ends_with("photo.jpg")));
        assert_eq!(
            analysis.empty_folders,
            vec![r"C:\Users\Ana\Desktop\Vide".to_string()]
        );
        assert_eq!(analysis.duplicate_candidates.len(), 1);
        assert_eq!(analysis.duplicate_candidates[0].len(), 2);
    }

    #[test]
    fn duplicates_keep_one_copy_the_oldest_or_outside_downloads() {
        let (fs, folders) = machine();
        let candidates = vec![vec![
            r"C:\Users\Ana\Videos\film.mp4".to_string(),
            r"C:\Users\Ana\Videos\copie\film.mp4".to_string(),
        ]];
        let groups = duplicates(
            &fs,
            &folders,
            &candidates,
            &|_| Ok("same".into()),
            &AtomicBool::new(false),
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].keep, r"C:\Users\Ana\Videos\copie\film.mp4"); // the oldest
        assert_eq!(groups[0].files.len(), 2);

        let different = duplicates(
            &fs,
            &folders,
            &candidates,
            &|p| Ok(p.display().to_string()),
            &AtomicBool::new(false),
        );
        assert!(different.is_empty());
    }

    /// EF-23 and A.13: a deletion is simulated by default; outside expert mode a
    /// sensitive path is refused; the test shell refuses the Recycle Bin.
    #[test]
    fn deletion_respects_the_zones_the_mode_and_the_simulation() {
        let (fs, folders) = machine();
        let zones = Zones::new(&folders);
        let request = DeleteRequest {
            paths: vec![
                r"C:\Users\Ana\Videos\film.mp4".into(),
                r"C:\Windows\System32\big.dll".into(),
            ],
            ..DeleteRequest::default()
        };
        assert!(request.dry_run);
        let simulated = delete(&fs, &zones, &NoSystemShell, &request, true);
        assert!(simulated.simulated);
        assert_eq!(simulated.items[0].done, "simulated");
        assert_eq!(
            simulated.items[1].error.as_ref().unwrap().code,
            "EXPERT_REQUIRED"
        );

        let real = delete(&fs, &zones, &NoSystemShell, &request, false);
        assert_eq!(real.items[0].done, "failed"); // the test engine has no Recycle Bin
        assert_eq!(real.freed_now_bytes, 0);
    }

    /// « Immédiat » and the cleaning of caches on a real temporary folder.
    #[test]
    fn immediate_deletion_and_cache_cleaning_free_the_space_at_once() {
        let parent =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\manager-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = crate::fs::strip_verbatim(&dir.path().canonicalize().unwrap());
        let file = base.join("big.bin");
        std::fs::write(&file, vec![1u8; 4096]).unwrap();
        let cache = base.join("Cache");
        std::fs::create_dir_all(cache.join("sub")).unwrap();
        std::fs::write(cache.join("a.tmp"), b"x").unwrap();
        std::fs::write(cache.join(r"sub\b.tmp"), b"y").unwrap();
        let zones = Zones::new(&KnownFolders::under(&base));

        let request = DeleteRequest {
            paths: vec![file.display().to_string()],
            mode: DeleteMode::Immediate,
            dry_run: false,
            ..DeleteRequest::default()
        };
        let result = delete(&RealFileSystem, &zones, &NoSystemShell, &request, false);
        assert_eq!(
            result.items[0].done, "deleted",
            "{:?}",
            result.items[0].error
        );
        assert!(result.freed_now_bytes >= 4096);
        assert!(!file.exists());

        let cleaned = clean_caches(
            &RealFileSystem,
            &zones,
            &[cache.display().to_string()],
            &ManualAction::default(),
            false,
        );
        assert_eq!(cleaned.items[0].done, "deleted");
        assert!(cache.exists(), "the cache folder itself stays");
        assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    }

    #[test]
    fn caches_are_found_with_their_browser_and_zone() {
        let folders = KnownFolders::under(r"C:\");
        let mut fs = FakeFileSystem::new();
        fs.add_file(folders.local.join(r"Temp\x.tmp"), 1_000, None)
            .add_file(folders.windir.join(r"Temp\y.tmp"), 2_000, None)
            .add_file(
                folders
                    .local
                    .join(r"Google\Chrome\User Data\Default\Cache\f_0001"),
                5_000,
                None,
            )
            .add_file(
                folders
                    .local
                    .join(r"Microsoft\Windows\Explorer\thumbcache_256.db"),
                7_000,
                None,
            )
            .add_file(
                folders
                    .local
                    .join(r"Microsoft\Windows\Explorer\iconcache.db"),
                9_000,
                None,
            );
        let found = caches(&fs, &folders);
        let user = found
            .iter()
            .find(|c| c.name.starts_with("Fichiers temporaires de l'utilisateur"))
            .unwrap();
        assert_eq!((user.size_bytes, user.zone), (1_000, Zone::Normal));
        let windows = found.iter().find(|c| c.name.contains("Windows")).unwrap();
        assert_eq!(windows.zone, Zone::Sensitive); // never in « Tout libérer »
        let chrome = found
            .iter()
            .find(|c| c.browser.as_deref() == Some("Chrome"))
            .unwrap();
        assert_eq!(chrome.size_bytes, 5_000);
        let thumbnails = found
            .iter()
            .find(|c| c.name.starts_with("Miniatures"))
            .unwrap();
        assert_eq!(thumbnails.size_bytes, 7_000);
    }

    #[test]
    fn a_deletion_at_restart_can_be_cancelled_alone() {
        let list: Vec<String> = [
            r"\??\C:\a.tmp",
            "",
            r"\??\C:\old.txt",
            r"\??\C:\new.txt",
            r"\??\C:\b.tmp",
            "",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let kept = without_pending(&list, r"C:\A.TMP").unwrap();
        assert_eq!(
            kept,
            list[2..].to_vec(),
            "the rename and the other deletion stay"
        );
        assert!(
            without_pending(&list, r"C:\old.txt").is_none(),
            "a rename is not a deletion"
        );
        assert!(without_pending(&list, r"C:\none").is_none());
    }

    /// EF-24: the quarantine frees the space now only on another drive.
    #[test]
    fn the_quarantine_frees_space_only_on_another_drive() {
        assert!(other_drive(
            r"D:\CleanSlate",
            &[r"C:\Users\Ana\Videos\film.mp4".into()]
        ));
        assert!(!other_drive(
            r"C:\Users\Ana\AppData\Local\CleanSlate",
            &[r"C:\x".into()]
        ));
        assert!(!other_drive(r"D:\Q", &[r"C:\x".into(), r"d:\y".into()]));
        assert!(!other_drive(r"D:\Q", &[]));
    }

    #[test]
    fn disks_and_their_unallocated_space_are_read_from_get_disk() {
        let one = r#"{"Number":0,"FriendlyName":"SSD","Size":1000,"AllocatedSize":900}"#;
        assert_eq!(parse_disks(one)[0].unallocated_bytes, 100);
        let two = r#"[{"Number":0,"FriendlyName":"A","Size":10,"AllocatedSize":10},{"Number":1,"FriendlyName":null,"Size":50,"AllocatedSize":20}]"#;
        let disks = parse_disks(two);
        assert_eq!(
            (
                disks.len(),
                disks[1].unallocated_bytes,
                disks[1].name.as_str()
            ),
            (2, 30, "")
        );
        assert!(parse_disks("").is_empty());
    }

    #[test]
    fn requests_are_simulations_by_default() {
        let delete: DeleteRequest = serde_json::from_str(r#"{"paths":["C:\\x"]}"#).unwrap();
        assert!(delete.dry_run);
        assert_eq!(delete.mode, DeleteMode::Safe);
        let moving: MoveRequest =
            serde_json::from_str(r#"{"paths":[],"destination":"D:\\"}"#).unwrap();
        assert!(moving.dry_run);
    }
}
