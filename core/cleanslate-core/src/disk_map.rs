//! Storage map of a drive (D73): what really takes the space, as a tree of folders and big
//! files up to three levels deep, each one with a category (installed applications, orphan
//! leftovers, quarantine, user files, Windows and system, others). Read-only: nothing is
//! moved, links and junctions are never followed, a folder that cannot be read is counted as
//! « not measured ». Sizes are the space taken on the disk: cloud files not downloaded count
//! 0, compressed files their compressed size, every file rounded up to the allocation unit.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::fs::FileSystem;
use crate::safety::KnownFolders;

/// What a part of the disk is (legend of the map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCategory {
    /// Program Files, `Local\Programs`, install folders of installed applications.
    Applications,
    /// Orphan leftovers of the last scan: space CleanSlate can give back.
    Leftovers,
    /// CleanSlate's quarantine: given back when it is purged.
    Quarantine,
    /// Documents, Pictures, Videos, Music, Downloads, Desktop, OneDrive…
    UserFiles,
    /// Windows, page and hibernation files, restore points, recycle bin, unreadable parts.
    System,
    /// Anything else (AppData, other folders).
    Other,
}

/// Request of `cs_disk_map`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskMapRequest {
    /// Root of the drive, e.g. `C:\`, or a folder to detail further (D81).
    pub root: PathBuf,
    /// Levels kept below the root (1 to 3, D73); a deeper folder is detailed by a new map
    /// rooted at it.
    #[serde(default = "default_depth")]
    pub max_depth: u8,
    /// Runs the orphan scan first when none was made yet (true by default, D73): a level
    /// computed on a click reuses the last one instead, else it waited minutes (D83).
    #[serde(default = "default_scan")]
    pub scan_leftovers: bool,
}

fn default_scan() -> bool {
    true
}

fn default_depth() -> u8 {
    3
}

/// One rectangle of the map: a folder, a big file, or a group of small items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskMapNode {
    /// Name shown on the map.
    pub name: String,
    /// Full path (empty for a group).
    pub path: String,
    /// Space taken on the disk, in bytes.
    pub size_bytes: u64,
    /// Number of files below.
    pub files: u64,
    /// Category, for the color and the legend.
    pub category: StorageCategory,
    /// Children kept on the map, largest first (empty below the last level).
    pub children: Vec<DiskMapNode>,
    /// « Autres petits éléments »: small items gathered in one rectangle.
    pub is_group: bool,
    /// A folder (`false`: a file or a group).
    pub is_folder: bool,
    /// A folder with sub-folders (D81): `false` for a terminal folder, holding only files,
    /// which the map opens in the Gestionnaire. Missing in a map saved before D81.
    #[serde(default)]
    pub has_folders: bool,
}

/// Result of `cs_disk_map`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiskMap {
    /// The drive, with its children.
    pub root: DiskMapNode,
    /// Size of the volume.
    pub total_bytes: u64,
    /// Free space of the volume.
    pub free_bytes: u64,
    /// Folders that could not be read (access refused).
    pub denied: u64,
    /// Duration of the computation.
    pub duration_ms: u64,
    /// Stopped by `cs_cancel`: the sizes are partial.
    pub cancelled: bool,
}

/// Progress of the computation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskMapProgress {
    /// Files counted so far.
    pub files: u64,
    /// Bytes counted so far.
    pub bytes: u64,
    /// Folder being read.
    pub current: String,
}

/// Name of the group of small items, and of the space that could not be read.
pub const SMALL_ITEMS: &str = "Autres petits éléments";
/// Part of the used space that no folder accounts for (access refused, system files).
pub const NOT_MEASURED: &str = "Espace système non mesuré";

/// At most this many rectangles per level; the rest is grouped (D73).
const MAX_CHILDREN: usize = 40;
/// Children smaller than this share of their parent are grouped.
const MIN_SHARE: f64 = 0.005;
/// Progress is reported every this many files.
const PROGRESS_EVERY: u64 = 2_000;

/// Decides the category of a path (prefix rules, most specific first).
#[derive(Debug, Default, Clone)]
pub struct Categorizer {
    quarantine: Vec<String>,
    leftovers: Vec<String>,
    system: Vec<String>,
    system_files: Vec<String>,
    user: Vec<String>,
    applications: Vec<String>,
}

