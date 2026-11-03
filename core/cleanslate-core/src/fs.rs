//! File system access, behind a trait so tests never touch the real disk.
//!
//! [`RealFileSystem`] reads the real disk; [`FakeFileSystem`] is an
//! in-memory tree for unit tests. Both are read-only at this stage: writes
//! will be added with the quarantine (J4), always behind `safety::validate`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::error::{CoreError, CoreResult};

/// `FILE_ATTRIBUTE_REPARSE_POINT`: symbolic link, junction, mount point…
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// Data stored compressed or sparse: its size on disk differs from its length.
const FILE_ATTRIBUTE_SPARSE_OR_COMPRESSED: u32 = 0x200 | 0x800;
/// Cloud placeholder whose data is not on the disk (`OFFLINE`, `RECALL_ON_OPEN`,
/// `RECALL_ON_DATA_ACCESS`).
const FILE_ATTRIBUTE_NOT_ON_DISK: u32 = 0x1000 | 0x4_0000 | 0x40_0000;

/// Metadata of a file or folder, read without following links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMetadata {
    /// `true` for a folder.
    pub is_dir: bool,
    /// `true` for a symbolic link or junction: it must never be traversed.
    pub is_reparse_point: bool,
    /// Size in bytes (0 for a folder).
    pub len: u64,
    /// Bytes the data really takes on the disk, before rounding to clusters (D73): 0 for a
    /// cloud file not downloaded (OneDrive), the compressed size of a compressed or sparse
    /// file, else `len`.
    pub on_disk: u64,
    /// Last modification time, when available.
    pub modified: Option<DateTime<Utc>>,
}

/// One child of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Full path of the child.
    pub path: PathBuf,
    /// File name of the child.
    pub name: OsString,
    /// Metadata of the child (links are not followed).
    pub metadata: EntryMetadata,
}

/// Read access to a file system.
///
/// `Send + Sync` lets the engine share it between scan threads.
pub trait FileSystem: Send + Sync {
    /// Reads the metadata of `path` without following links (safety rule 6).
    fn metadata(&self, path: &Path) -> CoreResult<EntryMetadata>;

    /// Lists the direct children of the folder `path`.
    fn read_dir(&self, path: &Path) -> CoreResult<Vec<DirEntry>>;

    /// Returns `true` if `path` exists (a dangling link counts as existing).
    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }

    /// Final path of `path` once every link is resolved (used by `safety` to
    /// detect a path that leaves its root). The default keeps the path as is:
    /// an in-memory tree has no links to resolve.
    fn canonicalize(&self, path: &Path) -> CoreResult<PathBuf> {
        self.metadata(path)?;
        Ok(path.to_path_buf())
    }

    /// Reads a small file (shortcuts), refusing files larger than `max_len`.
    fn read_file(&self, path: &Path, max_len: u64) -> CoreResult<Vec<u8>>;
}

/// Removes the `\\?\` (or `\\?\UNC\`) prefix added by `std::fs::canonicalize`.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// The real disk, read-only.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealFileSystem;

impl RealFileSystem {
    fn convert(path: &Path, meta: &fs::Metadata) -> EntryMetadata {
        use std::os::windows::fs::MetadataExt;
        let attributes = meta.file_attributes();
        let is_reparse_point = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        let len = if meta.is_dir() { 0 } else { meta.len() };
        let on_disk = if meta.is_dir() || attributes & FILE_ATTRIBUTE_NOT_ON_DISK != 0 {
            0
        } else if attributes & FILE_ATTRIBUTE_SPARSE_OR_COMPRESSED != 0 {
            crate::native::compressed_size(path).unwrap_or(len)
        } else {
            len
        };
        EntryMetadata {
            // A reparse point is reported as such, never as a folder to walk.
            is_dir: meta.is_dir() && !is_reparse_point,
            is_reparse_point,
            len,
            on_disk,
            modified: meta.modified().ok().map(DateTime::<Utc>::from),
        }
    }
}

impl FileSystem for RealFileSystem {
    fn metadata(&self, path: &Path) -> CoreResult<EntryMetadata> {
        // symlink_metadata never follows links or junctions.
        let meta = fs::symlink_metadata(path).map_err(|e| CoreError::from_io(path, e))?;
        Ok(Self::convert(path, &meta))
    }

