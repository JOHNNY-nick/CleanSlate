//! Safety rules: the last line of defence (specs § 9, ENF-01, ENF-03).
//!
//! Every destructive action must call [`SafetyRules::validate_path`] or
//! [`SafetyRules::validate_key`] right before acting, even if the scan already
//! filtered the target. The blacklist is hard-coded and cannot be disabled;
//! user exclusions can only add protections, never remove one.

use std::path::{Component, Path, PathBuf};

use crate::error::{CoreError, CoreResult};
use crate::fs::FileSystem;
use crate::model::LeftoverKind;
use crate::registry::{Hive, RegKeyPath};
use crate::services::{is_builtin_account, ServiceState, DRIVER_TYPES, STATE_STOPPED, WIN32_TYPES};
use crate::store::is_family_name;
use crate::system::{command_executable_raw, program_path, SERVICES_KEY};
use crate::tasks::{parse_actions, TaskAction};

/// Folder names protected wherever they appear in a path (specs § 9.1).
pub const PROTECTED_NAMES: [&str; 18] = [
    "microsoft",
    "windows",
    "windows defender",
    "windows nt",
    "common files",
    "packages",
    "windowsapps",
    "temp",
    "microsoft.net",
    "dotnet",
    "intel",
    "nvidia corporation",
    "amd",
    "package cache",
    "cleanslate",
    "system volume information",
    "$recycle.bin",
    "recovery",
];

/// Registry key names protected wherever they appear in a key (specs § 9.1).
pub const PROTECTED_KEY_NAMES: [&str; 8] = [
    "microsoft",
    "classes",
    "policies",
    "clients",
    "registeredapplications",
    "windows",
    "cleanslate",
    // Test area of the registry (safety rule 2): never proposed.
    "cleanslatetest",
];

/// Registry trees protected with everything below them.
pub const PROTECTED_KEY_TREES: [&str; 4] = [
    r"HKLM\SYSTEM",
    r"HKLM\SAM",
    r"HKLM\SECURITY",
    r"HKLM\HARDWARE",
];

/// Registry scan roots (EF-06), protected themselves: only their children
/// are candidates.
pub const REGISTRY_SCAN_ROOTS: [&str; 3] = [
    r"HKCU\Software",
    r"HKLM\SOFTWARE",
    r"HKLM\SOFTWARE\WOW6432Node",
];

/// Folders of the Start menu holding Windows' own shortcuts: refused even
/// below the Start menu roots (narrowest form of D20).
pub const WINDOWS_START_MENU_FOLDERS: [&str; 12] = [
    "accessibility",
    "accessories",
    "administrative tools",
    "maintenance",
    "system tools",
    "windows accessories",
    "windows administrative tools",
    "windows ease of access",
    "windows powershell",
    "windows system",
    "windows tools",
    "windows kits",
];

/// Parents of the only two registry keys of a package family that may be
/// quarantined (D48); the family name is the single segment below them.
pub const PACKAGE_KEY_PARENTS: [&str; 2] = [
    r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Storage",
    r"HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData",
];

/// Publisher id of Windows' own packages: never touched (D48).
const WINDOWS_PUBLISHER_ID: &str = "cw5n1h2txyewy";

/// Start menu programs folder, below `%APPDATA%` or `%PROGRAMDATA%`.
const START_MENU_PROGRAMS: &str = r"Microsoft\Windows\Start Menu\Programs";

/// Well-known folders of the machine. Injectable so that tests never depend
/// on the real system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownFolders {
    /// `%WINDIR%`, protected with all its content.
    pub windir: PathBuf,
    /// `Program Files`.
    pub program_files: PathBuf,
    /// `Program Files (x86)`.
    pub program_files_x86: PathBuf,
    /// `%PROGRAMDATA%`.
    pub program_data: PathBuf,
    /// `C:\Users`: every profile folder below it is protected.
    pub users: PathBuf,
    /// `%USERPROFILE%`.
    pub user_profile: PathBuf,
    /// `%APPDATA%` (Roaming).
    pub roaming: PathBuf,
    /// `%LOCALAPPDATA%`.
    pub local: PathBuf,
    /// `AppData\LocalLow`.
    pub local_low: PathBuf,
    /// `%LOCALAPPDATA%\Programs`.
    pub local_programs: PathBuf,
    /// Documents, Desktop, Downloads, Pictures, Music, Videos, Public…
    pub user_folders: Vec<PathBuf>,
    /// Start menu and desktop folders holding shortcuts (EF-07).
    pub shortcut_folders: Vec<PathBuf>,
    /// The two Start menu roots (`%APPDATA%` and `%PROGRAMDATA%`
    /// `\Microsoft\Windows\Start Menu\Programs`): protected themselves; only
    /// the narrow exceptions of D20 apply below them.
    pub start_menu: Vec<PathBuf>,
    /// `Documents` (scan root of EF-05).
    pub documents: PathBuf,
}

impl KnownFolders {
    /// Folders of a fictitious machine whose system drive is `drive` (tests).
    pub fn under(drive: impl AsRef<Path>) -> Self {
        let d = drive.as_ref();
        let profile = d.join(r"Users\Ana");
        let roaming = profile.join(r"AppData\Roaming");
        let local = profile.join(r"AppData\Local");
        let documents = profile.join("Documents");
        KnownFolders {
            windir: d.join("Windows"),
            program_files: d.join("Program Files"),
            program_files_x86: d.join("Program Files (x86)"),
            program_data: d.join("ProgramData"),
            users: d.join("Users"),
            user_folders: ["Desktop", "Downloads", "Pictures", "Music", "Videos"]
                .iter()
                .map(|f| profile.join(f))
                .chain([documents.clone(), d.join(r"Users\Public")])
                .collect(),
            shortcut_folders: vec![
                profile.join("Desktop"),
                d.join(r"Users\Public\Desktop"),
                roaming.join(r"Microsoft\Windows\Start Menu\Programs"),
                d.join(r"ProgramData\Microsoft\Windows\Start Menu\Programs"),
            ],
            start_menu: vec![
                roaming.join(START_MENU_PROGRAMS),
                d.join("ProgramData").join(START_MENU_PROGRAMS),
            ],
            local_low: profile.join(r"AppData\LocalLow"),
            local_programs: local.join("Programs"),
            user_profile: profile,
            roaming,
            local,
            documents,
        }
    }