fn key(path: &Path) -> String {
    path.to_string_lossy()
        .to_lowercase()
        .trim_end_matches('\\')
        .to_owned()
}

fn within(path: &str, root: &str) -> bool {
    !root.is_empty() && (path == root || path.starts_with(&format!(r"{root}\")))
}

impl Categorizer {
    /// Rules of this machine: known folders, install folders of the installed applications,
    /// locations of the last orphan scan, and CleanSlate's data folder.
    pub fn new(
        folders: &KnownFolders,
        install_locations: &[PathBuf],
        leftovers: &[String],
        data_dir: Option<&Path>,
    ) -> Self {
        let drive = folders
            .windir
            .ancestors()
            .last()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let mut user: Vec<String> = folders.user_folders.iter().map(|p| key(p)).collect();
        user.extend(
            ["OneDrive", "Videos", "Music", "Pictures"]
                .iter()
                .map(|f| key(&folders.user_profile.join(f))),
        );
        Categorizer {
            quarantine: data_dir.map(|d| vec![key(d)]).unwrap_or_default(),
            leftovers: leftovers.iter().map(|l| key(Path::new(l))).collect(),
            system: [
                folders.windir.clone(),
                drive.join("System Volume Information"),
                drive.join("$Recycle.Bin"),
                drive.join("Recovery"),
                folders.program_data.join("Microsoft"),
            ]
            .iter()
            .map(|p| key(p))
            .collect(),
            system_files: [
                "pagefile.sys",
                "hiberfil.sys",
                "swapfile.sys",
                "dumpstack.log.tmp",
            ]
            .iter()
            .map(|f| key(&drive.join(f)))
            .collect(),
            user,
            applications: [
                &folders.program_files,
                &folders.program_files_x86,
                &folders.local_programs,
            ]
            .iter()
            .map(|p| key(p))
            .chain(install_locations.iter().map(|p| key(p)))
            .filter(|p| p.matches('\\').count() >= 1)
            .collect(),
        }
    }

    /// `true` when a leftover or the quarantine lies below `path`: the map keeps the levels
    /// leading to them, whatever the depth, so that what can be recovered stays visible.
    pub fn leads_to_recoverable(&self, path: &Path) -> bool {
        let p = format!(r"{}\", key(path));
        self.leftovers
            .iter()
            .chain(&self.quarantine)
            .any(|r| r.starts_with(&p))
    }

    /// Category of `path`, or `None` when no rule says (the folder then takes the main
    /// category of its content).
    pub fn of(&self, path: &Path) -> Option<StorageCategory> {
        let p = key(path);
        let any = |roots: &[String]| roots.iter().any(|r| within(&p, r));
        if any(&self.quarantine) {
            Some(StorageCategory::Quarantine)
        } else if any(&self.leftovers) {
            Some(StorageCategory::Leftovers)
        } else if any(&self.system) || self.system_files.contains(&p) {
            Some(StorageCategory::System)
        } else if any(&self.user) {
            Some(StorageCategory::UserFiles)
        } else if any(&self.applications) {
            Some(StorageCategory::Applications)
        } else {
            None
        }
    }
}

/// Everything the computation reads.
pub struct DiskMapEnv<'a> {
    /// File system (read only).
    pub fs: &'a dyn FileSystem,
    /// Categories.
    pub categorizer: &'a Categorizer,
    /// Allocation unit of the volume, in bytes (sizes are rounded up to it).
    pub cluster: u64,
    /// Set by `cs_cancel`.
    pub cancel: &'a AtomicBool,
}

struct Walk<'a> {
    env: &'a DiskMapEnv<'a>,
    files: AtomicU64,
    bytes: AtomicU64,
    denied: AtomicU64,
    progress: &'a (dyn Fn(&DiskMapProgress) + Sync),
}

/// Space a file takes: its bytes on disk rounded up to the cluster; 0 stays 0.
pub fn allocated(on_disk: u64, cluster: u64) -> u64 {
    if on_disk == 0 || cluster <= 1 {
        return on_disk;
    }
    on_disk.div_ceil(cluster) * cluster
}

/// Computes the map of `request.root`. `total` and `free` describe the volume; when the
/// measured folders do not account for all the used space (access refused, files Windows
/// hides), the difference is shown as [`NOT_MEASURED`], in the System category.
pub fn build(
    env: &DiskMapEnv,
    request: &DiskMapRequest,
    total: u64,
    free: u64,
    progress: &(dyn Fn(&DiskMapProgress) + Sync),
) -> DiskMap {
    let started = std::time::Instant::now();
    let walk = Walk {
        env,
        files: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
        denied: AtomicU64::new(0),
        progress,
    };
    let depth = request.max_depth.clamp(1, 3);
    let mut root = walk.folder(&request.root, 0, depth, None);
    root.name = request
        .root
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_owned();
    let used = total.saturating_sub(free);
    let cancelled = env.cancel.load(Ordering::Relaxed);
    // Only the whole drive accounts for the used space; a folder detailed later does not.
    let whole_drive = request.root.parent().is_none();
    if whole_drive && !cancelled && used > root.size_bytes {
        let missing = used - root.size_bytes;
        root.children.push(DiskMapNode {
            name: NOT_MEASURED.to_owned(),
            path: String::new(),
            size_bytes: missing,
            files: 0,
            category: StorageCategory::System,
            children: Vec::new(),
            is_group: true,
            is_folder: false,
            has_folders: false,
        });
        root.size_bytes = used;
        root.children
            .sort_by_key(|c| std::cmp::Reverse(c.size_bytes));
    }
    DiskMap {
        root,
        total_bytes: total,
        free_bytes: free,
        denied: walk.denied.load(Ordering::Relaxed),
        duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        cancelled,
    }
}

impl Walk<'_> {
    fn folder(
        &self,
        path: &Path,
        level: u8,
        depth: u8,
        inherited: Option<StorageCategory>,
    ) -> DiskMapNode {
        let own = self.env.categorizer.of(path).or(inherited);
        let mut node = DiskMapNode {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            size_bytes: 0,
            files: 0,
            category: own.unwrap_or(StorageCategory::Other),
            children: Vec::new(),
            is_group: false,
            is_folder: true,
            has_folders: false,
        };
        if self.env.cancel.load(Ordering::Relaxed) {
            return node;
        }
        let entries = match self.env.fs.read_dir(path) {
            Ok(entries) => entries,
            Err(_) => {
                // Access refused or vanished: counted, never retried (the used space it
                // takes appears as « not measured » at the root).
                self.denied.fetch_add(1, Ordering::Relaxed);
                return node;
            }
        };
        let keep = level < depth || self.env.categorizer.leads_to_recoverable(path);
        let (dirs, files): (Vec<_>, Vec<_>) = entries
            .into_iter()
            .filter(|e| !e.metadata.is_reparse_point || !e.metadata.is_dir)
            .partition(|e| e.metadata.is_dir);
        node.has_folders = !dirs.is_empty();
        let mut children: Vec<DiskMapNode> = dirs
            .par_iter()
            .map(|d| self.folder(&d.path, level + 1, depth, own))
            .collect();
        for file in &files {
            let size = allocated(file.metadata.on_disk, self.env.cluster);
            let count = self.files.fetch_add(1, Ordering::Relaxed) + 1;
            self.bytes.fetch_add(size, Ordering::Relaxed);
            if count.is_multiple_of(PROGRESS_EVERY) {
                (self.progress)(&DiskMapProgress {
                    files: count,
                    bytes: self.bytes.load(Ordering::Relaxed),
                    current: path.to_string_lossy().into_owned(),
                });
            }
            node.size_bytes += size;
            node.files += 1;
            if keep {
                children.push(DiskMapNode {
                    name: file.name.to_string_lossy().into_owned(),
                    path: file.path.to_string_lossy().into_owned(),
                    size_bytes: size,
                    files: 1,
                    category: self
                        .env
                        .categorizer
                        .of(&file.path)
                        .or(own)
                        .unwrap_or(StorageCategory::Other),
                    children: Vec::new(),
                    is_group: false,
                    is_folder: false,
                    has_folders: false,
                });
            }
        }
        for child in children.iter().filter(|c| c.is_folder) {
            node.size_bytes += child.size_bytes;
            node.files += child.files;
        }
        if own.is_none() {
            node.category = dominant(&children);
        }
        node.children = if keep {
            group_small(children, node.size_bytes)
        } else {
            Vec::new()
        };
        node
    }
}

/// Category taking the most space among `children` (Other when empty).
fn dominant(children: &[DiskMapNode]) -> StorageCategory {
    let mut sums: Vec<(StorageCategory, u64)> = Vec::new();
    for child in children {
        match sums.iter_mut().find(|(c, _)| *c == child.category) {
            Some((_, s)) => *s += child.size_bytes,
            None => sums.push((child.category, child.size_bytes)),
        }
    }
    sums.into_iter()
        .max_by_key(|(_, s)| *s)
        .map_or(StorageCategory::Other, |(c, _)| c)
}

/// Largest first; keeps at most [`MAX_CHILDREN`] items of at least [`MIN_SHARE`] of
/// `parent_size`, gathers the others in one [`SMALL_ITEMS`] rectangle. Empty items go.
pub fn group_small(mut children: Vec<DiskMapNode>, parent_size: u64) -> Vec<DiskMapNode> {
    children.retain(|c| c.size_bytes > 0);
    children.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.name.cmp(&b.name))
    });
    let threshold = (parent_size as f64 * MIN_SHARE) as u64;
    let split = children
        .iter()
        .take(MAX_CHILDREN)
        .take_while(|c| c.size_bytes >= threshold.max(1))
        .count();
    let small: Vec<DiskMapNode> = children.split_off(split);
    if small.len() == 1 {
        children.extend(small);
    } else if !small.is_empty() {
        children.push(DiskMapNode {
            name: SMALL_ITEMS.to_owned(),
            path: String::new(),
            size_bytes: small.iter().map(|c| c.size_bytes).sum(),
            files: small.iter().map(|c| c.files).sum(),
            category: dominant(&small),
            children: Vec::new(),
            is_group: true,
            is_folder: false,
            has_folders: false,
        });
    }
    children
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;

    const MB: u64 = 1024 * 1024;

    fn machine() -> (FakeFileSystem, KnownFolders) {
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Windows\System32\kernel32.dll", 800 * MB, None)
            .add_file(r"C:\pagefile.sys", 4_000 * MB, None)
            .add_file(r"C:\Program Files\VideoLAN\VLC\vlc.exe", 300 * MB, None)
            .add_file(r"C:\Users\Ana\Documents\thesis.docx", 200 * MB, None)
            .add_file(r"C:\Users\Ana\Videos\holiday.mp4", 1_500 * MB, None)
            .add_file(
                r"C:\Users\Ana\AppData\Roaming\OldTool\cache.bin",
                100 * MB,
                None,
            )
            .add_file(
                r"C:\Users\Ana\AppData\Local\CleanSlate\quarantine\s1\files\a.bin",
                50 * MB,
                None,
            )
            // A cloud file not downloaded takes no space; a compressed one, its compressed size.
            .add_file_on_disk(r"C:\Users\Ana\OneDrive\big.zip", 900 * MB, 0, None)
            .add_file_on_disk(r"C:\Users\Ana\Documents\logs.txt", 60 * MB, 6 * MB, None)
            .add_reparse_point(r"C:\Users\Ana\AppData\Local\Trap");
        (fs, KnownFolders::under(r"C:\"))
    }

    fn compute(
        fs: &FakeFileSystem,
        folders: &KnownFolders,
        cancel: &AtomicBool,
        total: u64,
        free: u64,
    ) -> DiskMap {
        let categorizer = Categorizer::new(
            folders,
            &[],
            &[r"C:\Users\Ana\AppData\Roaming\OldTool".to_owned()],
            Some(Path::new(r"C:\Users\Ana\AppData\Local\CleanSlate")),
        );
        let env = DiskMapEnv {
            fs,
            categorizer: &categorizer,
            cluster: 4096,
            cancel,
        };
        let request = DiskMapRequest {
            root: PathBuf::from(r"C:\"),
            max_depth: 3,
            scan_leftovers: true,
        };
        build(&env, &request, total, free, &|_| {})
    }

    fn find<'a>(node: &'a DiskMapNode, path: &str) -> Option<&'a DiskMapNode> {
        if node.path.eq_ignore_ascii_case(path) {
            return Some(node);
        }
        node.children.iter().find_map(|c| find(c, path))
    }

    #[test]
    fn sizes_and_categories_of_a_machine() {
        let (fs, folders) = machine();
        let map = compute(&fs, &folders, &AtomicBool::new(false), 0, 0);
        let cat = |p: &str| find(&map.root, p).map(|n| n.category);
        assert_eq!(cat(r"C:\Windows"), Some(StorageCategory::System));
        assert_eq!(cat(r"C:\pagefile.sys"), Some(StorageCategory::System));
        assert_eq!(
            cat(r"C:\Program Files"),
            Some(StorageCategory::Applications)
        );
        assert_eq!(
            cat(r"C:\Users\Ana\Videos"),
            Some(StorageCategory::UserFiles)
        );
        assert_eq!(
            cat(r"C:\Users\Ana\Documents"),
            Some(StorageCategory::UserFiles)
        );
        assert_eq!(
            cat(r"C:\Users\Ana\AppData\Roaming\OldTool"),
            Some(StorageCategory::Leftovers)
        );
        assert_eq!(
            cat(r"C:\Users\Ana\AppData\Local\CleanSlate"),
            Some(StorageCategory::Quarantine)
        );
        // Real space: the cloud file counts 0, the compressed one its 6 Mo.
        assert_eq!(
            find(&map.root, r"C:\Users\Ana\OneDrive").map(|n| n.size_bytes),
            None
        );
        assert_eq!(
            find(&map.root, r"C:\Users\Ana\Documents").map(|n| n.size_bytes),
            Some(206 * MB)
        );
        assert_eq!(
            map.root.size_bytes,
            (800 + 4_000 + 300 + 200 + 1_500 + 100 + 50 + 6) * MB
        );
        // A mixed folder takes the main category of its content (Videos > the rest).
        assert_eq!(cat(r"C:\Users"), Some(StorageCategory::UserFiles));
        // Children are sorted, largest first.
        let sizes: Vec<u64> = map.root.children.iter().map(|c| c.size_bytes).collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn links_are_never_followed_and_unreadable_space_is_shown() {
        let (fs, folders) = machine();
        let map = compute(
            &fs,
            &folders,
            &AtomicBool::new(false),
            10_000 * MB,
            2_000 * MB,
        );
        assert!(find(&map.root, r"C:\Users\Ana\AppData\Local\Trap").is_none());
        let missing = map
            .root
            .children
            .iter()
            .find(|c| c.name == NOT_MEASURED)
            .unwrap();
        assert_eq!(missing.category, StorageCategory::System);
        assert_eq!(map.root.size_bytes, 8_000 * MB);
        assert_eq!(missing.size_bytes, 8_000 * MB - (6_956 * MB));
    }

    #[test]
    fn files_are_rounded_up_to_the_cluster() {
        assert_eq!(allocated(1, 4096), 4096);
        assert_eq!(allocated(4096, 4096), 4096);
        assert_eq!(allocated(4097, 4096), 8192);
        assert_eq!(allocated(0, 4096), 0);
        assert_eq!(allocated(10, 1), 10);
    }

    /// D81: any folder can be detailed further; a terminal folder says so.
    #[test]
    fn a_folder_is_detailed_by_its_own_map_and_terminal_folders_are_marked() {
        let (fs, folders) = machine();
        let cancel = AtomicBool::new(false);
        let categorizer = Categorizer::new(&folders, &[], &[], None);
        let env = DiskMapEnv {
            fs: &fs,
            categorizer: &categorizer,
            cluster: 4096,
            cancel: &cancel,
        };
        let request = DiskMapRequest {
            root: PathBuf::from(r"C:\Users\Ana"),
            max_depth: 3,
            scan_leftovers: true,
        };
        let map = build(&env, &request, 100_000 * MB, 10_000 * MB, &|_| {});
        assert!(
            map.root.children.iter().all(|c| c.name != NOT_MEASURED),
            "a folder never claims the unmeasured space of the drive"
        );
        assert!(map.root.has_folders);
        let videos = find(&map.root, r"C:\Users\Ana\Videos").unwrap();
        assert!(
            videos.is_folder && !videos.has_folders,
            "only files: terminal"
        );
        let appdata = find(&map.root, r"C:\Users\Ana\AppData").unwrap();
        assert!(appdata.has_folders);
        // Third level below Ana: OldTool is on the map, terminal, its files left for a click.
        let old = find(&map.root, r"C:\Users\Ana\AppData\Roaming\OldTool").unwrap();
        assert!(old.children.is_empty() && !old.has_folders && old.size_bytes >= 100 * MB);
    }

    #[test]
    fn small_items_are_grouped() {
        let leaf = |name: &str, size: u64| DiskMapNode {
            name: name.to_owned(),
            path: format!(r"C:\{name}"),
            size_bytes: size,
            files: 1,
            category: StorageCategory::Other,
            children: Vec::new(),
            is_group: false,
            is_folder: false,
            has_folders: false,
        };
        let mut children = vec![leaf("big", 10_000), leaf("empty", 0)];
        children.extend((0..50).map(|i| leaf(&format!("tiny{i}"), 10)));
        let kept = group_small(children, 10_500);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "big");
        assert_eq!(kept[1].name, SMALL_ITEMS);
        assert!(kept[1].is_group);
        assert_eq!(kept[1].size_bytes, 500);
        assert_eq!(kept[1].files, 50);
        // A single small item stays itself.
        let one = group_small(vec![leaf("big", 10_000), leaf("tiny", 10)], 10_010);
        assert_eq!(one[1].name, "tiny");
    }

    #[test]
    fn three_levels_at_most_except_on_the_way_to_what_can_be_recovered() {
        let (mut fs, folders) = machine();
        fs.add_file(r"C:\Data\A\B\C\deep.bin", 70 * MB, None);
        let map = compute(&fs, &folders, &AtomicBool::new(false), 0, 0);
        // Level 3 (C:\Data\A\B) keeps its size but no children.
        let b = find(&map.root, r"C:\Data\A\B").unwrap();
        assert!(b.children.is_empty());
        assert_eq!(b.size_bytes, 70 * MB);
        // The leftovers and the quarantine, deeper, stay visible on the map (D73).
        assert!(find(&map.root, r"C:\Users\Ana\AppData\Roaming\OldTool").is_some());
        assert!(find(&map.root, r"C:\Users\Ana\AppData\Local\CleanSlate").is_some());
    }

    #[test]
    fn cancelling_stops_at_once() {
        let (fs, folders) = machine();
        let map = compute(&fs, &folders, &AtomicBool::new(true), 10_000 * MB, 0);
        assert!(map.cancelled);
        assert_eq!(map.root.size_bytes, 0);
        assert!(map.root.children.iter().all(|c| c.name != NOT_MEASURED));
    }

    #[test]
    fn progress_is_reported_while_counting() {
        let mut fs = FakeFileSystem::new();
        for i in 0..(PROGRESS_EVERY + 5) {
            fs.add_file(format!(r"C:\Data\f{i}.bin"), 10, None);
        }
        let folders = KnownFolders::under(r"C:\");
        let categorizer = Categorizer::new(&folders, &[], &[], None);
        let cancel = AtomicBool::new(false);
        let env = DiskMapEnv {
            fs: &fs,
            categorizer: &categorizer,
            cluster: 4096,
            cancel: &cancel,
        };
        let seen = AtomicU64::new(0);
        let map = build(
            &env,
            &DiskMapRequest {
                root: PathBuf::from(r"C:\"),
                max_depth: 2,
                scan_leftovers: true,
            },
            0,
            0,
            &|p| {
                seen.store(p.files, Ordering::Relaxed);
            },
        );
        assert_eq!(seen.load(Ordering::Relaxed), PROGRESS_EVERY);
        assert_eq!(map.root.files, PROGRESS_EVERY + 5);
    }
}