    fn read_dir(&self, path: &Path) -> CoreResult<Vec<DirEntry>> {
        let meta = self.metadata(path)?;
        if meta.is_reparse_point {
            return Err(CoreError::Protected {
                location: path.display().to_string(),
            });
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(path).map_err(|e| CoreError::from_io(path, e))? {
            let entry = entry.map_err(|e| CoreError::from_io(path, e))?;
            let child = entry.path();
            // On Windows, DirEntry::metadata comes from the directory listing itself: no
            // file is opened and links are not followed (D73: a whole disk stays fast).
            let meta = entry
                .metadata()
                .map_err(|e| CoreError::from_io(&child, e))?;
            entries.push(DirEntry {
                name: entry.file_name(),
                metadata: Self::convert(&child, &meta),
                path: child,
            });
        }
        Ok(entries)
    }

    fn canonicalize(&self, path: &Path) -> CoreResult<PathBuf> {
        // On Windows, std::fs::canonicalize calls GetFinalPathNameByHandleW.
        let canonical = fs::canonicalize(path).map_err(|e| CoreError::from_io(path, e))?;
        Ok(strip_verbatim(&canonical))
    }

    fn read_file(&self, path: &Path, max_len: u64) -> CoreResult<Vec<u8>> {
        let meta = self.metadata(path)?;
        if meta.is_dir || meta.is_reparse_point || meta.len > max_len {
            return Err(CoreError::InvalidInput(format!(
                "not a small regular file: {}",
                path.display()
            )));
        }
        fs::read(path).map_err(|e| CoreError::from_io(path, e))
    }
}

/// In-memory file system for tests.
///
/// Paths are compared exactly (case-sensitive), unlike Windows: tests must
/// use consistent casing.
#[derive(Debug, Default, Clone)]
pub struct FakeFileSystem {
    nodes: BTreeMap<PathBuf, EntryMetadata>,
    /// Children of each folder, so that `read_dir` stays fast on big trees.
    children: BTreeMap<PathBuf, BTreeSet<PathBuf>>,
    /// Content of the files added with [`FakeFileSystem::add_file_with_content`].
    contents: BTreeMap<PathBuf, Vec<u8>>,
}

impl FakeFileSystem {
    /// Creates an empty file system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a folder whose modification time is `modified`.
    pub fn add_dir_modified(
        &mut self,
        path: impl AsRef<Path>,
        modified: DateTime<Utc>,
    ) -> &mut Self {
        self.insert(path.as_ref(), Self::dir_meta(false, Some(modified)));
        self
    }
    /// Adds a folder and all its missing parents.
    pub fn add_dir(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.insert(path.as_ref(), Self::dir_meta(false, None));
        self
    }

    /// Adds a file of `len` bytes, creating its parent folders.
    pub fn add_file(
        &mut self,
        path: impl AsRef<Path>,
        len: u64,
        modified: Option<DateTime<Utc>>,
    ) -> &mut Self {
        self.add_file_on_disk(path, len, len, modified)
    }

    /// Adds a file whose data takes `on_disk` bytes on the disk (compressed file, or 0 for a
    /// cloud file not downloaded), creating its parent folders.
    pub fn add_file_on_disk(
        &mut self,
        path: impl AsRef<Path>,
        len: u64,
        on_disk: u64,
        modified: Option<DateTime<Utc>>,
    ) -> &mut Self {
        let meta = EntryMetadata {
            is_dir: false,
            is_reparse_point: false,
            len,
            on_disk,
            modified,
        };
        self.insert(path.as_ref(), meta);
        self
    }

    /// Adds a file with the given content, creating its parent folders.
    pub fn add_file_with_content(
        &mut self,
        path: impl AsRef<Path>,
        content: Vec<u8>,
        modified: Option<DateTime<Utc>>,
    ) -> &mut Self {
        self.add_file(path.as_ref(), content.len() as u64, modified);
        self.contents.insert(path.as_ref().to_path_buf(), content);
        self
    }

    /// Adds a junction or symbolic link, creating its parent folders.
    pub fn add_reparse_point(&mut self, path: impl AsRef<Path>) -> &mut Self {
        self.insert(path.as_ref(), Self::dir_meta(true, None));
        self
    }

    fn dir_meta(is_reparse_point: bool, modified: Option<DateTime<Utc>>) -> EntryMetadata {
        EntryMetadata {
            is_dir: !is_reparse_point,
            is_reparse_point,
            len: 0,
            on_disk: 0,
            modified,
        }
    }

