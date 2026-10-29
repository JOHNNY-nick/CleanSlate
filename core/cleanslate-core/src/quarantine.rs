//! Quarantine, restoration and purge (EF-16 to EF-19, specs § 7.2, § 7.3).
//!
//! Nothing is ever deleted directly: every item is moved (files) or exported
//! then removed (registry keys) into `quarantine\<session>\`, after
//! [`SafetyRules`] validation, with `manifest.json` written **before** each
//! move and updated after, so that an interrupted session can be resumed.

use std::collections::BTreeMap;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};
use crate::fs::FileSystem;
use crate::model::{
    ItemError, Leftover, LeftoverKind, Origin, QuarantineStatus, QuarantinedItem, Session,
    SessionStatus,
};
use crate::native::{self, Sha256};
use crate::registry::{utf16_le_with_bom, KeySnapshot, RegKeyPath, Registry};
use crate::safety::SafetyRules;

/// Above this size, the user must type [`CONFIRMATION_WORD`] (specs § 9.2).
pub const MAX_BYTES: u64 = 20 * 1024 * 1024 * 1024;
/// Above this number of files, the user must type [`CONFIRMATION_WORD`].
pub const MAX_FILES: u64 = 50_000;
/// Word typed to confirm a very large session (design § 5).
pub const CONFIRMATION_WORD: &str = "QUARANTAINE";

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
const MANIFEST: &str = "manifest.json";

/// SHA-256 of one file of a quarantined folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileHash {
    /// Path relative to the item, `\`-separated.
    pub relative_path: String,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256, lower-case hexadecimal.
    pub sha256: String,
}

/// Content of `quarantine\<session>\manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The session, items included.
    pub session: Session,
    /// Item being moved when the manifest was written (crash recovery).
    pub pending: Option<String>,
    /// Detailed hashes of every file, by item id.
    pub files: BTreeMap<String, Vec<FileHash>>,
}

/// A session that only lists what would be done (ENF-04): nothing moved,
/// nothing saved.
pub fn simulate(origin: Origin, items: &[Leftover], now: DateTime<Utc>) -> Session {
    let items: Vec<QuarantinedItem> = items
        .iter()
        .map(|item| QuarantinedItem {
            id: item.id.clone(),
            kind: item.kind,
            original_location: item.location.clone(),
            quarantine_path: PathBuf::new(),
            size_bytes: item.size_bytes,
            sha256: None,
            original_acl: None,
            quarantined_at: now,
            restored_at: None,
            status: QuarantineStatus::Quarantined,
            error: None,
        })
        .collect();
    Session {
        id: Uuid::nil(),
        created_at: now,
        origin,
        freed_bytes: items.iter().map(|i| i.size_bytes).sum(),
        items,
        errors: Vec::new(),
        status: SessionStatus::Completed,
        simulated: true,
    }
}

/// Files below `root` (or `root` itself), and whether a reparse point was met.
/// (relative path, absolute path, size) of one file.
type FileEntry = (String, PathBuf, u64);

fn collect_files(root: &Path) -> CoreResult<(Vec<FileEntry>, bool)> {
    let meta = fs::symlink_metadata(root).map_err(|e| CoreError::from_io(root, e))?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok((Vec::new(), true));
    }
    if !meta.is_dir() {
        return Ok((vec![(String::new(), root.to_path_buf(), meta.len())], false));
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| CoreError::from_io(&dir, e))? {
            let entry = entry.map_err(|e| CoreError::from_io(&dir, e))?;
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).map_err(|e| CoreError::from_io(&path, e))?;
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Ok((files, true));
            }
            if meta.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                files.push((relative, path, meta.len()));
            }
        }
    }
    files.sort();
    Ok((files, false))
}

/// Hashes of `files`; for a folder, the global hash is the SHA-256 of the
/// sorted `relative\tsha256\n` lines.
fn hash_files(files: &[FileEntry]) -> CoreResult<(Vec<FileHash>, String)> {
    let mut hashes = Vec::with_capacity(files.len());
    for (relative, path, size) in files {
        hashes.push(FileHash {
            relative_path: relative.clone(),
            size: *size,
            sha256: Sha256::of_file(path)?,
        });
    }
    let global = match hashes.as_slice() {
        [single] if single.relative_path.is_empty() => single.sha256.clone(),
        _ => {
            let lines: String = hashes
                .iter()
                .map(|h| format!("{}\t{}\n", h.relative_path, h.sha256))
                .collect();
            Sha256::of_bytes(lines.as_bytes())?
        }
    };
    Ok((hashes, global))
}

