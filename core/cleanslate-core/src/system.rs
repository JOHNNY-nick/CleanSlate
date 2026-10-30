//! Read-only probes of the running system: executables of services and of
//! running processes (specs § 6.1, step 1). A candidate that contains one of
//! them is never proposed.

use windows::core::PWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use std::path::{Path, PathBuf};

use crate::fs::FileSystem;
use crate::matcher::command_executable;
use crate::model::LeftoverKind;
use crate::registry::{Hive, KeyView, RegData, RegKeyPath, Registry};
use crate::safety::KnownFolders;

/// Key listing Windows services.
pub const SERVICES_KEY: &str = r"SYSTEM\CurrentControlSet\Services";
/// Largest task definition read.
const MAX_TASK_LEN: u64 = 1024 * 1024;
/// Depth of the task folders walked below `System32\Tasks`.
const TASK_DEPTH: usize = 8;

/// A scheduled task or a service whose program no longer exists (EF-07).
/// Detection only: CleanSlate never removes them (V11 plan, decision of the
/// owner), the user does it in the Task Scheduler or with `sc.exe delete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadEntry {
    /// [`LeftoverKind::ScheduledTask`] or [`LeftoverKind::Service`].
    pub kind: LeftoverKind,
    /// Task path (`\Vendor\Updater`) or service name.
    pub name: String,
    /// Definition file of the task, or key of the service.
    pub location: String,
    /// Missing program.
    pub program: PathBuf,
}

/// Scheduled tasks whose programs are all missing, read from the task
/// definitions (`%WINDIR%\System32\Tasks`, XML). Tasks of `\Microsoft\`,
/// programs under `%WINDIR%` and relative commands are never reported.
/// Unreadable folders (no administrator rights) are skipped.
pub fn dead_tasks(fs: &dyn FileSystem, folders: &KnownFolders) -> Vec<DeadEntry> {
    let root = folders.windir.join(r"System32\Tasks");
    let mut found = Vec::new();
    walk_tasks(fs, folders, &root, &root, 0, &mut found);
    found
}

fn walk_tasks(
    fs: &dyn FileSystem,
    folders: &KnownFolders,
    root: &Path,
    dir: &Path,
    depth: usize,
    found: &mut Vec<DeadEntry>,
) {
    if depth > TASK_DEPTH {
        return;
    }
    let entries = match fs.read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::debug!(%err, "task folder skipped");
            return;
        }
    };
    for entry in entries {
        let name = entry.path.strip_prefix(root).unwrap_or(&entry.path);
        if entry.metadata.is_reparse_point
            || (depth == 0 && name.as_os_str().eq_ignore_ascii_case("Microsoft"))
        {
            continue;
        }
        if entry.metadata.is_dir {
            walk_tasks(fs, folders, root, &entry.path, depth + 1, found);
            continue;
        }
        let Ok(bytes) = fs.read_file(&entry.path, MAX_TASK_LEN) else {
            continue;
        };
        let programs: Vec<PathBuf> = task_commands(&decode_xml(&bytes))
            .iter()
            .filter_map(|c| program_path(c, folders))
            .collect();
        // Every program must be missing; one present program keeps the task alive.
        if let Some(program) = programs.first() {
            if programs.iter().all(|p| !fs.exists(p)) {
                found.push(DeadEntry {
                    kind: LeftoverKind::ScheduledTask,
                    name: format!(r"\{}", name.display()),
                    location: entry.path.display().to_string(),
                    program: program.clone(),
                });
            }
        }
    }
}

/// Services whose program is missing (`ImagePath`). Drivers, programs under
/// `%WINDIR%` and entries without a program are never reported.
pub fn dead_services(
    fs: &dyn FileSystem,
    registry: &dyn Registry,
    folders: &KnownFolders,
) -> Vec<DeadEntry> {
    let root = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY).with_view(KeyView::Force64);
    let Ok(names) = registry.subkeys(&root) else {
        return Vec::new();
    };
    names
        .iter()
        .filter_map(|name| {
            let key = root.join(name);
            let values = registry.values(&key).ok()?;
            let get = |wanted: &str| {
                values
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(wanted))
                    .map(|(_, v)| v)
            };
            // Types 1 and 2 are kernel and file system drivers.
            if let Some(RegData::Dword(kind)) = get("Type") {
                if *kind & 0x3 != 0 {
                    return None;
                }
            }
            let image = get("ImagePath")?.as_str()?;
            let program = program_path(&command_executable_raw(image)?, folders)?;
            (!fs.exists(&program)).then(|| DeadEntry {
                kind: LeftoverKind::Service,
                name: name.clone(),
                location: key.to_string(),
                program,
            })
        })
        .collect()
}

/// Program of a service command line, quoted or not (`.exe` ends it).
fn command_executable_raw(command: &str) -> Option<String> {
    command_executable(command).or_else(|| {
        let command = command.trim();
        command
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
            .map(str::to_owned)
    })
}

