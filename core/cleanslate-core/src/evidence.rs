//! Signs that a program folder is still in use (task 3 of the owner, D78):
//! working shortcuts, Windows Installer components, App Paths and file
//! associations, package managers, recent launches. **Read-only**: gathered
//! once before a scan through the injected file system and registry; the
//! score of each candidate then lowers its confidence when a sign points at it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Duration, TimeZone, Utc};

use crate::fs::FileSystem;
use crate::matcher::normalize;
use crate::registry::{Hive, KeyView, RegData, RegKeyPath, Registry};
use crate::safety::KnownFolders;
use crate::shortcut;

/// Values of Windows Installer components read at most: the scan stays fast.
const MAX_INSTALLER_VALUES: usize = 300_000;

/// Depth of the shortcut folders walked.
const SHORTCUT_DEPTH: usize = 4;

/// Longest shortcut file read.
const MAX_SHORTCUT_LEN: u64 = 1 << 20;

/// What still points at a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A shortcut of the Start menu or the desktop whose target exists.
    Shortcut,
    /// A component installed by Windows Installer.
    Installer,
    /// An App Paths entry or a file association.
    Registration,
}

/// The signs gathered before a scan.
#[derive(Debug, Default, Clone)]
pub struct Evidence {
    /// Lower-case folders (no trailing `\`) and what points there.
    referenced: Vec<(String, Source)>,
    /// Normalized names of packages and their manager.
    packages: HashMap<String, &'static str>,
    /// Lower-case executable names and their last known launch.
    launched: HashMap<String, DateTime<Utc>>,
}

impl Evidence {
    /// Reads every sign, stopping early when `cancel` is set.
    pub fn gather(
        fs: &dyn FileSystem,
        registry: &dyn Registry,
        folders: &KnownFolders,
        cancel: &AtomicBool,
    ) -> Self {
        let mut evidence = Evidence::default();
        evidence.shortcuts(fs, folders, cancel);
        evidence.installer(registry, cancel);
        evidence.registrations(registry);
        evidence.package_managers(fs, folders);
        evidence.prefetch(fs, folders);
        evidence.user_assist(registry);
        tracing::info!(
            referenced = evidence.referenced.len(),
            packages = evidence.packages.len(),
            launched = evidence.launched.len(),
            "usage evidence gathered"
        );
        evidence
    }

    /// What points into `folder` (itself or below), if anything.
    pub fn referenced_in(&self, folder: &str) -> Option<Source> {
        let folder = folder.trim_end_matches('\\').to_lowercase();
        let below = format!("{folder}\\");
        self.referenced
            .iter()
            .find(|(path, _)| *path == folder || path.starts_with(&below))
            .map(|(_, source)| *source)
    }