    /// Folders of this machine (environment variables and known folders).
    pub fn from_system() -> Self {
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
        let mut f = Self::under(format!(r"{drive}\"));
        let set = |slot: &mut PathBuf, value: Option<PathBuf>| {
            if let Some(v) = value {
                *slot = v;
            }
        };
        set(&mut f.windir, env("SystemRoot").or_else(|| env("windir")));
        set(
            &mut f.program_files,
            env("ProgramW6432").or_else(|| env("ProgramFiles")),
        );
        set(&mut f.program_files_x86, env("ProgramFiles(x86)"));
        set(&mut f.program_data, env("ProgramData"));
        set(&mut f.user_profile, env("USERPROFILE"));
        set(&mut f.roaming, env("APPDATA"));
        set(&mut f.local, env("LOCALAPPDATA"));
        if let Some(parent) = f.user_profile.parent() {
            f.users = parent.to_path_buf();
        }
        f.local_low = f.user_profile.join(r"AppData\LocalLow");
        f.local_programs = f.local.join("Programs");
        f.start_menu = vec![
            f.roaming.join(START_MENU_PROGRAMS),
            f.program_data.join(START_MENU_PROGRAMS),
        ];
        known::fill(&mut f);
        f
    }

    /// Folder roots scanned by EF-05.
    pub fn scan_roots(&self) -> Vec<PathBuf> {
        vec![
            self.program_files.clone(),
            self.program_files_x86.clone(),
            self.roaming.clone(),
            self.local.clone(),
            self.local_programs.clone(),
            self.program_data.clone(),
            self.documents.clone(),
        ]
    }
}

/// Known folders resolved through `SHGetKnownFolderPath`.
mod known {
    use std::path::PathBuf;

    use windows::core::GUID;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads,
        FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Programs, FOLDERID_Public,
        FOLDERID_PublicDesktop, FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };

    use super::KnownFolders;

    fn get(id: &GUID) -> Option<PathBuf> {
        // SAFETY: `id` is a valid GUID; on success the returned buffer is a
        // NUL-terminated string owned by us, freed with CoTaskMemFree below.
        unsafe {
            let raw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.0 as *const _));
            text.map(PathBuf::from)
        }
    }

    pub(super) fn fill(f: &mut KnownFolders) {
        if let Some(docs) = get(&FOLDERID_Documents) {
            f.documents = docs;
        }
        let user = [
            FOLDERID_Desktop,
            FOLDERID_Downloads,
            FOLDERID_Pictures,
            FOLDERID_Music,
            FOLDERID_Videos,
            FOLDERID_Public,
        ];
        f.user_folders = user.iter().filter_map(get).collect();
        f.user_folders.push(f.documents.clone());
        let shortcuts = [
            FOLDERID_Desktop,
            FOLDERID_PublicDesktop,
            FOLDERID_Programs,
            FOLDERID_CommonPrograms,
        ];
        let found: Vec<PathBuf> = shortcuts.iter().filter_map(get).collect();
        if !found.is_empty() {
            f.shortcut_folders = found;
        }
    }
}

/// Lower-cased path text with `\` separators and no trailing separator.
fn norm(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', r"\").to_lowercase();
    text.trim_end_matches('\\').to_owned()
}