/// Absolute program of a command, environment variables expanded with the
/// folders of this machine; `None` when it cannot be judged (relative,
/// unknown variable) or lies under `%WINDIR%`.
fn program_path(command: &str, folders: &KnownFolders) -> Option<PathBuf> {
    let mut text = command.trim().trim_matches('"').to_owned();
    if let Some(rest) = text.strip_prefix(r"\??\") {
        text = rest.to_owned();
    }
    let windir = folders.windir.to_string_lossy().into_owned();
    if text
        .get(..12)
        .is_some_and(|p| p.eq_ignore_ascii_case(r"\SystemRoot\"))
    {
        text = format!(r"{windir}\{}", text.get(12..).unwrap_or_default());
    }
    let drive = windir.get(..2).unwrap_or("C:").to_owned();
    let variables = [
        ("%SystemRoot%", windir.clone()),
        ("%windir%", windir.clone()),
        ("%SystemDrive%", drive),
        (
            "%ProgramFiles%",
            folders.program_files.to_string_lossy().into_owned(),
        ),
        (
            "%ProgramFiles(x86)%",
            folders.program_files_x86.to_string_lossy().into_owned(),
        ),
        (
            "%ProgramW6432%",
            folders.program_files.to_string_lossy().into_owned(),
        ),
        (
            "%ProgramData%",
            folders.program_data.to_string_lossy().into_owned(),
        ),
        (
            "%LOCALAPPDATA%",
            folders.local.to_string_lossy().into_owned(),
        ),
        ("%APPDATA%", folders.roaming.to_string_lossy().into_owned()),
        (
            "%USERPROFILE%",
            folders.user_profile.to_string_lossy().into_owned(),
        ),
    ];
    for (name, value) in &variables {
        while let Some(i) = text.to_ascii_lowercase().find(&name.to_ascii_lowercase()) {
            text.replace_range(i..i + name.len(), value);
        }
    }
    let absolute = text.as_bytes().get(1) == Some(&b':') && !text.contains('%');
    let under_windir = text
        .to_ascii_lowercase()
        .starts_with(&format!(r"{}\", windir.to_ascii_lowercase()));
    (absolute && !under_windir).then(|| PathBuf::from(text))
}

/// Text of a task definition: UTF-16 LE with a byte order mark, else UTF-8.
fn decode_xml(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            // Pairs of bytes; an odd trailing byte is ignored.
            let (pairs, _) = rest.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
            String::from_utf16_lossy(&units)
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Contents of the `<Command>` elements of a task (its `Exec` actions).
fn task_commands(xml: &str) -> Vec<String> {
    xml.split("<Command>")
        .skip(1)
        .filter_map(|part| part.split("</Command>").next())
        .map(|c| {
            c.replace("&quot;", "\"")
                .replace("&apos;", "'")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&")
        })
        .collect()
}

/// Executables of the services (`ImagePath`), read-only. Drivers (`.sys`)
/// and unreadable entries are ignored.
pub fn service_executables(registry: &dyn Registry) -> Vec<String> {
    let root = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY).with_view(KeyView::Force64);
    let Ok(names) = registry.subkeys(&root) else {
        return Vec::new();
    };
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    names
        .iter()
        .filter_map(|name| registry.values(&root.join(name)).ok())
        .filter_map(|values| {
            values
                .into_iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("ImagePath"))
                .and_then(|(_, v)| v.as_str().map(str::to_owned))
        })
        .filter_map(|raw| {
            let expanded = raw
                .trim()
                .trim_start_matches(r"\??\")
                .replace("%SystemRoot%", &system_root)
                .replace("%systemroot%", &system_root);
            command_executable(&expanded)
        })
        .collect()
}

/// Full paths of the running processes this process may query.
pub fn process_executables() -> Vec<String> {
    let mut found = Vec::new();
    // SAFETY: plain Win32 calls; the snapshot handle is closed below and the
    // entry structure is initialized with its size as the API requires.
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return found;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            if let Some(path) = process_path(entry.th32ProcessID) {
                found.push(path);
            }
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    found.sort();
    found.dedup();
    found
}

/// Full image path of one process, if it can be queried.
fn process_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    // SAFETY: the handle is checked, used once, then closed; the buffer
    // length is passed in and updated by the API.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = [0u16; 1024];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        ok.then(|| String::from_utf16_lossy(&buffer[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::TestRegistryKey;

    #[test]
    fn service_image_paths_are_parsed() {
        let area = TestRegistryKey::new();
        let root = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY);
        area.create(&root.join("Agent"))
            .set_value(
                "ImagePath",
                &r#""C:\Program Files\Vendor\agent.exe" -service"#,
            )
            .unwrap();
        area.create(&root.join("Driver"))
            .set_value("ImagePath", &r"\SystemRoot\System32\drivers\x.sys")
            .unwrap();
        area.create(&root.join("Host"))
            .set_value(
                "ImagePath",
                &r"%SystemRoot%\System32\svchost.exe -k netsvcs",
            )
            .unwrap();
        area.create(&root.join("Empty"));

        let found = service_executables(&area.registry());
        assert!(found.contains(&r"C:\Program Files\Vendor\agent.exe".to_owned()));
        assert!(found.iter().any(|f| f.ends_with(r"System32\svchost.exe")));
        assert_eq!(found.len(), 2);
    }

    /// Task definition as Windows writes it: UTF-16 LE with a byte order mark.
    fn task_xml(commands: &[&str]) -> Vec<u8> {
        let actions: String = commands
            .iter()
            .map(|c| format!("<Exec><Command>{c}</Command><Arguments>/q</Arguments></Exec>"))
            .collect();
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-16\"?><Task><Actions>{actions}</Actions></Task>"
        );
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    }

    #[test]
    fn dead_tasks_are_found_and_system_ones_ignored() {
        use crate::fs::FakeFileSystem;
        let base = Path::new(r"C:\Fake");
        let folders = KnownFolders::under(base);
        let tasks = base.join(r"Windows\System32\Tasks");
        let mut fs = FakeFileSystem::new();
        fs.add_file(base.join(r"Program Files\Alive\a.exe"), 10, None)
            .add_file_with_content(
                tasks.join(r"Vendor\Updater"),
                task_xml(&[r#""%ProgramFiles%\Vendor\upd.exe""#]),
                None,
            )
            .add_file_with_content(
                tasks.join("Alive"),
                task_xml(&[r"C:\Fake\Program Files\Alive\a.exe"]),
                None,
            )
            .add_file_with_content(
                tasks.join("Mixed"),
                task_xml(&[r"C:\Fake\Gone\x.exe", r"C:\Fake\Program Files\Alive\a.exe"]),
                None,
            )
            .add_file_with_content(
                tasks.join(r"Microsoft\Windows\Defrag"),
                task_xml(&[r"C:\Fake\Gone\defrag.exe"]),
                None,
            )
            .add_file_with_content(
                tasks.join("Sys"),
                task_xml(&[r"%windir%\System32\gone.exe"]),
                None,
            )
            .add_file_with_content(tasks.join("Relative"), task_xml(&["gone.exe"]), None)
            .add_file_with_content(tasks.join("Unknown"), task_xml(&[r"%NOPE%\gone.exe"]), None)
            .add_file_with_content(tasks.join("Com"), task_xml(&[]), None)
            .add_file_with_content(
                tasks.join("Entities"),
                task_xml(&[r"C:\Fake\Old &amp; Co\run.exe"]),
                None,
            );

        let mut found = dead_tasks(&fs, &folders);
        found.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, [r"\Entities", r"\Vendor\Updater"]);
        assert_eq!(found[0].program, Path::new(r"C:\Fake\Old & Co\run.exe"));
        assert_eq!(
            found[1].program,
            Path::new(r"C:\Fake\Program Files\Vendor\upd.exe")
        );
        assert_eq!(found[1].kind, LeftoverKind::ScheduledTask);
        assert!(found[1]
            .location
            .ends_with(r"System32\Tasks\Vendor\Updater"));
    }

    #[test]
    fn unreadable_task_folder_gives_nothing() {
        use crate::fs::FakeFileSystem;
        let folders = KnownFolders::under(r"C:\Fake");
        assert!(dead_tasks(&FakeFileSystem::new(), &folders).is_empty());
    }

    #[test]
    fn dead_services_skip_drivers_and_windows() {
        use crate::fs::FakeFileSystem;
        let area = TestRegistryKey::new();
        let root = RegKeyPath::new(Hive::LocalMachine, SERVICES_KEY);
        let service = |name: &str, image: &str, kind: u32| {
            let key = area.create(&root.join(name));
            key.set_value("ImagePath", &image).unwrap();
            key.set_value("Type", &kind).unwrap();
        };
        service("Gone", r#""C:\Fake\Program Files\Gone\svc.exe" -run"#, 16);
        service("OldApp", r"C:\Fake\Program Files\Old App\svc.exe /x", 16);
        service("Present", r"C:\Fake\Program Files\Alive\a.exe", 16);
        service("Driver", r"C:\Fake\Program Files\Gone\drv.exe", 1);
        service("Host", r"%SystemRoot%\System32\svchost.exe -k netsvcs", 32);
        area.create(&root.join("NoImage"));
        let mut fs = FakeFileSystem::new();
        fs.add_file(r"C:\Fake\Program Files\Alive\a.exe", 10, None);

        let mut found = dead_services(&fs, &area.registry(), &KnownFolders::under(r"C:\Fake"));
        found.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Gone", "OldApp"]);
        assert_eq!(
            found[1].program,
            Path::new(r"C:\Fake\Program Files\Old App\svc.exe")
        );
        assert_eq!(found[0].kind, LeftoverKind::Service);
        assert!(found[0].location.ends_with(r"Services\Gone"));
    }

    #[test]
    fn current_process_is_listed() {
        // Read-only: lists processes, including this test binary.
        let me = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .to_lowercase();
        let found = process_executables();
        assert!(found.iter().any(|p| p.to_lowercase() == me));
    }
}
