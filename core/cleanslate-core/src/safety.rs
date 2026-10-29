//! Safety rules: the last line of defence (specs § 9, ENF-01, ENF-03).
//!
//! Every destructive action must call [`SafetyRules::validate_path`] or
//! [`SafetyRules::validate_key`] right before acting, even if the scan already
//! filtered the target. The blacklist is hard-coded and cannot be disabled;
//! user exclusions can only add protections, never remove one.

use std::path::{Component, Path, PathBuf};

use crate::error::{CoreError, CoreResult};
use crate::fs::FileSystem;
use crate::registry::RegKeyPath;

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
        }
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
