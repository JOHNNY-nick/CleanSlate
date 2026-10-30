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
use crate::services::{ServiceConfig, ServiceControl};
use crate::store::PackageStore;
use crate::tasks::TaskStore;

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
    /// Scheduled tasks (export, delete, re-register).
    pub tasks: &'a dyn TaskStore,
    /// Services (query, delete, recreate).
    pub services: &'a dyn ServiceControl,
    /// Store packages of the current user (D48: their data is quarantined
    /// only once the package is gone).
    pub packages: &'a dyn PackageStore,
}

/// What `services\<id>.json` holds: enough to recreate the service and
/// put its key back identically (D47).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceBackup {
    /// Configuration read from the Service Control Manager.
    pub config: ServiceConfig,
    /// `HKLM\SYSTEM\CurrentControlSet\Services\<name>` and everything below.
    pub key: KeySnapshot,
}

/// Name of the service of a `…\Services\<name>` location.
fn service_name(location: &str) -> &str {
    location.rsplit('\\').next().unwrap_or_default()
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
            .filter(|i| {
                matches!(
                    i.kind,
                    LeftoverKind::Folder | LeftoverKind::File | LeftoverKind::Shortcut
                )
            })
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
        // Start menu folders last: they are quarantined only once their dead
        // shortcuts are gone (D20).
        let mut ordered: Vec<&Leftover> = items.iter().collect();
        ordered.sort_by_key(|i| {
            i.kind == LeftoverKind::Folder && self.rules.is_below_start_menu(Path::new(&i.location))
        });
        for leftover in ordered {
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
            LeftoverKind::ScheduledTask => self.move_task(manifest, &mut item, &dir),
            LeftoverKind::Service => self.move_service(manifest, &mut item, &dir),
            _ => Err((
                "UNSUPPORTED".into(),
                "type d'élément non pris en charge".into(),
            )),
        };
        if let Err((code, message)) = outcome {
            item.status = QuarantineStatus::Failed;
            item.error = Some(item_error(&code, message, &leftover.location));
        }
        tracing::info!(location = %item.original_location, status = ?item.status, "quarantine item");
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
        // Last line of defence, right before acting (safety rule 3); a Start
        // menu shortcut or emptied folder passes only through D20.
        let validation = if self.rules.is_package_folder(&original) {
            self.installed_families().and_then(|installed| {
                self.rules
                    .validate_package_folder(self.fs, &original, &installed)
            })
        } else {
            self.rules.validate_item_path(self.fs, item.kind, &original)
        };
        validation.map_err(to_pair)?;
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
        let validation = if self.rules.is_package_key(&key) {
            self.installed_families()
                .and_then(|installed| self.rules.validate_package_key(&key, &installed))
        } else {
            self.rules.validate_key(&key)
        };
        validation.map_err(to_pair)?;
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

    /// Family names of the packages of the user, listed right now (D48).
    fn installed_families(&self) -> CoreResult<Vec<String>> {
        Ok(self
            .packages
            .list()?
            .into_iter()
            .map(|p| p.family_name)
            .collect())
    }

    /// Scheduled tasks: read the definition now, validate it (D47), save
    /// the full XML and record it, then delete the task.
    fn move_task(
        &self,
        manifest: &mut Manifest,
        item: &mut QuarantinedItem,
        dir: &Path,
    ) -> Result<(), (String, String)> {
        let to_pair = |e: CoreError| (e.code().to_owned(), e.to_string());
        let path = item.original_location.clone();
        let xml = self.tasks.read(&path).map_err(to_pair)?;
        // Last line of defence, right before acting (safety rule 3).
        self.rules
            .validate_task(self.fs, &path, &xml)
            .map_err(to_pair)?;
        let tasks_dir = dir.join("tasks");
        fs::create_dir_all(&tasks_dir).map_err(|e| to_pair(CoreError::from_io(&tasks_dir, e)))?;
        let file = tasks_dir.join(format!("{}.xml", item.id));
        fs::write(&file, xml.as_bytes()).map_err(|e| to_pair(CoreError::from_io(&file, e)))?;
        item.quarantine_path = file;
        item.sha256 = Some(Sha256::of_bytes(xml.as_bytes()).map_err(to_pair)?);
        item.size_bytes = xml.len() as u64;
        item.status = QuarantineStatus::Quarantined;
        manifest.pending = Some(item.id.clone());
        replace_item(&mut manifest.session.items, item.clone());
        self.save(manifest).map_err(to_pair)?;
        self.tasks.delete(&path).map_err(to_pair)
    }

    /// Services: query the service now, validate it and its key (D47),
    /// save its configuration and key (`.json` + `.reg`), record, then
    /// `DeleteService`.
    fn move_service(
        &self,
        manifest: &mut Manifest,
        item: &mut QuarantinedItem,
        dir: &Path,
    ) -> Result<(), (String, String)> {
        let to_pair = |e: CoreError| (e.code().to_owned(), e.to_string());
        let key = RegKeyPath::parse(&item.original_location)
            .ok_or_else(|| ("INVALID_INPUT".to_owned(), "clé illisible".to_owned()))?;
        let name = service_name(&item.original_location).to_owned();
        self.rules.check_service_key(&key, &name).map_err(to_pair)?;
        let state = self.services.query(&name).map_err(to_pair)?;
        let dll = self
            .registry
            .values(&key.join("Parameters"))
            .ok()
            .and_then(|values| {
                values
                    .into_iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case("ServiceDll"))
                    .and_then(|(_, v)| v.as_str().map(str::to_owned))
            });
        self.rules
            .validate_service(self.fs, &state, dll.as_deref())
            .map_err(to_pair)?;
        let backup = ServiceBackup {
            config: state.config,
            key: KeySnapshot::read(self.registry, &key).map_err(to_pair)?,
        };
        let services_dir = dir.join("services");
        fs::create_dir_all(&services_dir)
            .map_err(|e| to_pair(CoreError::from_io(&services_dir, e)))?;
        let json_path = services_dir.join(format!("{}.json", item.id));
        let reg_path = services_dir.join(format!("{}.reg", item.id));
        let json = serde_json::to_vec_pretty(&backup).map_err(|e| to_pair(e.into()))?;
        fs::write(&json_path, &json).map_err(|e| to_pair(CoreError::from_io(&json_path, e)))?;
        fs::write(&reg_path, utf16_le_with_bom(&backup.key.to_reg_file(&key)))
            .map_err(|e| to_pair(CoreError::from_io(&reg_path, e)))?;
        item.quarantine_path = json_path;
        item.sha256 = Some(Sha256::of_bytes(&json).map_err(to_pair)?);
        item.size_bytes = backup.key.size_bytes();
        item.status = QuarantineStatus::Quarantined;
        manifest.pending = Some(item.id.clone());
        replace_item(&mut manifest.session.items, item.clone());
        self.save(manifest).map_err(to_pair)?;
        self.services.delete(&name).map_err(to_pair)
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
                LeftoverKind::ScheduledTask => {
                    item.quarantine_path.exists() && !self.tasks.exists(&item.original_location)
                }
                LeftoverKind::Service => {
                    item.quarantine_path.exists()
                        && !self.services.exists(service_name(&item.original_location))
                }
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
        // Reverse order of the quarantine: Start menu folders first, then
        // the shortcuts that go back into them (D20).
        let mut order: Vec<usize> = (0..manifest.session.items.len()).collect();
        order.sort_by_key(|&i| {
            let item = &manifest.session.items[i];
            !(item.kind == LeftoverKind::Folder
                && self
                    .rules
                    .is_below_start_menu(Path::new(&item.original_location)))
        });
        for index in order {
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
                    tracing::info!(location = %restored.original_location, "item restored");
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
        if item.kind == LeftoverKind::ScheduledTask {
            let path = &item.original_location;
            self.rules.check_task_path(path).map_err(to_pair)?;
            if self.tasks.exists(path) {
                return Err(exists());
            }
            let xml = fs::read_to_string(&item.quarantine_path)
                .map_err(|e| to_pair(CoreError::from_io(&item.quarantine_path, e)))?;
            // Same folder, same name, same definition.
            return self.tasks.register(path, &xml).map_err(to_pair);
        }
        if item.kind == LeftoverKind::Service {
            let key = RegKeyPath::parse(&item.original_location)
                .ok_or_else(|| ("INVALID_INPUT".to_owned(), "clé illisible".to_owned()))?;
            let name = service_name(&item.original_location);
            self.rules.check_service_key(&key, name).map_err(to_pair)?;
            if self.services.exists(name) {
                return Err(exists());
            }
            let bytes = fs::read(&item.quarantine_path)
                .map_err(|e| to_pair(CoreError::from_io(&item.quarantine_path, e)))?;
            let backup: ServiceBackup =
                serde_json::from_slice(&bytes).map_err(|e| to_pair(e.into()))?;
            self.services.create(&backup.config).map_err(to_pair)?;
            // Then every value and sub-key of its key, as saved.
            return backup.key.write(self.registry, &key).map_err(to_pair);
        }
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
        self.rules
            .check_item_path(item.kind, &original)
            .map_err(to_pair)?;
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
            for kind in ["files", "registry", "tasks", "services"] {
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
            tracing::info!(id = %session.id, "session purged");
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
        tasks: crate::tasks::FakeTaskStore,
        services: crate::services::FakeServiceControl,
        packages: crate::store::FakePackageStore,
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
                tasks: Default::default(),
                services: Default::default(),
                packages: Default::default(),
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
                tasks: &self.tasks,
                services: &self.services,
                packages: &self.packages,
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

    /// D20: a dead Start menu shortcut and its emptied folder go to
    /// quarantine (folder last) and come back identically.
    #[test]
    fn start_menu_shortcut_and_folder_round_trip() {
        let lab = Lab::new();
        let menu = lab.roaming(r"Microsoft\Windows\Start Menu\Programs");
        let folder = menu.join("OldTool");
        let shortcut = folder.join("OldTool.lnk");
        let bytes = crate::shortcut::tests::Lnk::local(r"C:\Gone\old.exe").build();
        lab.write(&shortcut, &bytes);
        let before = mtime(&shortcut);

        // The folder alone is refused while it still holds the shortcut.
        let folder_item = leftover(LeftoverKind::Folder, &folder.to_string_lossy(), 0);
        let shortcut_item = leftover(LeftoverKind::Shortcut, &shortcut.to_string_lossy(), 10);
        let alone = lab.run(|q| {
            q.clean(
                Origin::Orphans,
                std::slice::from_ref(&folder_item),
                None,
                Utc::now(),
            )
            .unwrap()
        });
        assert_eq!(alone.items[0].status, QuarantineStatus::Failed);
        assert!(shortcut.exists());

        // Folder listed first: still processed after its shortcut.
        let session = lab.run(|q| {
            q.clean(
                Origin::Orphans,
                &[folder_item.clone(), shortcut_item.clone()],
                None,
                Utc::now(),
            )
            .unwrap()
        });
        assert_eq!(
            session.status,
            SessionStatus::Completed,
            "{:?}",
            session.errors
        );
        assert!(!folder.exists());
        assert!(menu.exists(), "the Start menu root is never touched");

        let restored = lab.run(|q| q.restore(session.id, None, Utc::now()).unwrap());
        assert_eq!(
            restored.status,
            SessionStatus::Restored,
            "{:?}",
            restored.errors
        );
        assert_eq!(fs::read(&shortcut).unwrap(), bytes);
        assert_eq!(mtime(&shortcut), before);
    }

    /// D47: a dead task leaves through the store with its full XML saved
    /// first, and comes back identical (same path, same definition).
    #[test]
    fn task_round_trip_with_fake_store() {
        let lab = Lab::new();
        let xml = "<Task><RegistrationInfo><Author>Vendor</Author></RegistrationInfo>\
                   <Actions><Exec><Command>C:\\Gone\\upd.exe</Command><Arguments>/q</Arguments></Exec></Actions></Task>";
        lab.tasks.register(r"\Vendor\Updater", xml).unwrap();
        lab.tasks
            .register(r"\Microsoft\Windows\Old", &xml.replace("upd", "old"))
            .unwrap();
        let task = leftover(LeftoverKind::ScheduledTask, r"\Vendor\Updater", 0);
        let microsoft = leftover(LeftoverKind::ScheduledTask, r"\Microsoft\Windows\Old", 0);

        let session = lab.run(|q| {
            q.clean(
                Origin::Orphans,
                &[task.clone(), microsoft.clone()],
                None,
                Utc::now(),
            )
            .unwrap()
        });
        assert_eq!(session.items[0].status, QuarantineStatus::Quarantined);
        assert_eq!(session.items[1].status, QuarantineStatus::Failed);
        assert!(!lab.tasks.exists(r"\Vendor\Updater"));
        assert!(lab.tasks.exists(r"\Microsoft\Windows\Old"), "never touched");
        assert_eq!(
            fs::read_to_string(&session.items[0].quarantine_path).unwrap(),
            xml
        );

        let restored = lab.run(|q| q.restore(session.id, None, Utc::now()).unwrap());
        assert!(restored.errors.is_empty(), "{:?}", restored.errors);
        assert_eq!(lab.tasks.read(r"\Vendor\Updater").unwrap(), xml);
        // A task recreated meanwhile is never overwritten.
        let again = lab.run(|q| {
            q.clean(
                Origin::Orphans,
                std::slice::from_ref(&task),
                None,
                Utc::now(),
            )
            .unwrap()
        });
        lab.tasks.register(r"\Vendor\Updater", "<Task/>").unwrap();
        let refused = lab.run(|q| q.restore(again.id, None, Utc::now()).unwrap());
        assert_eq!(refused.errors[0].code, "DESTINATION_EXISTS");
    }

    /// D47: a dead service leaves with its configuration and key saved
    /// first, and is recreated identically; neighbours stay in place.
    #[test]
    fn service_round_trip_with_fake_control() {
        use crate::services::{tests::full_config, ServiceState, STATE_STOPPED};
        use crate::system::SERVICES_KEY;
        let lab = Lab::new();
        let registry = lab.area.registry();
        let services = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY);
        let key = services.join("OldSvc");
        let config = full_config("OldSvc", r#""C:\Gone\svc.exe" -run"#);
        let k = lab.area.create(&key);
        k.set_value("ImagePath", &config.binary_path.as_str())
            .unwrap();
        k.set_value("Type", &0x10u32).unwrap();
        k.set_value("Start", &2u32).unwrap();
        k.set_value("Description", &"Agent de mise à jour").unwrap();
        lab.area
            .create(&key.join("Parameters"))
            .set_value("Channel", &"stable")
            .unwrap();
        let before = KeySnapshot::read(&registry, &key).unwrap();
        let state = |config: crate::services::ServiceConfig,
                     current_state: u32,
                     dependents: Vec<String>| {
            ServiceState {
                config,
                current_state,
                dependents,
            }
        };
        {
            let mut all = lab.services.0.lock().unwrap();
            all.insert(
                "oldsvc".into(),
                state(config.clone(), STATE_STOPPED, vec![]),
            );
            all.insert(
                "running".into(),
                state(full_config("Running", r"C:\Gone\r.exe"), 4, vec![]),
            );
            all.insert(
                "needed".into(),
                state(
                    full_config("Needed", r"C:\Gone\n.exe"),
                    STATE_STOPPED,
                    vec!["Other".into()],
                ),
            );
        }
        let item =
            |name: &str| leftover(LeftoverKind::Service, &services.join(name).to_string(), 0);

        let session = lab.run(|q| {
            q.clean(
                Origin::Orphans,
                &[item("OldSvc"), item("Running"), item("Needed")],
                None,
                Utc::now(),
            )
            .unwrap()
        });
        assert_eq!(
            session.items[0].status,
            QuarantineStatus::Quarantined,
            "{:?}",
            session.errors
        );
        assert_eq!(session.items[1].status, QuarantineStatus::Failed);
        assert_eq!(session.items[2].status, QuarantineStatus::Failed);
        assert!(!lab.services.exists("OldSvc"));
        assert!(lab.services.exists("Running") && lab.services.exists("Needed"));
        let saved: ServiceBackup =
            serde_json::from_slice(&fs::read(&session.items[0].quarantine_path).unwrap()).unwrap();
        assert_eq!(saved.config, config);
        assert_eq!(saved.key, before);
        assert!(session.items[0]
            .quarantine_path
            .with_extension("reg")
            .exists());

        // DeleteService removes the key: do it here, as the SCM would.
        registry.delete_tree(&key).unwrap();
        let restored = lab.run(|q| q.restore(session.id, None, Utc::now()).unwrap());
        assert!(restored.errors.is_empty(), "{:?}", restored.errors);
        assert_eq!(lab.services.query("OldSvc").unwrap().config, config);
        assert_eq!(KeySnapshot::read(&registry, &key).unwrap(), before);
    }

    /// A shortcut-shaped item that is not a shortcut never uses D20.
    #[test]
    fn start_menu_exception_is_kind_bound() {
        let lab = Lab::new();
        let menu = lab.roaming(r"Microsoft\Windows\Start Menu\Programs");
        let file = menu.join("Old.lnk");
        lab.write(&file, b"x");
        let as_file = leftover(LeftoverKind::File, &file.to_string_lossy(), 1);
        let session = lab.run(|q| {
            q.clean(Origin::Orphans, &[as_file], None, Utc::now())
                .unwrap()
        });
        assert_eq!(session.items[0].status, QuarantineStatus::Failed);
        assert!(file.exists());
        let root = leftover(LeftoverKind::Folder, &menu.to_string_lossy(), 1);
        fs::remove_file(&file).unwrap();
        let session = lab.run(|q| q.clean(Origin::Orphans, &[root], None, Utc::now()).unwrap());
        assert_eq!(session.items[0].status, QuarantineStatus::Failed);
        assert!(menu.exists());
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
            leftover(LeftoverKind::RegistryValue, r"HKCU\Software\X\Value", 1),
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