    /// Package manager that installed a program of this name, if any.
    pub fn package_of(&self, name: &str) -> Option<&'static str> {
        let key = normalize(name);
        (!key.is_empty())
            .then(|| self.packages.get(&key).copied())
            .flatten()
    }

    /// Last launch of one of these executables (names or paths).
    pub fn last_launch(&self, executables: &[String]) -> Option<DateTime<Utc>> {
        executables
            .iter()
            .filter_map(|e| file_name(e).and_then(|n| self.launched.get(&n)))
            .max()
            .copied()
    }

    /// Adds a referenced folder (tests and gatherers).
    pub fn add_reference(&mut self, folder: &str, source: Source) {
        let folder = folder.trim_end_matches('\\').to_lowercase();
        if !folder.is_empty() {
            self.referenced.push((folder, source));
        }
    }

    /// Adds a package (tests and gatherers).
    pub fn add_package(&mut self, name: &str, manager: &'static str) {
        let key = normalize(name);
        if !key.is_empty() {
            self.packages.insert(key, manager);
        }
    }

    /// Adds a launch (tests and gatherers); keeps the most recent.
    pub fn add_launch(&mut self, executable: &str, when: DateTime<Utc>) {
        if let Some(name) = file_name(executable) {
            let entry = self.launched.entry(name).or_insert(when);
            if when > *entry {
                *entry = when;
            }
        }
    }

    // Shortcuts of the desktops and Start menus whose target still exists: the
    // folder of the target is in use.
    fn shortcuts(&mut self, fs: &dyn FileSystem, folders: &KnownFolders, cancel: &AtomicBool) {
        let mut stack: Vec<(PathBuf, usize)> = folders
            .shortcut_folders
            .iter()
            .chain(&folders.start_menu)
            .map(|f| (f.clone(), 0))
            .collect();
        while let Some((folder, depth)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Ok(entries) = fs.read_dir(&folder) else {
                continue;
            };
            for entry in entries {
                if entry.metadata.is_reparse_point {
                    continue;
                }
                if entry.metadata.is_dir {
                    if depth < SHORTCUT_DEPTH {
                        stack.push((entry.path, depth + 1));
                    }
                    continue;
                }
                let is_lnk = entry
                    .path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
                let target = is_lnk
                    .then(|| fs.read_file(&entry.path, MAX_SHORTCUT_LEN).ok())
                    .flatten()
                    .and_then(|bytes| shortcut::parse(&bytes))
                    .and_then(|info| shortcut::live_target(&info, fs, folders));
                if let Some(parent) = target.as_deref().and_then(|t| Path::new(t).parent()) {
                    self.add_reference(&parent.to_string_lossy(), Source::Shortcut);
                }
            }
        }
    }

    // Windows Installer: every component lists the paths of its files.
    fn installer(&mut self, registry: &dyn Registry, cancel: &AtomicBool) {
        let root = RegKeyPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData",
        )
        .with_view(KeyView::Force64);
        let mut read = 0;
        for sid in registry.subkeys(&root).unwrap_or_default() {
            let components = root.join(&sid).join("Components");
            for component in registry.subkeys(&components).unwrap_or_default() {
                if cancel.load(Ordering::Relaxed) || read >= MAX_INSTALLER_VALUES {
                    return;
                }
                for (_, data) in registry
                    .values(&components.join(&component))
                    .unwrap_or_default()
                {
                    read += 1;
                    if let RegData::String(path) | RegData::ExpandString(path) = data {
                        if let Some(folder) = local_folder(&path) {
                            self.add_reference(&folder, Source::Installer);
                        }
                    }
                }
            }
        }
    }

    // App Paths (« monprog.exe » known to Windows) and the commands that open files.
    fn registrations(&mut self, registry: &dyn Registry) {
        for hive in [Hive::LocalMachine, Hive::CurrentUser] {
            let base = if hive == Hive::LocalMachine {
                "SOFTWARE"
            } else {
                "Software"
            };
            let app_paths = RegKeyPath::new(
                hive,
                format!(r"{base}\Microsoft\Windows\CurrentVersion\App Paths"),
            );
            for exe in registry.subkeys(&app_paths).unwrap_or_default() {
                for (name, data) in registry.values(&app_paths.join(&exe)).unwrap_or_default() {
                    if let RegData::String(text) | RegData::ExpandString(text) = data {
                        let folder = if name.eq_ignore_ascii_case("Path") {
                            Some(text.trim_end_matches(';').trim_matches('"').to_owned())
                        } else if name.is_empty() {
                            command_exe(&text).and_then(|e| local_folder(&e))
                        } else {
                            None
                        };
                        if let Some(folder) = folder.filter(|f| has_drive(f)) {
                            self.add_reference(&folder, Source::Registration);
                        }
                    }
                }
            }
            let applications = RegKeyPath::new(hive, format!(r"{base}\Classes\Applications"));
            for exe in registry.subkeys(&applications).unwrap_or_default() {
                let command = applications.join(&exe).join(r"shell\open\command");
                for (name, data) in registry.values(&command).unwrap_or_default() {
                    if let (true, RegData::String(text) | RegData::ExpandString(text)) =
                        (name.is_empty(), data)
                    {
                        if let Some(folder) = command_exe(&text).and_then(|e| local_folder(&e)) {
                            self.add_reference(&folder, Source::Registration);
                        }
                    }
                }
            }
        }
    }

    // Scoop, Chocolatey and winget keep one folder per installed package.
    fn package_managers(&mut self, fs: &dyn FileSystem, folders: &KnownFolders) {
        let managers: [(PathBuf, &'static str); 3] = [
            (folders.user_profile.join(r"scoop\apps"), "Scoop"),
            (folders.program_data.join(r"chocolatey\lib"), "Chocolatey"),
            (folders.local.join(r"Microsoft\WinGet\Packages"), "winget"),
        ];
        for (root, manager) in managers {
            for entry in fs.read_dir(&root).unwrap_or_default() {
                if !entry.metadata.is_dir || entry.metadata.is_reparse_point {
                    continue;
                }
                let name = entry
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if name.eq_ignore_ascii_case("scoop") {
                    continue;
                }
                self.add_package(&package_name(&name), manager);
                self.add_reference(&entry.path.to_string_lossy(), Source::Installer);
            }
        }
    }

    // Prefetch: one file per program launched, « NAME.EXE-1A2B3C4D.pf », dated
    // by the last launch. Readable with administrator rights only.
    fn prefetch(&mut self, fs: &dyn FileSystem, folders: &KnownFolders) {
        for entry in fs
            .read_dir(&folders.windir.join("Prefetch"))
            .unwrap_or_default()
        {
            let name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let (Some(exe), Some(when)) = (prefetch_exe(&name), entry.metadata.modified) {
                self.add_launch(&exe, when);
            }
        }
    }

    // UserAssist: programs launched from the shell, names in ROT13, last launch in
    // the data (Windows 7 and later: FILETIME at offset 60 of 72 bytes).
    fn user_assist(&mut self, registry: &dyn Registry) {
        let root = RegKeyPath::new(
            Hive::CurrentUser,
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist",
        );
        for guid in registry.subkeys(&root).unwrap_or_default() {
            for (name, data) in registry
                .values(&root.join(&guid).join("Count"))
                .unwrap_or_default()
            {
                let RegData::Binary(bytes) = data else {
                    continue;
                };
                if let Some(when) = user_assist_time(&bytes) {
                    let decoded = rot13(&name);
                    if decoded.to_lowercase().ends_with(".exe") {
                        self.add_launch(&decoded, when);
                    }
                }
            }
        }
    }
}