/// Copies a file or folder, keeping the modification times.
fn copy_tree(from: &Path, to: &Path) -> CoreResult<()> {
    let meta = fs::symlink_metadata(from).map_err(|e| CoreError::from_io(from, e))?;
    if !meta.is_dir() {
        fs::copy(from, to).map_err(|e| CoreError::from_io(from, e))?;
        return Ok(());
    }
    fs::create_dir_all(to).map_err(|e| CoreError::from_io(to, e))?;
    for entry in fs::read_dir(from).map_err(|e| CoreError::from_io(from, e))? {
        let entry = entry.map_err(|e| CoreError::from_io(from, e))?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

/// Removes a file or folder (std never follows links or junctions).
fn remove_tree(path: &Path) -> CoreResult<()> {
    let meta = fs::symlink_metadata(path).map_err(|e| CoreError::from_io(path, e))?;
    let result = if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| CoreError::from_io(path, e))
}

/// `true` if the hashes of `copy` equal the recorded ones.
fn verify(copy: &Path, expected: &[FileHash]) -> bool {
    match collect_files(copy).and_then(|(files, _)| hash_files(&files)) {
        Ok((hashes, _)) => hashes == expected,
        Err(_) => false,
    }
}

/// Moves `from` to `to`: same-volume move, else copy + verify + delete.
fn relocate(from: &Path, to: &Path, hashes: &[FileHash]) -> Result<(), (String, String)> {
    match native::move_path(from, to) {
        Ok(()) => Ok(()),
        Err(native::ERROR_NOT_SAME_DEVICE) => {
            let failed = |code: &str, msg: String| (code.to_owned(), msg);
            copy_tree(from, to).map_err(|e| failed("IO_ERROR", e.to_string()))?;
            if !verify(to, hashes) {
                let _ = remove_tree(to);
                return Err(failed(
                    "VERIFY_FAILED",
                    "la copie ne correspond pas à l'original".into(),
                ));
            }
            remove_tree(from).map_err(|e| failed("IO_ERROR", e.to_string()))
        }
        Err(native::ERROR_SHARING_VIOLATION | native::ERROR_LOCK_VIOLATION) => {
            Err(("LOCKED".into(), "utilisé par un programme".into()))
        }
        Err(code) => Err(("IO_ERROR".into(), format!("erreur Windows {code}"))),
    }
}

fn item_error(code: &str, message: impl Into<String>, location: &str) -> ItemError {
    ItemError {
        code: code.to_owned(),
        message: message.into(),
        path: Some(location.to_owned()),
    }
}

/// Session status from its items.
fn status_of(items: &[QuarantinedItem]) -> SessionStatus {
    if items.iter().all(|i| i.status == QuarantineStatus::Purged) && !items.is_empty() {
        SessionStatus::Purged
    } else if !items.is_empty() && items.iter().all(|i| i.status == QuarantineStatus::Restored) {
        SessionStatus::Restored
    } else if items.iter().any(|i| i.status == QuarantineStatus::Failed) {
        SessionStatus::Partial
    } else {
        SessionStatus::Completed
    }
}

/// The quarantine of one engine.
pub struct Quarantine<'a> {
    /// `<data_dir>\quarantine`.
    pub root: &'a Path,
    /// File system (validation of paths).
    pub fs: &'a dyn FileSystem,
    /// Registry.
    pub registry: &'a dyn Registry,
    /// Safety rules, checked right before every action.
    pub rules: &'a SafetyRules,
}

