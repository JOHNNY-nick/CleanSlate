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

use crate::matcher::command_executable;
use crate::registry::{Hive, KeyView, RegKeyPath, Registry};

/// Key listing Windows services.
pub const SERVICES_KEY: &str = r"SYSTEM\CurrentControlSet\Services";

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