    fn insert(&mut self, path: &Path, meta: EntryMetadata) {
        let mut child = path;
        for parent in path.ancestors().skip(1) {
            if parent.as_os_str().is_empty() {
                break;
            }
            self.children
                .entry(parent.to_path_buf())
                .or_default()
                .insert(child.to_path_buf());
            self.nodes
                .entry(parent.to_path_buf())
                .or_insert_with(|| Self::dir_meta(false, None));
            child = parent;
        }
        self.nodes.insert(path.to_path_buf(), meta);
    }
}

impl FileSystem for FakeFileSystem {
    fn read_file(&self, path: &Path, max_len: u64) -> CoreResult<Vec<u8>> {
        let meta = self.metadata(path)?;
        if meta.is_dir || meta.is_reparse_point || meta.len > max_len {
            return Err(CoreError::InvalidInput(format!(
                "not a small regular file: {}",
                path.display()
            )));
        }
        Ok(self.contents.get(path).cloned().unwrap_or_default())
    }

    fn metadata(&self, path: &Path) -> CoreResult<EntryMetadata> {
        self.nodes
            .get(path)
            .cloned()
            .ok_or_else(|| CoreError::NotFound {
                location: path.display().to_string(),
            })
    }

    fn read_dir(&self, path: &Path) -> CoreResult<Vec<DirEntry>> {
        let meta = self.metadata(path)?;
        if meta.is_reparse_point {
            return Err(CoreError::Protected {
                location: path.display().to_string(),
            });
        }
        if !meta.is_dir {
            return Err(CoreError::InvalidInput(format!(
                "not a folder: {}",
                path.display()
            )));
        }
        Ok(self
            .children
            .get(path)
            .into_iter()
            .flatten()
            .filter_map(|child| {
                self.nodes.get(child).map(|meta| DirEntry {
                    path: child.clone(),
                    name: child.file_name().unwrap_or_default().to_os_string(),
                    metadata: meta.clone(),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[DirEntry]) -> Vec<String> {
        let mut names: Vec<String> = entries
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn fake_creates_parents_and_lists_children() {
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Root\App\a.txt", 10, None)
            .add_dir(r"C:\Root\Other")
            .add_reparse_point(r"C:\Root\Link");

        let root = Path::new(r"C:\Root");
        assert!(fs.metadata(root).unwrap().is_dir);
        assert_eq!(names(&fs.read_dir(root).unwrap()), ["App", "Link", "Other"]);
        assert_eq!(
            fs.metadata(Path::new(r"C:\Root\App\a.txt")).unwrap().len,
            10
        );
    }

    #[test]
    fn fake_refuses_to_walk_reparse_points() {
        let mut fs = FakeFileSystem::new();
        fs.add_reparse_point(r"C:\Root\Link");
        let link = Path::new(r"C:\Root\Link");
        let meta = fs.metadata(link).unwrap();
        assert!(meta.is_reparse_point);
        assert!(!meta.is_dir);
        assert_eq!(fs.read_dir(link).unwrap_err().code(), "PROTECTED");
    }

    #[test]
    fn fake_reports_missing_paths() {
        let fs = FakeFileSystem::new();
        assert!(!fs.exists(Path::new(r"C:\Nope")));
        assert_eq!(
            fs.read_dir(Path::new(r"C:\Nope")).unwrap_err().code(),
            "NOT_FOUND"
        );
    }

    #[test]
    fn fake_read_dir_on_file_is_invalid() {
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\f.txt", 1, None);
        assert_eq!(
            fs.read_dir(Path::new(r"C:\f.txt")).unwrap_err().code(),
            "INVALID_INPUT"
        );
    }

    #[test]
    fn real_reads_a_temp_tree() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("file.bin"), [0u8; 42]).unwrap();

        let real = RealFileSystem;
        let entries = real.read_dir(dir.path()).unwrap();
        assert_eq!(names(&entries), ["file.bin", "sub"]);

        let file = real.metadata(&dir.path().join("file.bin")).unwrap();
        assert_eq!(file.len, 42);
        assert!(!file.is_dir && !file.is_reparse_point);
        assert!(file.modified.is_some());

        let sub = real.metadata(&dir.path().join("sub")).unwrap();
        assert!(sub.is_dir && !sub.is_reparse_point);
    }

    #[test]
    fn real_reports_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let err = RealFileSystem
            .metadata(&dir.path().join("missing"))
            .unwrap_err();
        assert_eq!(err.code(), "NOT_FOUND");
    }
}