impl Quarantine<'_> {
    fn session_dir(&self, id: Uuid) -> PathBuf {
        self.root.join(id.to_string())
    }

    /// Writes the manifest atomically (temporary file, then rename).
    fn save(&self, manifest: &Manifest) -> CoreResult<()> {
        let dir = self.session_dir(manifest.session.id);
        fs::create_dir_all(&dir).map_err(|e| CoreError::from_io(&dir, e))?;
        let path = dir.join(MANIFEST);
        let temp = dir.join("manifest.json.tmp");
        let json = serde_json::to_vec_pretty(manifest)?;
        fs::write(&temp, json).map_err(|e| CoreError::from_io(&temp, e))?;
        fs::rename(&temp, &path).map_err(|e| CoreError::from_io(&path, e))
    }

    /// Reads the manifest of a session.
    pub fn load(&self, id: Uuid) -> CoreResult<Manifest> {
        let path = self.session_dir(id).join(MANIFEST);
        let bytes = fs::read(&path).map_err(|e| CoreError::from_io(&path, e))?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Every session found in the quarantine folder, recovered if needed.
    pub fn sessions(&self) -> Vec<Session> {
        let Ok(entries) = fs::read_dir(self.root) else {
            return Vec::new();
        };
        let mut sessions: Vec<Session> = entries
            .flatten()
            .filter_map(|e| Uuid::parse_str(&e.file_name().to_string_lossy()).ok())
            .filter_map(|id| self.load(id).ok())
            .filter_map(|m| self.recover(m).ok())
            .map(|m| m.session)
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        sessions
    }

    /// Checks the 20 GB / 50 000 files ceiling (specs § 9.2).
    pub fn check_ceiling(&self, items: &[Leftover], confirmation: Option<&str>) -> CoreResult<()> {
        let bytes: u64 = items.iter().map(|i| i.size_bytes).sum();
        let files: u64 = items
            .iter()
            .filter(|i| i.kind != LeftoverKind::RegistryKey)
            .map(|i| {
                collect_files(Path::new(&i.location))
                    .map(|(f, _)| f.len() as u64)
                    .unwrap_or(0)
            })
            .sum();
        let confirmed = confirmation.is_some_and(|c| c.trim() == CONFIRMATION_WORD);
        if (bytes > MAX_BYTES || files > MAX_FILES) && !confirmed {
            return Err(CoreError::ConfirmationRequired { bytes, files });
        }
        Ok(())
    }

    /// Moves `items` to a new quarantine session (EF-17). A failure on one
    /// item never stops the session (ENF-09).
    pub fn clean(
        &self,
        origin: Origin,
        items: &[Leftover],
        confirmation: Option<&str>,
        now: DateTime<Utc>,
    ) -> CoreResult<Session> {
        self.check_ceiling(items, confirmation)?;
        let mut manifest = Manifest {
            session: Session {
                id: Uuid::new_v4(),
                created_at: now,
                origin,
                items: Vec::new(),
                freed_bytes: 0,
                errors: Vec::new(),
                status: SessionStatus::Completed,
                simulated: false,
            },
            pending: None,
            files: BTreeMap::new(),
        };
        self.save(&manifest)?;
        for leftover in items {
            self.quarantine_one(&mut manifest, leftover, now)?;
        }
        let session = &mut manifest.session;
        session.freed_bytes = session
            .items
            .iter()
            .filter(|i| {
                matches!(
                    i.status,
                    QuarantineStatus::Quarantined | QuarantineStatus::PendingReboot
                )
            })
            .map(|i| i.size_bytes)
            .sum();
        session.errors = session
            .items
            .iter()
            .filter_map(|i| i.error.clone())
            .collect();
        session.status = status_of(&session.items);
        self.save(&manifest)?;
        tracing::info!(id = %manifest.session.id, items = manifest.session.items.len(), "quarantine session done");
        Ok(manifest.session)
    }

    fn quarantine_one(
        &self,
        manifest: &mut Manifest,
        leftover: &Leftover,
        now: DateTime<Utc>,
    ) -> CoreResult<()> {
        let mut item = QuarantinedItem {
            id: leftover.id.clone(),
            kind: leftover.kind,
            original_location: leftover.location.clone(),
            quarantine_path: PathBuf::new(),
            size_bytes: leftover.size_bytes,
            sha256: None,
            original_acl: None,
            quarantined_at: now,
            restored_at: None,
            status: QuarantineStatus::Failed,
            error: None,
        };
        let dir = self.session_dir(manifest.session.id);
        let outcome = match leftover.kind {
            LeftoverKind::Folder | LeftoverKind::File | LeftoverKind::Shortcut => {
                self.move_files(manifest, &mut item, &dir)
            }
            LeftoverKind::RegistryKey => self.move_key(manifest, &mut item, &dir),
            _ => Err((
                "UNSUPPORTED".into(),
                "type d'élément non pris en charge".into(),
            )),
        };
        if let Err((code, message)) = outcome {
            item.status = QuarantineStatus::Failed;
            item.error = Some(item_error(&code, message, &leftover.location));
        }
        manifest.pending = None;
        replace_item(&mut manifest.session.items, item);
        self.save(manifest)
    }

    /// Files and folders: validate, hash, record, then move.
    fn move_files(
        &self,
        manifest: &mut Manifest,
        item: &mut QuarantinedItem,
        dir: &Path,
    ) -> Result<(), (String, String)> {
        let original = PathBuf::from(&item.original_location);
        let to_pair = |e: CoreError| (e.code().to_owned(), e.to_string());
        // Last line of defence, right before acting (safety rule 3).
        self.rules
            .validate_path(self.fs, &original)
            .map_err(to_pair)?;
        let (files, has_link) = collect_files(&original).map_err(to_pair)?;
        if has_link {
            return Err((
                "CONTAINS_LINK".into(),
                "contient un lien symbolique ou une jonction".into(),
            ));
        }
        let (hashes, global) = hash_files(&files).map_err(to_pair)?;
        let name = original
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        let target = dir.join("files").join(&item.id).join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| to_pair(CoreError::from_io(parent, e)))?;
        }
        item.quarantine_path = target.clone();
        item.sha256 = Some(global);
        item.original_acl = native::sddl_of(&original);
        item.status = QuarantineStatus::Quarantined;
        item.size_bytes = files.iter().map(|(_, _, s)| s).sum();
        manifest.files.insert(item.id.clone(), hashes.clone());
        // Manifest written BEFORE the move (specs § 7.2).
        manifest.pending = Some(item.id.clone());
        replace_item(&mut manifest.session.items, item.clone());
        self.save(manifest).map_err(to_pair)?;

        match relocate(&original, &target, &hashes) {
            Ok(()) => Ok(()),
            Err((code, _)) if code == "LOCKED" && item.kind != LeftoverKind::Folder => {
                native::move_at_reboot(&original, &target)
                    .map(|()| item.status = QuarantineStatus::PendingReboot)
                    .map_err(|_| {
                        (
                            "LOCKED".to_owned(),
                            "fichier utilisé par un programme".to_owned(),
                        )
                    })
            }
            Err(error) => Err(error),
        }
    }

    /// Registry keys: validate, snapshot (.json + .reg), record, then delete.
    fn move_key(
        &self,
        manifest: &mut Manifest,
        item: &mut QuarantinedItem,
        dir: &Path,
    ) -> Result<(), (String, String)> {
        let to_pair = |e: CoreError| (e.code().to_owned(), e.to_string());
        let key = RegKeyPath::parse(&item.original_location)
            .ok_or_else(|| ("INVALID_INPUT".to_owned(), "clé illisible".to_owned()))?;
        self.rules.validate_key(&key).map_err(to_pair)?;
        let snapshot = KeySnapshot::read(self.registry, &key).map_err(to_pair)?;
        let registry_dir = dir.join("registry");
        fs::create_dir_all(&registry_dir)
            .map_err(|e| to_pair(CoreError::from_io(&registry_dir, e)))?;
        let json_path = registry_dir.join(format!("{}.json", item.id));
        let reg_path = registry_dir.join(format!("{}.reg", item.id));
        let json = serde_json::to_vec_pretty(&snapshot).map_err(|e| to_pair(e.into()))?;
        fs::write(&json_path, &json).map_err(|e| to_pair(CoreError::from_io(&json_path, e)))?;
        fs::write(&reg_path, utf16_le_with_bom(&snapshot.to_reg_file(&key)))
            .map_err(|e| to_pair(CoreError::from_io(&reg_path, e)))?;
        item.quarantine_path = json_path;
        item.sha256 = Some(Sha256::of_bytes(&json).map_err(to_pair)?);
        item.size_bytes = snapshot.size_bytes();
        item.status = QuarantineStatus::Quarantined;
        manifest.pending = Some(item.id.clone());
        replace_item(&mut manifest.session.items, item.clone());
        self.save(manifest).map_err(to_pair)?;
        self.registry.delete_tree(&key).map_err(to_pair)
    }

    /// Resumes a session interrupted in the middle of a move (crash).
    pub fn recover(&self, mut manifest: Manifest) -> CoreResult<Manifest> {
        let Some(pending) = manifest.pending.take() else {
            return Ok(manifest);
        };
        if let Some(item) = manifest.session.items.iter_mut().find(|i| i.id == pending) {
            let moved = match item.kind {
                LeftoverKind::RegistryKey => RegKeyPath::parse(&item.original_location)
                    .is_some_and(|k| !self.registry.key_exists(&k)),
                _ => {
                    let original_there = fs::symlink_metadata(&item.original_location).is_ok();
                    let copy_there = fs::symlink_metadata(&item.quarantine_path).is_ok();
                    if original_there && copy_there {
                        // Interrupted copy: the original is intact, drop the copy.
                        let _ = remove_tree(&item.quarantine_path);
                    }
                    !original_there && copy_there
                }
            };
            if !moved {
                item.status = QuarantineStatus::Failed;
                item.error = Some(item_error(
                    "INTERRUPTED",
                    "opération interrompue, élément laissé en place",
                    &item.original_location,
                ));
            }
        }
        manifest.session.errors = manifest
            .session
            .items
            .iter()
            .filter_map(|i| i.error.clone())
            .collect();
        manifest.session.status = status_of(&manifest.session.items);
        self.save(&manifest)?;
        Ok(manifest)
    }

    /// Puts items back (EF-18): all of them, or only `item_ids`. Refuses an
    /// item whose original location exists again.
    pub fn restore(
        &self,
        id: Uuid,
        item_ids: Option<&[String]>,
        now: DateTime<Utc>,
    ) -> CoreResult<Session> {
        let mut manifest = self.recover(self.load(id)?)?;
        let selected = |item: &QuarantinedItem| item_ids.is_none_or(|ids| ids.contains(&item.id));
        let mut errors = Vec::new();
        for index in 0..manifest.session.items.len() {
            let item = manifest.session.items[index].clone();
            let restorable = matches!(
                item.status,
                QuarantineStatus::Quarantined | QuarantineStatus::PendingReboot
            );
            if !restorable || !selected(&item) {
                continue;
            }
            let hashes = manifest.files.get(&item.id).cloned().unwrap_or_default();
            match self.restore_one(&item, &hashes) {
                Ok(()) => {
                    let restored = &mut manifest.session.items[index];
                    restored.status = QuarantineStatus::Restored;
                    restored.restored_at = Some(now);
                    restored.error = None;
                }
                Err((code, message)) => {
                    errors.push(item_error(&code, message, &item.original_location))
                }
            }
            self.save(&manifest)?;
        }
        manifest.session.status = status_of(&manifest.session.items);
        self.save(&manifest)?;
        let mut session = manifest.session;
        session.errors = errors;
        Ok(session)
    }

    fn restore_one(
        &self,
        item: &QuarantinedItem,
        hashes: &[FileHash],
    ) -> Result<(), (String, String)> {
        let to_pair = |e: CoreError| (e.code().to_owned(), e.to_string());
        let exists = || {
            (
                "DESTINATION_EXISTS".to_owned(),
                "un élément existe déjà à cet emplacement".to_owned(),
            )
        };
        if item.kind == LeftoverKind::RegistryKey {
            let key = RegKeyPath::parse(&item.original_location)
                .ok_or_else(|| ("INVALID_INPUT".to_owned(), "clé illisible".to_owned()))?;
            if self.registry.key_exists(&key) {
                return Err(exists());
            }
            let bytes = fs::read(&item.quarantine_path)
                .map_err(|e| to_pair(CoreError::from_io(&item.quarantine_path, e)))?;
            let snapshot: KeySnapshot =
                serde_json::from_slice(&bytes).map_err(|e| to_pair(e.into()))?;
            return snapshot.write(self.registry, &key).map_err(to_pair);
        }
        let original = PathBuf::from(&item.original_location);
        self.rules.check_path(&original).map_err(to_pair)?;
        if fs::symlink_metadata(&original).is_ok() {
            return Err(exists());
        }
        if let Some(parent) = original.parent() {
            fs::create_dir_all(parent).map_err(|e| to_pair(CoreError::from_io(parent, e)))?;
        }
        relocate(&item.quarantine_path, &original, hashes)
    }

    /// Deletes for good the sessions older than `days` (EF-19), only inside
    /// the quarantine folder. Returns the freed bytes.
    pub fn purge(&self, days: u32, now: DateTime<Utc>) -> CoreResult<u64> {
        let limit = now - Duration::days(i64::from(days));
        let mut freed = 0;
        for session in self.sessions() {
            if session.created_at > limit || session.status == SessionStatus::Purged {
                continue;
            }
            let mut manifest = self.load(session.id)?;
            for kind in ["files", "registry"] {
                let dir = self.session_dir(session.id).join(kind);
                // Never outside the quarantine folder.
                if dir.starts_with(self.root) && dir.exists() {
                    remove_tree(&dir)?;
                }
            }
            for item in &mut manifest.session.items {
                if matches!(
                    item.status,
                    QuarantineStatus::Quarantined | QuarantineStatus::PendingReboot
                ) {
                    freed += item.size_bytes;
                    item.status = QuarantineStatus::Purged;
                }
            }
            manifest.session.status = SessionStatus::Purged;
            self.save(&manifest)?;
        }
        Ok(freed)
    }
}