/// `true` if `path` equals `root` or lies below it (both normalized).
fn is_within(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!(r"{root}\"))
}

/// The protection rules of one engine.
#[derive(Debug, Clone)]
pub struct SafetyRules {
    trees: Vec<String>,
    exact: Vec<String>,
    users: String,
    exclusions: Vec<String>,
    start_menu: Vec<String>,
    folders: KnownFolders,
}

impl SafetyRules {
    /// Builds the rules for these folders plus the user's exclusions
    /// (folders or registry keys never to propose, EF-20).
    pub fn new(folders: &KnownFolders, exclusions: &[String]) -> Self {
        let mut exact: Vec<String> = [
            &folders.program_files,
            &folders.program_files_x86,
            &folders.program_data,
            &folders.users,
            &folders.user_profile,
            &folders.roaming,
            &folders.local,
            &folders.local_low,
            &folders.local_programs,
            &folders.documents,
        ]
        .iter()
        .map(|p| norm(p))
        .collect();
        exact.extend(folders.user_folders.iter().map(|p| norm(p)));
        exact.extend(folders.shortcut_folders.iter().map(|p| norm(p)));
        exact.push(norm(&folders.user_profile.join("AppData")));
        exact.extend(REGISTRY_SCAN_ROOTS.iter().map(|k| k.to_lowercase()));
        SafetyRules {
            trees: vec![norm(&folders.windir)],
            exact,
            users: norm(&folders.users),
            exclusions: exclusions
                .iter()
                .map(|e| norm(Path::new(e)))
                .filter(|e| !e.is_empty())
                .collect(),
            start_menu: folders.start_menu.iter().map(|p| norm(p)).collect(),
            folders: folders.clone(),
        }
    }

    /// D47, pure part (also before a restore): a task path outside
    /// `\Microsoft\` (any case, anywhere in the path), with no empty, `.`
    /// or `..` segment.
    pub fn check_task_path(&self, path: &str) -> CoreResult<()> {
        let refuse = || CoreError::Protected {
            location: path.to_owned(),
        };
        let rest = path.strip_prefix('\\').ok_or_else(refuse)?;
        let bad = rest.split('\\').any(|part| {
            let name = part.trim_end_matches([' ', '.']).to_lowercase();
            name.is_empty() || part == "." || part == ".." || name == "microsoft"
        });
        if bad {
            return Err(refuse());
        }
        Ok(())
    }

    /// D47, right before acting on a scheduled task: its definition `xml`,
    /// read just now, holds only `Exec` actions, each with a local absolute
    /// program outside `%WINDIR%` that no longer exists. A COM handler, an
    /// e-mail or message action, a relative command or an unresolved
    /// variable refuses the task.
    pub fn validate_task(&self, fs: &dyn FileSystem, path: &str, xml: &str) -> CoreResult<()> {
        self.check_task_path(path)?;
        let refuse = || CoreError::Protected {
            location: path.to_owned(),
        };
        let actions = parse_actions(xml);
        if actions.is_empty() {
            return Err(refuse());
        }
        for action in actions {
            let TaskAction::Exec(command) = action else {
                return Err(refuse());
            };
            let program = program_path(&command, &self.folders).ok_or_else(refuse)?;
            if fs.exists(&program) {
                return Err(refuse());
            }
        }
        Ok(())
    }

    /// D47, pure part: exactly `HKLM\SYSTEM\CurrentControlSet\Services\<name>`
    /// for one well-formed service name; never the `Services` key itself nor
    /// a sub-key of a service.
    pub fn check_service_key(&self, key: &RegKeyPath, name: &str) -> CoreResult<()> {
        let refuse = || CoreError::Protected {
            location: key.to_string(),
        };
        let trimmed = name.trim_end_matches([' ', '.']);
        if trimmed.is_empty() || name.contains(['\\', '/']) || name == "." || name == ".." {
            return Err(refuse());
        }
        if !key
            .to_string()
            .eq_ignore_ascii_case(&format!(r"HKLM\{SERVICES_KEY}\{name}"))
        {
            return Err(refuse());
        }
        Ok(())
    }

    /// D47, right before acting on a service (`state` queried just now):
    /// a stopped Win32 service (never a driver), whose program is a local
    /// absolute path outside `%WINDIR%` that no longer exists, not hosted
    /// by `svchost.exe` nor through a `ServiceDll`, on which no other
    /// service depends, running under a built-in account (it must be
    /// recreated identically, without a password).
    pub fn validate_service(
        &self,
        fs: &dyn FileSystem,
        state: &ServiceState,
        service_dll: Option<&str>,
    ) -> CoreResult<()> {
        let config = &state.config;
        let key = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY).join(&config.name);
        self.check_service_key(&key, &config.name)?;
        let refuse = || CoreError::Protected {
            location: key.to_string(),
        };
        let raw = config.binary_path.to_ascii_lowercase();
        let windows_text = ["svchost.exe", "%systemroot%", "%windir%", r"\systemroot\"]
            .iter()
            .any(|t| raw.contains(t));
        if config.service_type & DRIVER_TYPES != 0
            || config.service_type & WIN32_TYPES == 0
            || state.current_state != STATE_STOPPED
            || windows_text
            || service_dll.is_some()
            || !state.dependents.is_empty()
            || !is_builtin_account(config.account.as_deref())
        {
            return Err(refuse());
        }
        let program = command_executable_raw(&config.binary_path)
            .and_then(|c| program_path(&c, &self.folders))
            .ok_or_else(refuse)?;
        if fs.exists(&program) {
            return Err(refuse());
        }
        Ok(())
    }

    /// Refuses a path with `.` or `..` segments or that is not absolute.
    fn check_shape(path: &Path) -> CoreResult<()> {
        let raw = path.to_string_lossy();
        let bad_segment = raw
            .split(['\\', '/'])
            .any(|segment| segment == "." || segment == "..");
        if !path.is_absolute() || bad_segment {
            return Err(CoreError::Protected {
                location: path.display().to_string(),
            });
        }
        Ok(())
    }

    /// On-disk part of a validation: no reparse point on the way (ENF-03)
    /// and the final path equals the given one (no link escaping its root).
    fn check_on_disk(fs: &dyn FileSystem, path: &Path) -> CoreResult<()> {
        let refuse = || CoreError::Protected {
            location: path.display().to_string(),
        };
        let ancestors: Vec<&Path> = path.ancestors().collect();
        for step in ancestors.iter().rev() {
            if step.parent().is_none() {
                continue; // drive root
            }
            if fs.metadata(step)?.is_reparse_point {
                return Err(refuse());
            }
        }
        let canonical = fs.canonicalize(path)?;
        if norm(&canonical) != norm(path) {
            return Err(refuse());
        }
        Ok(())
    }

    /// Shared part of the Start menu exceptions (D20): `path` lies strictly
    /// below one of the two roots, no component below the root is a
    /// protected name or a folder of Windows tools, and no user exclusion
    /// covers it. Returns the components below the root.
    fn check_below_start_menu(&self, path: &Path) -> CoreResult<Vec<String>> {
        let refuse = || CoreError::Protected {
            location: path.display().to_string(),
        };
        Self::check_shape(path)?;
        let text = norm(path);
        let rest = self
            .start_menu
            .iter()
            .find_map(|root| text.strip_prefix(&format!(r"{root}\")))
            .ok_or_else(refuse)?;
        let parts: Vec<String> = rest.split('\\').map(str::to_owned).collect();
        let forbidden = |p: &String| {
            // Windows ignores trailing spaces and dots in names.
            let p = p.trim_end_matches([' ', '.']);
            p.is_empty() || PROTECTED_NAMES.contains(&p) || WINDOWS_START_MENU_FOLDERS.contains(&p)
        };
        if parts.iter().any(forbidden) || self.exclusions.iter().any(|e| is_within(&text, e)) {
            return Err(refuse());
        }
        Ok(parts)
    }

    /// D20, pure part (also checked before a restore): a `.lnk` file
    /// strictly below a Start menu root.
    pub fn check_start_menu_shortcut(&self, path: &Path) -> CoreResult<()> {
        self.check_below_start_menu(path)?;
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            return Err(CoreError::Protected {
                location: path.display().to_string(),
            });
        }
        Ok(())
    }

    /// D20, right before acting: [`Self::check_start_menu_shortcut`], the
    /// item is a file, and no reparse point lies on the way.
    pub fn validate_start_menu_shortcut(&self, fs: &dyn FileSystem, path: &Path) -> CoreResult<()> {
        self.check_start_menu_shortcut(path)?;
        if fs.metadata(path)?.is_dir {
            return Err(CoreError::Protected {
                location: path.display().to_string(),
            });
        }
        Self::check_on_disk(fs, path)
    }

    /// D20, pure part: a folder strictly below a Start menu root (never a
    /// root itself).
    pub fn check_start_menu_folder(&self, path: &Path) -> CoreResult<()> {
        self.check_below_start_menu(path).map(|_| ())
    }

    /// D20, right before acting: [`Self::check_start_menu_folder`], and the
    /// folder is **empty** (its dead shortcuts already in quarantine).
    pub fn validate_start_menu_folder(&self, fs: &dyn FileSystem, path: &Path) -> CoreResult<()> {
        self.check_start_menu_folder(path)?;
        let refuse = || CoreError::Protected {
            location: path.display().to_string(),
        };
        if !fs.metadata(path)?.is_dir || !fs.read_dir(path)?.is_empty() {
            return Err(refuse());
        }
        Self::check_on_disk(fs, path)
    }

    /// Validation right before acting on a file or folder item (rule 3):
    /// the general rules first; only when they refuse an item lying below a
    /// Start menu root does the single exception matching its kind decide
    /// (D20). Never both relaxed at once.
    pub fn validate_item_path(
        &self,
        fs: &dyn FileSystem,
        kind: LeftoverKind,
        path: &Path,
    ) -> CoreResult<()> {
        let general = self.validate_path(fs, path);
        if general.is_ok() || !self.is_below_start_menu(path) {
            return general;
        }
        match kind {
            LeftoverKind::Shortcut => self.validate_start_menu_shortcut(fs, path),
            LeftoverKind::Folder => self.validate_start_menu_folder(fs, path),
            _ => general,
        }
    }

    /// Pure counterpart of [`Self::validate_item_path`], checked before a
    /// restore (the item is not on disk).
    pub fn check_item_path(&self, kind: LeftoverKind, path: &Path) -> CoreResult<()> {
        let general = self.check_path(path);
        if general.is_err() && kind == LeftoverKind::Folder && self.is_package_folder(path) {
            return self.check_package_folder(path).map(|_| ());
        }
        if general.is_ok() || !self.is_below_start_menu(path) {
            return general;
        }
        match kind {
            LeftoverKind::Shortcut => self.check_start_menu_shortcut(path),
            LeftoverKind::Folder => self.check_start_menu_folder(path),
            _ => general,
        }
    }

    /// D48: a well-formed family name that is neither Windows' (publisher
    /// `cw5n1h2txyewy`) nor Microsoft's (`Microsoft*`, `Windows.*`).
    fn check_family(&self, family: &str, location: &str) -> CoreResult<()> {
        let lower = family.to_ascii_lowercase();
        let ours = lower.starts_with("microsoft")
            || lower.starts_with("windows.")
            || lower.ends_with(&format!("_{WINDOWS_PUBLISHER_ID}"));
        if !is_family_name(family) || ours {
            return Err(CoreError::Protected {
                location: location.to_owned(),
            });
        }
        Ok(())
    }

    /// `true` if `path` is a direct child of `%LOCALAPPDATA%\Packages`.
    pub fn is_package_folder(&self, path: &Path) -> bool {
        path.parent().map(norm) == Some(norm(&self.folders.local.join("Packages")))
    }

    /// D48, pure part (also before a restore): `%LOCALAPPDATA%\Packages\<family>`
    /// exactly (never `Packages` itself nor anything below a package
    /// folder), for an allowed family; returns the family name.
    pub fn check_package_folder(&self, path: &Path) -> CoreResult<String> {
        let refuse = || CoreError::Protected {
            location: path.display().to_string(),
        };
        Self::check_shape(path)?;
        if !self.is_package_folder(path) {
            return Err(refuse());
        }
        let family = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(refuse)?
            .to_owned();
        self.check_family(&family, &path.display().to_string())?;
        if self.exclusions.iter().any(|e| is_within(&norm(path), e)) {
            return Err(refuse());
        }
        Ok(family)
    }

    /// D48, right before acting: [`Self::check_package_folder`], a folder
    /// without reparse point on the way, and its package absent from
    /// `installed` (the packages of the user, listed just now; an empty
    /// list means the listing failed and refuses everything).
    pub fn validate_package_folder(
        &self,
        fs: &dyn FileSystem,
        path: &Path,
        installed: &[String],
    ) -> CoreResult<()> {
        let family = self.check_package_folder(path)?;
        let refuse = || CoreError::Protected {
            location: path.display().to_string(),
        };
        if installed.is_empty() || installed.iter().any(|f| f.eq_ignore_ascii_case(&family)) {
            return Err(refuse());
        }
        if !fs.metadata(path)?.is_dir {
            return Err(refuse());
        }
        Self::check_on_disk(fs, path)
    }

    /// D48, pure part: one of the two [`PACKAGE_KEY_PARENTS`] followed by a
    /// single allowed family name, exactly; returns the family name.
    pub fn check_package_key(&self, key: &RegKeyPath) -> CoreResult<String> {
        let text = key.to_string();
        let refuse = || CoreError::Protected {
            location: text.clone(),
        };
        let family = PACKAGE_KEY_PARENTS
            .iter()
            .find_map(|parent| {
                let head = text.get(..parent.len() + 1)?;
                head.eq_ignore_ascii_case(&format!(r"{parent}\"))
                    .then(|| text[parent.len() + 1..].to_owned())
            })
            .ok_or_else(refuse)?;
        if family.contains('\\') || family.is_empty() {
            return Err(refuse());
        }
        self.check_family(&family, &text)?;
        if self
            .exclusions
            .iter()
            .any(|e| is_within(&text.to_lowercase(), e))
        {
            return Err(refuse());
        }
        Ok(family)
    }

    /// D48, right before acting: [`Self::check_package_key`] and its package
    /// absent from `installed` (an empty list refuses everything).
    pub fn validate_package_key(&self, key: &RegKeyPath, installed: &[String]) -> CoreResult<()> {
        let family = self.check_package_key(key)?;
        if installed.is_empty() || installed.iter().any(|f| f.eq_ignore_ascii_case(&family)) {
            return Err(CoreError::Protected {
                location: key.to_string(),
            });
        }
        Ok(())
    }

    /// `true` if `key` lies directly below one of the [`PACKAGE_KEY_PARENTS`].
    pub fn is_package_key(&self, key: &RegKeyPath) -> bool {
        let text = key.to_string().to_lowercase();
        PACKAGE_KEY_PARENTS.iter().any(|parent| {
            text.strip_prefix(&format!(r"{}\", parent.to_lowercase()))
                .is_some_and(|rest| !rest.is_empty() && !rest.contains('\\'))
        })
    }

    /// `true` if `path` lies strictly below a Start menu root.
    pub fn is_below_start_menu(&self, path: &Path) -> bool {
        let text = norm(path);
        self.start_menu
            .iter()
            .any(|root| text.starts_with(&format!(r"{root}\")))
    }

    /// Checks a path against the blacklist and the exclusions, without
    /// touching the disk.
    pub fn check_path(&self, path: &Path) -> CoreResult<()> {
        let refuse = || CoreError::Protected {
            location: path.display().to_string(),
        };
        if !path.is_absolute() {
            return Err(refuse());
        }
        // `components()` silently drops inner `.` segments: check the raw text.
        let raw = path.to_string_lossy();
        if raw
            .split(['\\', '/'])
            .any(|segment| segment == "." || segment == "..")
        {
            return Err(refuse());
        }
        let mut normal_components = 0;
        for component in path.components() {
            match component {
                Component::CurDir | Component::ParentDir => return Err(refuse()),
                Component::Normal(name) => {
                    normal_components += 1;
                    let name = name.to_string_lossy().to_lowercase();
                    if PROTECTED_NAMES.contains(&name.trim_end()) {
                        return Err(refuse());
                    }
                }
                Component::Prefix(_) | Component::RootDir => {}
            }
        }
        // Drive root (C:\) and anything that is not below it.
        if normal_components == 0 {
            return Err(refuse());
        }
        let text = norm(path);
        if self.trees.iter().any(|t| is_within(&text, t))
            || self.exact.contains(&text)
            || self.exclusions.iter().any(|e| is_within(&text, e))
        {
            return Err(refuse());
        }
        // Every user profile folder (C:\Users\<name>) itself.
        if Path::new(&text).parent().map(norm).as_deref() == Some(self.users.as_str()) {
            return Err(refuse());
        }
        Ok(())
    }

    /// Full validation of a path right before acting on it: blacklist,
    /// exclusions, no reparse point on the way (ENF-03), and the final path
    /// must equal the given one (no link escaping its root).
    pub fn validate_path(&self, fs: &dyn FileSystem, path: &Path) -> CoreResult<()> {
        self.check_path(path)?;
        Self::check_on_disk(fs, path)
    }

    /// Checks a registry key against the blacklist and the exclusions.
    pub fn validate_key(&self, key: &RegKeyPath) -> CoreResult<()> {
        let text = key.to_string().to_lowercase();
        let refuse = || CoreError::Protected {
            location: key.to_string(),
        };
        let parts: Vec<&str> = text.split('\\').collect();
        if parts.len() < 3
            || parts
                .iter()
                .any(|p| p.is_empty() || *p == "." || *p == "..")
        {
            return Err(refuse());
        }
        if PROTECTED_KEY_TREES
            .iter()
            .any(|t| is_within(&text, &t.to_lowercase()))
            || REGISTRY_SCAN_ROOTS.iter().any(|r| text == r.to_lowercase())
            || parts.iter().any(|p| PROTECTED_KEY_NAMES.contains(p))
            || parts.last() == Some(&"wow6432node")
            || self.exclusions.iter().any(|e| is_within(&text, e))
        {
            return Err(refuse());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FakeFileSystem, RealFileSystem};
    use crate::registry::Hive;

    fn rules() -> SafetyRules {
        SafetyRules::new(&KnownFolders::under(r"C:\"), &[])
    }

    fn assert_protected(path: &str) {
        let err = rules().check_path(Path::new(path)).expect_err(path);
        assert_eq!(err.code(), "PROTECTED", "{path}");
    }

    fn assert_allowed(path: &str) {
        rules()
            .check_path(Path::new(path))
            .unwrap_or_else(|e| panic!("{path} refused: {e}"));
    }

    /// One test per blacklist entry (specs § 9.2: « un test unitaire par entrée »).
    macro_rules! protected_paths {
        ($($test:ident => $path:expr,)*) => {$(
            #[test]
            fn $test() {
                assert_protected($path);
            }
        )*};
    }

    protected_paths! {
        windir_itself => r"C:\Windows",
        windir_content => r"C:\Windows\System32\drivers\etc\hosts",
        drive_root => r"C:\",
        other_drive_root => r"D:\",
        program_files => r"C:\Program Files",
        program_files_x86 => r"C:\Program Files (x86)",
        program_data => r"C:\ProgramData",
        users_root => r"C:\Users",
        user_profile => r"C:\Users\Ana",
        other_user_profile => r"C:\Users\Bob",
        appdata => r"C:\Users\Ana\AppData",
        appdata_roaming => r"C:\Users\Ana\AppData\Roaming",
        appdata_local => r"C:\Users\Ana\AppData\Local",
        appdata_local_low => r"C:\Users\Ana\AppData\LocalLow",
        documents => r"C:\Users\Ana\Documents",
        desktop => r"C:\Users\Ana\Desktop",
        downloads => r"C:\Users\Ana\Downloads",
        pictures => r"C:\Users\Ana\Pictures",
        music => r"C:\Users\Ana\Music",
        videos => r"C:\Users\Ana\Videos",
        public => r"C:\Users\Public",
        system_volume_information => r"C:\System Volume Information",
        recycle_bin => r"C:\$Recycle.Bin\S-1-5-21",
        recovery => r"C:\Recovery",
        name_microsoft => r"C:\Users\Ana\AppData\Roaming\Microsoft",
        name_windows => r"C:\Program Files\Windows",
        name_windows_defender => r"C:\ProgramData\Windows Defender",
        name_windows_nt => r"C:\Program Files\Windows NT",
        name_common_files => r"C:\Program Files\Common Files\Vendor",
        name_packages => r"C:\Users\Ana\AppData\Local\Packages\App_123",
        name_windowsapps => r"C:\Program Files\WindowsApps",
        name_temp => r"C:\Users\Ana\AppData\Local\Temp",
        name_microsoft_net => r"C:\Program Files\Microsoft.NET",
        name_dotnet => r"C:\Program Files\dotnet",
        name_intel => r"C:\Program Files\Intel",
        name_nvidia => r"C:\Program Files\NVIDIA Corporation",
        name_amd => r"C:\Program Files\AMD",
        name_package_cache => r"C:\ProgramData\Package Cache",
        name_cleanslate => r"C:\Users\Ana\AppData\Local\CleanSlate",
        name_case_insensitive => r"C:\Users\Ana\AppData\Roaming\MICROSOFT\Teams",
        scan_root_program_files => r"C:\Program Files\",
        scan_root_roaming => r"c:\users\ana\appdata\roaming",
        scan_root_local => r"C:\Users\Ana\AppData\Local",
        scan_root_local_programs => r"C:\Users\Ana\AppData\Local\Programs",
        scan_root_program_data => r"C:\ProgramData",
        scan_root_documents => r"C:\Users\Ana\Documents",
        shortcut_root_start_menu => r"C:\Users\Ana\AppData\Roaming\Microsoft\Windows\Start Menu\Programs",
        parent_dir_in_name => r"C:\Program Files\App\..\..\Windows",
        parent_dir_trailing => r"C:\Program Files\App\..",
        current_dir_in_name => r"C:\Program Files\.\App",
        relative_path => r"Program Files\App",
    }

    #[test]
    fn system_folders_are_absolute_and_protected() {
        // Read-only: environment variables and SHGetKnownFolderPath.
        let folders = KnownFolders::from_system();
        let rules = SafetyRules::new(&folders, &[]);
        for root in folders.scan_roots() {
            assert!(root.is_absolute(), "{}", root.display());
            assert!(rules.check_path(&root).is_err(), "{}", root.display());
        }
        assert!(rules.check_path(&folders.windir.join("System32")).is_err());
        assert!(!folders.shortcut_folders.is_empty());
    }

    #[test]
    fn ordinary_candidates_are_allowed() {
        for path in [
            r"C:\Program Files\Audacity",
            r"C:\Program Files (x86)\Old App\data",
            r"C:\Users\Ana\AppData\Roaming\audacity",
            r"C:\Users\Ana\AppData\Local\Programs\Discord",
            r"C:\ProgramData\VendorX",
            r"C:\Users\Ana\Documents\Audacity",
            r"C:\Users\Ana\Desktop\Old.lnk",
            r"C:\Program Files\Microsoft Office Tools Old",
        ] {
            assert_allowed(path);
        }
    }

    #[test]
    fn user_exclusions_protect_the_folder_and_its_content() {
        let rules = SafetyRules::new(
            &KnownFolders::under(r"C:\"),
            &[
                r"C:\ProgramData\KeepMe".to_owned(),
                r"HKCU\Software\KeepKey".to_owned(),
            ],
        );
        assert!(rules
            .check_path(Path::new(r"C:\ProgramData\KeepMe"))
            .is_err());
        assert!(rules
            .check_path(Path::new(r"c:\programdata\keepme\sub"))
            .is_err());
        assert!(rules
            .check_path(Path::new(r"C:\ProgramData\KeepMeNot"))
            .is_ok());
        let key = RegKeyPath::new(Hive::CurrentUser, r"Software\KeepKey\Sub");
        assert!(rules.validate_key(&key).is_err());
    }

    /// One test per registry blacklist entry.
    macro_rules! protected_keys {
        ($($test:ident => ($hive:expr, $path:expr),)*) => {$(
            #[test]
            fn $test() {
                let key = RegKeyPath::new($hive, $path);
                let err = rules().validate_key(&key).expect_err($path);
                assert_eq!(err.code(), "PROTECTED");
            }
        )*};
    }

    protected_keys! {
        key_hklm_system => (Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Services\X"),
        key_hklm_sam => (Hive::LocalMachine, r"SAM\Domains"),
        key_hklm_security => (Hive::LocalMachine, r"SECURITY\Policy"),
        key_hklm_hardware => (Hive::LocalMachine, r"HARDWARE\DESCRIPTION"),
        key_microsoft => (Hive::LocalMachine, r"SOFTWARE\Microsoft\Office"),
        key_classes => (Hive::CurrentUser, r"Software\Classes\.txt"),
        key_policies => (Hive::LocalMachine, r"SOFTWARE\Policies\Vendor"),
        key_clients => (Hive::LocalMachine, r"SOFTWARE\Clients\Mail"),
        key_registered_applications => (Hive::LocalMachine, r"SOFTWARE\RegisteredApplications"),
        key_wow6432node_itself => (Hive::LocalMachine, r"SOFTWARE\WOW6432Node"),
        key_windows => (Hive::CurrentUser, r"Software\Vendor\Windows"),
        key_uninstall => (Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\7-Zip"),
        key_cleanslate => (Hive::CurrentUser, r"Software\CleanSlate"),
        key_scan_root_hkcu_software => (Hive::CurrentUser, "Software"),
        key_scan_root_hklm_software => (Hive::LocalMachine, "SOFTWARE"),
        key_hive_root => (Hive::LocalMachine, ""),
        key_parent_dir => (Hive::CurrentUser, r"Software\..\Software\X"),
    }

    #[test]
    fn ordinary_keys_are_allowed() {
        for (hive, path) in [
            (Hive::CurrentUser, r"Software\Audacity"),
            (Hive::LocalMachine, r"SOFTWARE\WOW6432Node\OldVendor"),
            (Hive::LocalMachine, r"SOFTWARE\VideoLAN\VLC"),
        ] {
            rules()
                .validate_key(&RegKeyPath::new(hive, path))
                .unwrap_or_else(|e| panic!("{path}: {e}"));
        }
    }

    const USER_MENU: &str = r"C:\Users\Ana\AppData\Roaming\Microsoft\Windows\Start Menu\Programs";
    const COMMON_MENU: &str = r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs";

    /// D20: every allowed shape of the Start menu exception.
    #[test]
    fn start_menu_shortcuts_allowed() {
        for path in [
            format!(r"{USER_MENU}\Old App.lnk"),
            format!(r"{USER_MENU}\Vendor\Old App\Uninstall.LNK"),
            format!(r"{COMMON_MENU}\Old App.lnk"),
            format!(r"{COMMON_MENU}\Startup\Old Agent.lnk"),
            format!(r"{COMMON_MENU}\Microsoft Office Tools\Old.lnk"),
        ] {
            rules()
                .check_start_menu_shortcut(Path::new(&path))
                .unwrap_or_else(|e| panic!("{path}: {e}"));
        }
        rules()
            .check_start_menu_folder(Path::new(&format!(r"{USER_MENU}\Vendor")))
            .unwrap();
    }

    /// D20: each neighbouring case stays refused by the exception, and the
    /// general blacklist still refuses the Start menu.
    #[test]
    fn start_menu_neighbours_refused() {
        let refused_shortcuts = [
            USER_MENU.to_owned(),                // the root
            COMMON_MENU.to_owned(),              // the root
            format!(r"{USER_MENU}\Vendor"),      // a folder name
            format!(r"{USER_MENU}\Old App.url"), // not a .lnk
            format!(r"{USER_MENU}\Old App.exe"), // not a .lnk
            format!(r"{USER_MENU}\desktop.ini"), // not a .lnk
            r"C:\Users\Ana\AppData\Roaming\Microsoft\Windows\Start Menu\Old.lnk".to_owned(), // outside Programs
            r"C:\Users\Ana\AppData\Roaming\Microsoft\Windows\Old.lnk".to_owned(),
            r"C:\Users\Ana\Desktop\Old.lnk".to_owned(), // not the Start menu
            r"C:\Users\Bob\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Old.lnk"
                .to_owned(), // other user
            format!(r"{USER_MENU}\Administrative Tools\Old.lnk"), // Windows tools
            format!(r"{COMMON_MENU}\Windows Tools\Old.lnk"),
            format!(r"{COMMON_MENU}\Accessibility\Old.lnk"),
            format!(r"{COMMON_MENU}\Accessories\Old.lnk"),
            format!(r"{COMMON_MENU}\System Tools\Old.lnk"),
            format!(r"{COMMON_MENU}\Windows PowerShell\Old.lnk"),
            format!(r"{COMMON_MENU}\Maintenance\Old.lnk"),
            format!(r"{COMMON_MENU}\Microsoft\Old.lnk"), // protected name
            format!(r"{COMMON_MENU}\Windows\Old.lnk"),
            format!(r"{COMMON_MENU}\Microsoft.\Old.lnk"), // trailing dot
            format!(r"{USER_MENU}\Vendor\..\..\Old.lnk"), // parent segment
            format!(r"{USER_MENU}\.\Old.lnk"),
            r"Microsoft\Windows\Start Menu\Programs\Old.lnk".to_owned(), // relative
        ];
        for path in &refused_shortcuts {
            let err = rules()
                .check_start_menu_shortcut(Path::new(path))
                .expect_err(path);
            assert_eq!(err.code(), "PROTECTED", "{path}");
            // The general blacklist is unchanged: it still refuses the Start menu.
            if path.contains("Start Menu") {
                assert!(rules().check_path(Path::new(path)).is_err(), "{path}");
            }
        }
        for folder in [
            USER_MENU.to_owned(),
            COMMON_MENU.to_owned(),
            format!(r"{COMMON_MENU}\Administrative Tools"),
            format!(r"{COMMON_MENU}\Microsoft"),
            r"C:\Users\Ana\AppData\Roaming\Microsoft\Windows\Start Menu\Vendor".to_owned(),
        ] {
            assert!(
                rules().check_start_menu_folder(Path::new(&folder)).is_err(),
                "{folder}"
            );
        }
        // A user exclusion still wins over the exception.
        let excluded = SafetyRules::new(
            &KnownFolders::under(r"C:\"),
            &[format!(r"{USER_MENU}\Keep")],
        );
        assert!(excluded
            .check_start_menu_shortcut(Path::new(&format!(r"{USER_MENU}\Keep\a.lnk")))
            .is_err());
    }

    /// D20 on disk: the shortcut must be a file, the folder must be empty,
    /// and no reparse point may lie on the way.
    #[test]
    fn start_menu_validation_on_disk() {
        let mut fs = FakeFileSystem::new();
        let lnk = format!(r"{USER_MENU}\Vendor\Old.lnk");
        let dir_lnk = format!(r"{USER_MENU}\Folder.lnk");
        let empty = format!(r"{USER_MENU}\Empty");
        let linked = format!(r"{USER_MENU}\Linked");
        fs.add_file(&lnk, 10, None)
            .add_dir(&dir_lnk)
            .add_dir(&empty)
            .add_reparse_point(&linked);
        let rules = rules();
        rules
            .validate_start_menu_shortcut(&fs, Path::new(&lnk))
            .unwrap();
        assert!(rules
            .validate_start_menu_shortcut(&fs, Path::new(&dir_lnk))
            .is_err());
        rules
            .validate_start_menu_folder(&fs, Path::new(&empty))
            .unwrap();
        // Not empty: still holds Old.lnk.
        assert!(rules
            .validate_start_menu_folder(&fs, Path::new(&format!(r"{USER_MENU}\Vendor")))
            .is_err());
        assert!(rules
            .validate_start_menu_folder(&fs, Path::new(&linked))
            .is_err());
        // Missing shortcut: cannot be validated.
        assert!(rules
            .validate_start_menu_shortcut(&fs, Path::new(&format!(r"{USER_MENU}\Gone.lnk")))
            .is_err());

        // The dispatcher applies the exception of the item's kind only.
        rules
            .validate_item_path(&fs, LeftoverKind::Shortcut, Path::new(&lnk))
            .unwrap();
        rules
            .validate_item_path(&fs, LeftoverKind::Folder, Path::new(&empty))
            .unwrap();
        for kind in [
            LeftoverKind::File,
            LeftoverKind::Folder,
            LeftoverKind::RegistryKey,
        ] {
            assert!(rules
                .validate_item_path(&fs, kind, Path::new(&lnk))
                .is_err());
        }
        assert!(rules
            .validate_item_path(&fs, LeftoverKind::Shortcut, Path::new(&empty))
            .is_err());
        assert!(rules
            .check_item_path(LeftoverKind::Shortcut, Path::new(&lnk))
            .is_ok());
        assert!(rules
            .check_item_path(LeftoverKind::File, Path::new(&lnk))
            .is_err());
        // Outside the Start menu, the general rules alone decide.
        assert!(rules
            .check_item_path(
                LeftoverKind::Shortcut,
                Path::new(r"C:\Users\Ana\AppData\Roaming\Microsoft\x.lnk")
            )
            .is_err());
        assert!(rules
            .check_item_path(
                LeftoverKind::Shortcut,
                Path::new(r"C:\Users\Ana\Desktop\x.lnk")
            )
            .is_ok());
    }

    fn exec(command: &str) -> String {
        format!("<Task><Actions><Exec><Command>{command}</Command></Exec></Actions></Task>")
    }

    /// D47 tasks: the allowed shapes.
    #[test]
    fn dead_tasks_allowed() {
        let fs = FakeFileSystem::new();
        for (path, xml) in [
            (r"\OldUpdater", exec(r"C:\Program Files\Gone\upd.exe")),
            (r"\Vendor\Updater", exec(r#""%ProgramFiles%\Gone\upd.exe""#)),
            (
                r"\Vendor\Two",
                "<Task><Actions><Exec><Command>C:\\Gone\\a.exe</Command></Exec>\
                     <Exec><Command>C:\\Gone\\b.exe</Command></Exec></Actions></Task>"
                    .to_owned(),
            ),
            (r"\Microsoft Office Old\Updater", exec(r"C:\Gone\upd.exe")),
        ] {
            rules()
                .validate_task(&fs, path, &xml)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
        }
    }

    /// D47 tasks: each neighbouring case stays refused.
    #[test]
    fn task_neighbours_refused() {
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Program Files\Alive\a.exe", 1, None);
        let dead = exec(r"C:\Gone\upd.exe");
        let cases = [
            (r"\Microsoft\Windows\Defrag\ScheduledDefrag", dead.clone()),
            (r"\microsoft\Office\Old", dead.clone()),
            (r"\Vendor\MICROSOFT\Old", dead.clone()),
            (r"\Vendor\Microsoft.\Old", dead.clone()),
            (r"Vendor\Old", dead.clone()),
            (r"\Vendor\..\Microsoft\Old", dead.clone()),
            (r"\Vendor\\Old", dead.clone()),
            (r"\", dead.clone()),
            (
                r"\Vendor\Com",
                "<Task><Actions><ComHandler><ClassId>{1}</ClassId></ComHandler></Actions></Task>"
                    .to_owned(),
            ),
            (
                r"\Vendor\Mixed",
                "<Task><Actions><Exec><Command>C:\\Gone\\a.exe</Command></Exec>\
                 <ComHandler><ClassId>{1}</ClassId></ComHandler></Actions></Task>"
                    .to_owned(),
            ),
            (r"\Vendor\Mail", "<Task><Actions><SendEmail /></Actions></Task>".to_owned()),
            (r"\Vendor\NoAction", "<Task />".to_owned()),
            (r"\Vendor\Alive", exec(r"C:\Program Files\Alive\a.exe")),
            (
                r"\Vendor\HalfAlive",
                "<Task><Actions><Exec><Command>C:\\Gone\\a.exe</Command></Exec>\
                     <Exec><Command>C:\\Program Files\\Alive\\a.exe</Command></Exec></Actions></Task>".to_owned(),
            ),
            (r"\Vendor\Windir", exec(r"C:\Windows\System32\gone.exe")),
            (r"\Vendor\WindirVar", exec(r"%windir%\gone.exe")),
            (r"\Vendor\Relative", exec("gone.exe")),
            (r"\Vendor\Unknown", exec(r"%TOOLS%\gone.exe")),
            (r"\Vendor\Network", exec(r"\\server\share\gone.exe")),
        ];
        for (path, xml) in &cases {
            let err = rules().validate_task(&fs, path, xml).expect_err(path);
            assert_eq!(err.code(), "PROTECTED", "{path}");
        }
    }

    fn service(name: &str, binary: &str) -> ServiceState {
        ServiceState {
            config: crate::services::ServiceConfig {
                name: name.into(),
                display_name: name.into(),
                service_type: 0x10,
                start_type: 2,
                binary_path: binary.into(),
                account: Some(r"NT AUTHORITY\LocalService".into()),
                ..Default::default()
            },
            current_state: STATE_STOPPED,
            dependents: Vec::new(),
        }
    }

    /// D47 services: the allowed shapes.
    #[test]
    fn dead_services_allowed() {
        let fs = FakeFileSystem::new();
        for state in [
            service("OldSvc", r#""C:\Program Files\Gone\svc.exe" -run"#),
            service("OldSvc2", r"C:\Program Files\Old App\svc.exe /x"),
            ServiceState {
                config: crate::services::ServiceConfig {
                    service_type: 0x20,
                    account: None,
                    ..service("Shared", r"C:\Gone\shared.exe").config
                },
                ..service("Shared", r"C:\Gone\shared.exe")
            },
        ] {
            rules()
                .validate_service(&fs, &state, None)
                .unwrap_or_else(|e| panic!("{}: {e}", state.config.name));
        }
        let key = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY).join("OldSvc");
        rules().check_service_key(&key, "OldSvc").unwrap();
        rules()
            .check_service_key(&key, "oldsvc")
            .expect("case-insensitive");
    }

    /// D47 services: each neighbouring case stays refused, and the general
    /// blacklist still refuses `HKLM\SYSTEM`.
    #[test]
    fn service_neighbours_refused() {
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Program Files\Alive\svc.exe", 1, None);
        let base = || service("OldSvc", r"C:\Gone\svc.exe");
        let with = |f: &dyn Fn(&mut ServiceState)| {
            let mut s = base();
            f(&mut s);
            s
        };
        let cases: Vec<(&str, ServiceState, Option<&str>)> = vec![
            (
                "kernel driver",
                with(&|s| s.config.service_type = 0x1),
                None,
            ),
            (
                "file system driver",
                with(&|s| s.config.service_type = 0x2),
                None,
            ),
            (
                "driver + win32",
                with(&|s| s.config.service_type = 0x11),
                None,
            ),
            ("not win32", with(&|s| s.config.service_type = 0x100), None),
            ("running", with(&|s| s.current_state = 4), None),
            ("stop pending", with(&|s| s.current_state = 3), None),
            (
                "svchost",
                with(&|s| s.config.binary_path = r"C:\Gone\svchost.exe -k x".into()),
                None,
            ),
            (
                "systemroot text",
                with(&|s| s.config.binary_path = r"%SystemRoot%\gone.exe".into()),
                None,
            ),
            (
                "windir",
                with(&|s| s.config.binary_path = r"C:\Windows\System32\gone.exe".into()),
                None,
            ),
            (
                "\\SystemRoot\\",
                with(&|s| s.config.binary_path = r"\SystemRoot\gone.exe".into()),
                None,
            ),
            ("service dll", base(), Some(r"C:\Gone\svc.dll")),
            (
                "dependent",
                with(&|s| s.dependents = vec!["Other".into()]),
                None,
            ),
            (
                "user account",
                with(&|s| s.config.account = Some(r".\Ana".into())),
                None,
            ),
            (
                "present program",
                with(&|s| s.config.binary_path = r"C:\Program Files\Alive\svc.exe".into()),
                None,
            ),
            (
                "relative program",
                with(&|s| s.config.binary_path = "svc.exe".into()),
                None,
            ),
            (
                "unknown variable",
                with(&|s| s.config.binary_path = r"%TOOLS%\svc.exe".into()),
                None,
            ),
            (
                "name with separator",
                with(&|s| s.config.name = r"Old\Sub".into()),
                None,
            ),
            ("empty name", with(&|s| s.config.name = String::new()), None),
            ("dot name", with(&|s| s.config.name = "..".into()), None),
        ];
        for (case, state, dll) in &cases {
            let err = rules().validate_service(&fs, state, *dll).expect_err(case);
            assert_eq!(err.code(), "PROTECTED", "{case}");
        }
        let services = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY);
        for (key, name) in [
            (services.clone(), "OldSvc"),                    // Services itself
            (services.join(r"OldSvc\Parameters"), "OldSvc"), // a sub-key
            (services.join("Other"), "OldSvc"),              // another service
            (
                RegKeyPath::new(Hive::LocalMachine, r"SYSTEM\ControlSet001\Services\OldSvc"),
                "OldSvc",
            ),
            (
                RegKeyPath::new(
                    Hive::CurrentUser,
                    r"SYSTEM\CurrentControlSet\Services\OldSvc",
                ),
                "OldSvc",
            ),
            (services.join(".."), ".."),
        ] {
            assert!(rules().check_service_key(&key, name).is_err(), "{key}");
        }
        let key = services.join("OldSvc");
        assert!(
            rules().validate_key(&key).is_err(),
            "general rule unchanged"
        );
    }

    const PACKAGES: &str = r"C:\Users\Ana\AppData\Local\Packages";
    const STORAGE: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Storage";
    const SYSTEM_APP_DATA: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\SystemAppData";

    fn installed() -> Vec<String> {
        vec!["Vendor.Kept_abcdefghjk123".into()]
    }

    /// D48: the allowed shapes (folder and both keys of a removed package).
    #[test]
    fn store_leftovers_allowed() {
        let mut fs = FakeFileSystem::new();
        let folder = format!(r"{PACKAGES}\Vendor.Gone_abcdefghjk123");
        fs.add_file(format!(r"{folder}\LocalState\settings.dat"), 10, None);
        let rules = rules();
        rules
            .validate_package_folder(&fs, Path::new(&folder), &installed())
            .unwrap();
        assert_eq!(
            rules.check_package_folder(Path::new(&folder)).unwrap(),
            "Vendor.Gone_abcdefghjk123"
        );
        for parent in [STORAGE, SYSTEM_APP_DATA] {
            let key = RegKeyPath::new(
                Hive::CurrentUser,
                format!(r"{parent}\vendor.gone_abcdefghjk123"),
            );
            rules.validate_package_key(&key, &installed()).unwrap();
            assert!(rules.is_package_key(&key));
        }
        // The general blacklist still refuses all of them.
        assert!(rules.check_path(Path::new(&folder)).is_err());
        let key = RegKeyPath::new(
            Hive::CurrentUser,
            format!(r"{STORAGE}\vendor.gone_abcdefghjk123"),
        );
        assert!(rules.validate_key(&key).is_err());
    }

    /// D48: each neighbouring case stays refused.
    #[test]
    fn store_neighbours_refused() {
        let mut fs = FakeFileSystem::new();
        let gone = format!(r"{PACKAGES}\Vendor.Gone_abcdefghjk123");
        fs.add_dir(&gone)
            .add_dir(format!(r"{PACKAGES}\Vendor.Kept_abcdefghjk123"))
            .add_file(format!(r"{PACKAGES}\Vendor.File_abcdefghjk123"), 1, None)
            .add_reparse_point(format!(r"{PACKAGES}\Vendor.Link_abcdefghjk123"));
        let rules = rules();
        let folders = [
            PACKAGES.to_owned(),                              // Packages itself
            format!(r"{gone}\LocalState"),                    // below a package
            format!(r"{PACKAGES}\Vendor.Kept_abcdefghjk123"), // installed
            format!(r"{PACKAGES}\vendor.kept_abcdefghjk123"), // installed, other case
            format!(r"{PACKAGES}\Microsoft.WindowsCalculator_8wekyb3d8bbwe"), // Microsoft
            format!(r"{PACKAGES}\MicrosoftWindows.Client.CBS_cw5n1h2txyewy"),
            format!(r"{PACKAGES}\Windows.PrintDialog_cw5n1h2txyewy"),
            format!(r"{PACKAGES}\1527c705-839a-4832-9118-54d4Bd6a0c89_cw5n1h2txyewy"), // Windows publisher
            format!(r"{PACKAGES}\NotAFamily"),                                         // malformed
            format!(r"{PACKAGES}\Vendor.Bad_ABCDEFGHJK123"),
            format!(r"{PACKAGES}\Vendor.File_abcdefghjk123"), // a file
            format!(r"{PACKAGES}\Vendor.Link_abcdefghjk123"), // reparse point
            format!(r"{PACKAGES}\..\Packages\Vendor.Gone_abcdefghjk123"),
            r"C:\Users\Bob\AppData\Local\Packages\Vendor.Gone_abcdefghjk123".to_owned(), // other user
            r"C:\Users\Ana\AppData\Roaming\Packages\Vendor.Gone_abcdefghjk123".to_owned(),
        ];
        for folder in &folders {
            assert!(
                rules
                    .validate_package_folder(&fs, Path::new(folder), &installed())
                    .is_err(),
                "{folder}"
            );
        }
        // An empty list of installed packages (listing failed) refuses all.
        assert!(rules
            .validate_package_folder(&fs, Path::new(&gone), &[])
            .is_err());

        let keys = [
            RegKeyPath::new(Hive::CurrentUser, STORAGE), // the parent
            RegKeyPath::new(Hive::CurrentUser, SYSTEM_APP_DATA),
            RegKeyPath::new(
                Hive::CurrentUser,
                format!(r"{STORAGE}\vendor.gone_abcdefghjk123\Sub"),
            ),
            RegKeyPath::new(
                Hive::CurrentUser,
                format!(r"{STORAGE}\vendor.kept_abcdefghjk123"),
            ),
            RegKeyPath::new(
                Hive::CurrentUser,
                format!(r"{STORAGE}\microsoft.windowscalculator_8wekyb3d8bbwe"),
            ),
            RegKeyPath::new(Hive::CurrentUser, format!(r"{STORAGE}\notafamily")),
            RegKeyPath::new(
                Hive::LocalMachine,
                format!(r"{STORAGE}\vendor.gone_abcdefghjk123"),
            ),
            RegKeyPath::new(
                Hive::CurrentUser,
                r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Mappings\vendor.gone_abcdefghjk123",
            ),
        ];
        for key in &keys {
            assert!(
                rules.validate_package_key(key, &installed()).is_err(),
                "{key}"
            );
        }
        let gone_key = RegKeyPath::new(
            Hive::CurrentUser,
            format!(r"{STORAGE}\vendor.gone_abcdefghjk123"),
        );
        assert!(rules.validate_package_key(&gone_key, &[]).is_err());

        // A user exclusion wins over the exception.
        let excluded = SafetyRules::new(&KnownFolders::under(r"C:\"), std::slice::from_ref(&gone));
        assert!(excluded
            .validate_package_folder(&fs, Path::new(&gone), &installed())
            .is_err());
    }

    #[test]
    fn reparse_point_on_the_way_is_refused() {
        let mut fs = FakeFileSystem::new();
        fs.add_reparse_point(r"C:\ProgramData\Link")
            .add_dir(r"C:\ProgramData\Real\Data");
        let rules = rules();
        assert_eq!(
            rules
                .validate_path(&fs, Path::new(r"C:\ProgramData\Link"))
                .unwrap_err()
                .code(),
            "PROTECTED"
        );
        assert!(rules
            .validate_path(&fs, Path::new(r"C:\ProgramData\Real\Data"))
            .is_ok());
    }

    #[test]
    fn missing_path_cannot_be_validated() {
        let fs = FakeFileSystem::new();
        assert_eq!(
            rules()
                .validate_path(&fs, Path::new(r"C:\ProgramData\Gone"))
                .unwrap_err()
                .code(),
            "NOT_FOUND"
        );
    }

    /// Trapped junction to C:\Windows inside a temporary folder (ENF-03).
    #[test]
    fn trapped_junction_to_windows_is_refused() {
        // %TEMP% contains a protected name (« Temp »): use core/target instead.
        let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"..\target\safety-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let base = dir
            .path()
            .canonicalize()
            .map(|p| crate::fs::strip_verbatim(&p))
            .unwrap();
        let link = base.join("trap");
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&windir)
            .output()
            .unwrap();
        assert!(status.status.success(), "mklink failed");

        let rules = SafetyRules::new(&KnownFolders::under(r"C:\"), &[]);
        let real = RealFileSystem;
        for target in [link.clone(), link.join("System32")] {
            let err = rules.validate_path(&real, &target).unwrap_err();
            assert_eq!(err.code(), "PROTECTED", "{}", target.display());
        }
        // A plain folder next to it stays allowed.
        std::fs::create_dir(base.join("plain")).unwrap();
        assert!(rules.validate_path(&real, &base.join("plain")).is_ok());

        // Remove the junction itself (never its target) before the temp dir.
        std::fs::remove_dir(&link).unwrap();
        assert!(std::path::Path::new(&windir).join("System32").exists());
    }
}