/// ROT13 of the letters (UserAssist value names).
pub fn rot13(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => c,
        })
        .collect()
}

/// Last launch stored in a UserAssist entry of Windows 7 and later.
pub fn user_assist_time(bytes: &[u8]) -> Option<DateTime<Utc>> {
    let raw = u64::from_le_bytes(bytes.get(60..68)?.try_into().ok()?);
    if raw == 0 {
        return None;
    }
    // FILETIME: 100 ns since 1601-01-01.
    let seconds = i64::try_from(raw / 10_000_000).ok()? - 11_644_473_600;
    Utc.timestamp_opt(seconds, 0).single()
}

/// Executable of a command line: a quoted path, or the text up to `.exe`.
pub fn command_exe(command: &str) -> Option<String> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        return rest
            .split('"')
            .next()
            .map(str::to_owned)
            .filter(|e| !e.is_empty());
    }
    let lower = command.to_lowercase();
    lower.find(".exe").map(|end| command[..end + 4].to_owned())
}

/// Executable of a Prefetch file name, « NOTEPAD.EXE-D8414F97.pf » → « notepad.exe ».
pub fn prefetch_exe(file: &str) -> Option<String> {
    let lower = file.to_lowercase();
    let stem = lower.strip_suffix(".pf")?;
    let (exe, hash) = stem.rsplit_once('-')?;
    (exe.ends_with(".exe") && hash.len() == 8).then(|| exe.to_owned())
}

/// Program of a winget folder, « Publisher.App_Microsoft.Winget.Source_8wekyb3d8bbwe »
/// → « App »; other managers: the folder name itself.
pub fn package_name(folder: &str) -> String {
    let id = folder.split('_').next().unwrap_or(folder);
    id.rsplit('.').next().unwrap_or(id).to_owned()
}

/// True when the age of a launch makes it « recent » (D78: 30 days).
pub fn is_recent(launch: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now - launch < Duration::days(30)
}