fn replace_item(items: &mut Vec<QuarantinedItem>, item: QuarantinedItem) {
    match items.iter_mut().find(|i| i.id == item.id) {
        Some(existing) => *existing = item,
        None => items.push(item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{strip_verbatim, RealFileSystem};
    use crate::model::Confidence;
    use crate::registry::{Hive, RegData, TestRegistryKey};
    use crate::safety::KnownFolders;
    use std::time::SystemTime;

    /// Test machine rooted in a temporary folder under core/target (no
    /// « Temp » component, which the blacklist protects).
    struct Lab {
        _dir: tempfile::TempDir,
        base: PathBuf,
        rules: SafetyRules,
        quarantine: PathBuf,
        area: TestRegistryKey,
    }

    impl Lab {
        fn new() -> Self {
            let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\quarantine-tests");
            fs::create_dir_all(&parent).unwrap();
            let dir = tempfile::tempdir_in(&parent).unwrap();
            let base = strip_verbatim(&dir.path().canonicalize().unwrap());
            let rules = SafetyRules::new(&KnownFolders::under(&base), &[]);
            let quarantine = base.join(r"data\quarantine");
            Lab {
                _dir: dir,
                base,
                rules,
                quarantine,
                area: TestRegistryKey::new(),
            }
        }

        fn roaming(&self, rel: &str) -> PathBuf {
            self.base.join(r"Users\Ana\AppData\Roaming").join(rel)
        }

        fn write(&self, path: &Path, content: &[u8]) {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn run<T>(&self, f: impl FnOnce(&Quarantine) -> T) -> T {
            let registry = self.area.registry();
            let q = Quarantine {
                root: &self.quarantine,
                fs: &RealFileSystem,
                registry: &registry,
                rules: &self.rules,
            };
            f(&q)
        }
    }

    fn leftover(kind: LeftoverKind, location: &str, size: u64) -> Leftover {
        Leftover {
            id: crate::inventory::fnv1a_hex(location),
            kind,
            location: location.into(),
            size_bytes: size,
            last_modified: None,
            confidence: Confidence::High,
            reasons: vec![],
            guessed_app: None,
        }
    }

    fn mtime(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    #[test]
    fn quarantine_then_restore_identically() {
        let lab = Lab::new();
        let folder = lab.roaming("audacity");
        lab.write(&folder.join("audacity.cfg"), b"[prefs]\nlang=fr\n");
        lab.write(&folder.join(r"plug-ins\a.ny"), &[1u8; 5000]);
        let before = mtime(&folder.join("audacity.cfg"));

        let key = RegKeyPath::new(Hive::CurrentUser, r"Software\Audacity");
        let k = lab.area.create(&key);
        k.set_value("Lang", &"fr").unwrap();
        k.set_value("Count", &3u32).unwrap();
        lab.area
            .create(&key.join("Window"))
            .set_value("X", &10u32)
            .unwrap();
        let registry = lab.area.registry();
        let key_before = KeySnapshot::read(&registry, &key).unwrap();

        let items = [
            leftover(LeftoverKind::Folder, &folder.to_string_lossy(), 0),
            leftover(LeftoverKind::RegistryKey, &key.to_string(), 0),
        ];
        let session = lab
            .run(|q| q.clean(Origin::Orphans, &items, None, Utc::now()))
            .unwrap();
        assert_eq!(
            session.status,
            SessionStatus::Completed,
            "{:?}",
            session.errors
        );
        assert!(!folder.exists());
        assert!(!registry.key_exists(&key));
        assert_eq!(session.freed_bytes, 16 + 5000 + session.items[1].size_bytes);
        let reg_file = lab
            .quarantine
            .join(session.id.to_string())
            .join(format!(r"registry\{}.reg", items[1].id));
        assert_eq!(&fs::read(reg_file).unwrap()[..2], &[0xFF, 0xFE]);
        assert!(session.items[0].original_acl.is_some());
        assert_eq!(session.items[0].sha256.as_ref().unwrap().len(), 64);

        let restored = lab
            .run(|q| q.restore(session.id, None, Utc::now()))
            .unwrap();
        assert_eq!(restored.status, SessionStatus::Restored);
        assert_eq!(
            fs::read(folder.join("audacity.cfg")).unwrap(),
            b"[prefs]\nlang=fr\n"
        );
        assert_eq!(
            fs::read(folder.join(r"plug-ins\a.ny")).unwrap(),
            vec![1u8; 5000]
        );
        assert_eq!(mtime(&folder.join("audacity.cfg")), before);
        assert_eq!(KeySnapshot::read(&registry, &key).unwrap(), key_before);
    }

    #[test]
    fn partial_restore_and_existing_destination() {
        let lab = Lab::new();
        let a = lab.roaming("AppA");
        let b = lab.roaming("AppB");
        lab.write(&a.join("x"), b"a");
        lab.write(&b.join("y"), b"b");
        let items = [
            leftover(LeftoverKind::Folder, &a.to_string_lossy(), 1),
            leftover(LeftoverKind::Folder, &b.to_string_lossy(), 1),
        ];
        let session = lab
            .run(|q| q.clean(Origin::Orphans, &items, None, Utc::now()))
            .unwrap();

        let only_a = [items[0].id.clone()];
        let partial = lab
            .run(|q| q.restore(session.id, Some(&only_a), Utc::now()))
            .unwrap();
        assert!(a.exists() && !b.exists());
        assert_eq!(partial.status, SessionStatus::Completed);

        lab.write(&b.join("new"), b"someone recreated it");
        let refused = lab
            .run(|q| q.restore(session.id, None, Utc::now()))
            .unwrap();
        assert_eq!(refused.errors[0].code, "DESTINATION_EXISTS");
        assert_eq!(fs::read(b.join("new")).unwrap(), b"someone recreated it");
    }

    #[test]
    fn protected_and_linked_items_are_refused_and_left_in_place() {
        let lab = Lab::new();
        let protected = lab.base.join(r"Users\Ana\AppData\Roaming\Microsoft\Teams");
        lab.write(&protected.join("x"), b"x");
        let linked = lab.roaming("WithLink");
        lab.write(&linked.join("f"), b"f");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(linked.join("trap"))
            .arg(lab.roaming("WithLink"))
            .output()
            .unwrap();
        assert!(status.status.success());
        let items = [
            leftover(LeftoverKind::Folder, &protected.to_string_lossy(), 1),
            leftover(LeftoverKind::Folder, &linked.to_string_lossy(), 1),
            leftover(
                LeftoverKind::RegistryKey,
                r"HKLM\SOFTWARE\Microsoft\Windows",
                1,
            ),
            leftover(LeftoverKind::Service, "Svc", 1),
        ];
        let session = lab
            .run(|q| q.clean(Origin::Orphans, &items, None, Utc::now()))
            .unwrap();
        let codes: Vec<&str> = session.errors.iter().map(|e| e.code.as_str()).collect();
        assert_eq!(
            codes,
            ["PROTECTED", "CONTAINS_LINK", "PROTECTED", "UNSUPPORTED"]
        );
        assert_eq!(session.status, SessionStatus::Partial);
        assert_eq!(session.freed_bytes, 0);
        assert!(protected.join("x").exists() && linked.join("f").exists());
        fs::remove_dir(linked.join("trap")).unwrap();
    }

    #[test]
    fn ceiling_requires_the_confirmation_word() {
        let lab = Lab::new();
        let big = [leftover(
            LeftoverKind::RegistryKey,
            r"HKCU\Software\Big",
            MAX_BYTES + 1,
        )];
        let err = lab.run(|q| q.check_ceiling(&big, None)).unwrap_err();
        assert_eq!(err.code(), "CONFIRMATION_REQUIRED");
        assert!(lab
            .run(|q| q.check_ceiling(&big, Some("supprimer")))
            .is_err());
        assert!(lab
            .run(|q| q.check_ceiling(&big, Some(" QUARANTAINE ")))
            .is_ok());
    }

    #[test]
    fn interrupted_session_is_recovered() {
        let lab = Lab::new();
        let kept = lab.roaming("Kept");
        lab.write(&kept.join("k"), b"k");
        let session = lab
            .run(|q| q.clean(Origin::Orphans, &[], None, Utc::now()))
            .unwrap();
        // Crash just after writing the manifest, before the move.
        let mut manifest = lab.run(|q| q.load(session.id)).unwrap();
        let id = "pending-item".to_owned();
        manifest.session.items.push(QuarantinedItem {
            id: id.clone(),
            kind: LeftoverKind::Folder,
            original_location: kept.to_string_lossy().into_owned(),
            quarantine_path: lab
                .quarantine
                .join(session.id.to_string())
                .join(r"files\pending-item\Kept"),
            size_bytes: 1,
            sha256: None,
            original_acl: None,
            quarantined_at: Utc::now(),
            restored_at: None,
            status: QuarantineStatus::Quarantined,
            error: None,
        });
        manifest.pending = Some(id);
        lab.run(|q| q.save(&manifest)).unwrap();

        let sessions = lab.run(|q| q.sessions());
        let item = &sessions[0].items[0];
        assert_eq!(item.status, QuarantineStatus::Failed);
        assert_eq!(item.error.as_ref().unwrap().code, "INTERRUPTED");
        assert!(kept.join("k").exists());
        assert!(lab.run(|q| q.load(session.id)).unwrap().pending.is_none());
    }

    #[test]
    fn purge_removes_only_old_sessions_inside_the_quarantine() {
        let lab = Lab::new();
        let old = lab.roaming("Old");
        lab.write(&old.join("o"), b"old!");
        let recent = lab.roaming("Recent");
        lab.write(&recent.join("r"), b"r");
        let long_ago = Utc::now() - Duration::days(40);
        let s1 = lab
            .run(|q| {
                q.clean(
                    Origin::Orphans,
                    &[leftover(LeftoverKind::Folder, &old.to_string_lossy(), 4)],
                    None,
                    long_ago,
                )
            })
            .unwrap();
        let s2 = lab
            .run(|q| {
                q.clean(
                    Origin::Orphans,
                    &[leftover(LeftoverKind::Folder, &recent.to_string_lossy(), 1)],
                    None,
                    Utc::now(),
                )
            })
            .unwrap();

        let freed = lab.run(|q| q.purge(30, Utc::now())).unwrap();
        assert_eq!(freed, 4);
        let sessions = lab.run(|q| q.sessions());
        let status = |id: Uuid| sessions.iter().find(|s| s.id == id).unwrap().status;
        assert_eq!(status(s1.id), SessionStatus::Purged);
        assert_eq!(status(s2.id), SessionStatus::Completed);
        assert!(!lab
            .quarantine
            .join(s1.id.to_string())
            .join("files")
            .exists());
        assert!(lab
            .quarantine
            .join(s2.id.to_string())
            .join("files")
            .exists());
        assert!(
            lab.run(|q| q.restore(s1.id, None, Utc::now()))
                .unwrap()
                .items[0]
                .status
                == QuarantineStatus::Purged
        );
    }

    #[test]
    fn simulation_touches_nothing() {
        let items = [leftover(
            LeftoverKind::Folder,
            r"C:\Users\Ana\AppData\Roaming\x",
            42,
        )];
        let session = simulate(Origin::Orphans, &items, Utc::now());
        assert!(session.simulated);
        assert_eq!(session.freed_bytes, 42);
        assert!(session.id.is_nil());
    }

    #[test]
    fn registry_data_helper_is_used() {
        // Keep RegData in scope for readability of the snapshot assertions.
        assert_eq!(RegData::Dword(1), RegData::Dword(1));
    }
}