// Folder of a local file path (« C:\X\a.dll » → « C:\X »); registry paths
// of components (« 02:\… ») and network paths are left out.
fn local_folder(path: &str) -> Option<String> {
    let path = path.trim().trim_matches('"');
    has_drive(path)
        .then(|| {
            Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .flatten()
        .filter(|p| p.len() > 3)
}

fn has_drive(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\'
}

// Lower-case file name of a path or a name.
fn file_name(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?.trim().to_lowercase();
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use crate::registry::TestRegistryKey;

    #[test]
    fn decoders_of_the_windows_traces() {
        assert_eq!(
            rot13("P:\\Cebtenz Svyrf\\Gbby\\gbby.rkr"),
            "C:\\Program Files\\Tool\\tool.exe"
        );
        assert_eq!(
            prefetch_exe("NOTEPAD.EXE-D8414F97.pf").as_deref(),
            Some("notepad.exe")
        );
        assert_eq!(prefetch_exe("Layout.ini"), None);
        assert_eq!(
            command_exe(r#""C:\Tools\Viewer\view.exe" "%1""#).as_deref(),
            Some(r"C:\Tools\Viewer\view.exe")
        );
        assert_eq!(
            command_exe(r"C:\Tools\Viewer\view.exe %1").as_deref(),
            Some(r"C:\Tools\Viewer\view.exe")
        );
        assert_eq!(
            package_name("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe"),
            "FFmpeg"
        );
        assert_eq!(package_name("7zip"), "7zip");

        let mut data = vec![0u8; 72];
        // 2026-10-01T00:00:00Z as a FILETIME.
        let filetime: u64 = (1_790_812_800 + 11_644_473_600) * 10_000_000;
        data[60..68].copy_from_slice(&filetime.to_le_bytes());
        assert_eq!(
            user_assist_time(&data),
            Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).single()
        );
        assert_eq!(user_assist_time(&[0; 16]), None);
    }

    #[test]
    fn a_reference_covers_the_folder_and_what_is_below() {
        let mut evidence = Evidence::default();
        evidence.add_reference(r"C:\Users\Ana\AppData\Local\Tool\bin", Source::Shortcut);
        assert_eq!(
            evidence.referenced_in(r"C:\Users\Ana\AppData\Local\Tool"),
            Some(Source::Shortcut)
        );
        assert_eq!(
            evidence.referenced_in(r"C:\Users\Ana\AppData\Local\Tool\bin\"),
            Some(Source::Shortcut)
        );
        assert_eq!(
            evidence.referenced_in(r"C:\Users\Ana\AppData\Local\Tools"),
            None
        );
        assert_eq!(
            evidence.referenced_in(r"C:\Users\Ana\AppData\Local\Tool\bin\old"),
            None
        );
    }

    #[test]
    fn package_managers_and_prefetch_are_read_from_their_folders() {
        let base = Path::new(r"C:\");
        let folders = KnownFolders::under(base);
        let when = Utc
            .with_ymd_and_hms(2026, 10, 30, 8, 0, 0)
            .single()
            .unwrap();
        let mut fs = FakeFileSystem::new();
        fs.add_dir(folders.user_profile.join(r"scoop\apps\scoop"))
            .add_dir(folders.user_profile.join(r"scoop\apps\ripgrep"))
            .add_dir(folders.program_data.join(r"chocolatey\lib\notepadplusplus"))
            .add_dir(folders.local.join(
                r"Microsoft\WinGet\Packages\Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe",
            ))
            .add_file(
                folders.windir.join(r"Prefetch\TOOL.EXE-0A1B2C3D.pf"),
                10,
                Some(when),
            );
        let area = TestRegistryKey::new();

        let evidence = Evidence::gather(&fs, &area.registry(), &folders, &AtomicBool::new(false));

        assert_eq!(evidence.package_of("RipGrep"), Some("Scoop"));
        assert_eq!(evidence.package_of("NotepadPlusPlus"), Some("Chocolatey"));
        assert_eq!(evidence.package_of("ffmpeg"), Some("winget"));
        assert_eq!(evidence.package_of("scoop"), None);
        assert_eq!(
            evidence.last_launch(&[r"C:\Apps\Tool\tool.exe".into()]),
            Some(when)
        );
    }

    #[test]
    fn installer_app_paths_associations_and_user_assist_are_read_from_the_registry() {
        let area = TestRegistryKey::new();
        let component = RegKeyPath::new(
            Hive::LocalMachine,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData\S-1-5-18\Components\0123",
        );
        area.create(&component)
            .set_value("ABCD", &r"C:\Program Files\Suite\core.dll")
            .unwrap();
        let app_path = RegKeyPath::new(
            Hive::CurrentUser,
            r"Software\Microsoft\Windows\CurrentVersion\App Paths\viewer.exe",
        );
        area.create(&app_path)
            .set_value("", &r"C:\Tools\Viewer\viewer.exe")
            .unwrap();
        let open = RegKeyPath::new(
            Hive::CurrentUser,
            r"Software\Classes\Applications\paint.exe\shell\open\command",
        );
        area.create(&open)
            .set_value("", &r#""D:\Art\Paint\paint.exe" "%1""#)
            .unwrap();
        let count = RegKeyPath::new(
            Hive::CurrentUser,
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{GUID}\Count",
        );
        let mut data = vec![0u8; 72];
        let filetime: u64 = (1_790_812_800 + 11_644_473_600) * 10_000_000;
        data[60..68].copy_from_slice(&filetime.to_le_bytes());
        area.create(&count)
            .set_raw_value(
                rot13(r"C:\Games\Old\old.exe"),
                &winreg::RegValue {
                    bytes: data.into(),
                    vtype: winreg::enums::RegType::REG_BINARY,
                },
            )
            .unwrap();

        let evidence = Evidence::gather(
            &FakeFileSystem::new(),
            &area.registry(),
            &KnownFolders::under(Path::new(r"C:\")),
            &AtomicBool::new(false),
        );

        assert_eq!(
            evidence.referenced_in(r"C:\Program Files\Suite"),
            Some(Source::Installer)
        );
        assert_eq!(
            evidence.referenced_in(r"C:\Tools\Viewer"),
            Some(Source::Registration)
        );
        assert_eq!(
            evidence.referenced_in(r"D:\Art\Paint"),
            Some(Source::Registration)
        );
        assert_eq!(
            evidence.last_launch(&["OLD.EXE".into()]),
            Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).single()
        );
    }
}
