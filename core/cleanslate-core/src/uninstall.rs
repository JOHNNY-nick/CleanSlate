//! Official uninstaller of an application (EF-12 to EF-15, specs § 7.1).
//!
//! The `UninstallString` is split into executable + arguments and started
//! directly with `CreateProcessW`: never through `cmd /c` or any shell
//! (safety rule 8). The process runs in a Job Object and the engine waits for
//! the whole process tree (uninstallers that copy themselves to %TEMP%).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::error::{CoreError, CoreResult};
use crate::inventory::is_guid;
use crate::model::{InstalledApp, JobProcess};

/// Longest wait for an uninstaller (specs § 7.1).
pub const UNINSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Executables never started as uninstallers: shells and script hosts.
const FORBIDDEN_EXECUTABLES: [&str; 7] = [
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wscript.exe",
    "cscript.exe",
    "mshta.exe",
    "bash.exe",
];

/// A program and its arguments, ready to be started without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Full path of the executable.
    pub program: PathBuf,
    /// Arguments, unquoted.
    pub args: Vec<String>,
}

impl Command {
    /// Command line as `CreateProcessW` expects it (MSVC quoting rules).
    pub fn command_line(&self) -> String {
        std::iter::once(quote(&self.program.to_string_lossy()))
            .chain(self.args.iter().map(|a| quote(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Quotes one argument following the MSVC `argv` rules.
pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned();
    }
    let mut out = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Splits arguments on spaces, keeping quoted groups together.
fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for (index, c) in text.char_indices() {
        // NSIS: `_?=<folder>` is always last and takes the rest verbatim.
        if !has_token && !in_quotes && text[index..].starts_with("_?=") {
            args.push(text[index..].trim_end().to_owned());
            return args;
        }
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    args.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            c => {
                current.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        args.push(current);
    }
    args
}

/// Folder of `%SystemRoot%\System32`, where bare names like `MsiExec.exe`
/// are resolved (never through `PATH`).
fn system32() -> PathBuf {
    let root =
        std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    root.join("System32")
}

/// Splits an `UninstallString` into program and arguments (safety rule 8).
///
/// Handles quoted paths, unquoted paths with spaces up to `.exe`, bare
/// system programs (`MsiExec.exe`, `rundll32`), and turns `MsiExec /I{GUID}`
/// into `/X{GUID}`. Refuses shells and script hosts.
pub fn split_command(raw: &str) -> CoreResult<Command> {
    let text = raw.trim();
    let missing = || CoreError::NoUninstaller {
        location: raw.to_owned(),
    };
    if text.is_empty() {
        return Err(missing());
    }
    let (program, rest) = if let Some(after) = text.strip_prefix('"') {
        let end = after.find('"').ok_or_else(missing)?;
        (after[..end].to_owned(), &after[end + 1..])
    } else {
        let lower = text.to_ascii_lowercase();
        // Unquoted path with spaces: several « .exe » may end a candidate.
        // Take the first one that exists on disk, else the longest.
        let ends: Vec<usize> = lower
            .match_indices(".exe")
            .map(|(i, _)| i + 4)
            .filter(|&end| lower[end..].is_empty() || lower[end..].starts_with(char::is_whitespace))
            .collect();
        let exe_end = ends
            .iter()
            .copied()
            .find(|&end| program_exists(Path::new(&text[..end])))
            .or_else(|| ends.last().copied());
        match exe_end {
            Some(end) => (text[..end].to_owned(), &text[end..]),
            None => {
                let end = text.find(char::is_whitespace).unwrap_or(text.len());
                (text[..end].to_owned(), &text[end..])
            }
        }
    };
    let mut program = PathBuf::from(program.trim());
    if program.parent().is_none_or(|p| p.as_os_str().is_empty()) {
        // Bare name: resolve in System32, never through PATH.
        let mut name = program.to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".exe") {
            name.push_str(".exe");
        }
        program = system32().join(name);
    }
    let file_name = program
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if FORBIDDEN_EXECUTABLES.contains(&file_name.as_str()) {
        return Err(CoreError::UnsafeUninstaller {
            location: raw.to_owned(),
        });
    }
    let mut args = split_args(rest);
    if file_name == "msiexec.exe" {
        args = to_msi_uninstall(args);
    }
    Ok(Command { program, args })
}

/// `MsiExec /I{GUID}` (repair/modify) becomes `/X{GUID}` (uninstall).
fn to_msi_uninstall(args: Vec<String>) -> Vec<String> {
    args.into_iter()
        .map(|arg| {
            let lower = arg.to_ascii_lowercase();
            if lower == "/i" || lower == "-i" {
                "/X".to_owned()
            } else if (lower.starts_with("/i") || lower.starts_with("-i"))
                && arg[2..].starts_with('{')
            {
                format!("/X{}", &arg[2..])
            } else {
                arg
            }
        })
        .collect()
}

/// Command that uninstalls `app` (specs § 7.1, step 3): `msiexec /x
/// {ProductCode}` for MSI, else the `UninstallString` (or the
/// `QuietUninstallString` in silent mode, EF-14).
pub fn build_command(app: &InstalledApp, quiet: bool) -> CoreResult<Command> {
    if let Some(code) = app
        .product_code
        .as_deref()
        .filter(|c| app.is_msi && is_guid(c))
    {
        let mut args = vec!["/x".to_owned(), code.to_owned()];
        if quiet {
            args.push("/qn".to_owned());
        }
        return Ok(Command {
            program: system32().join("msiexec.exe"),
            args,
        });
    }
    let source = match (&app.quiet_uninstall_string, &app.uninstall_string) {
        (Some(q), _) if quiet => q,
        (_, Some(u)) => u,
        _ => {
            return Err(CoreError::NoUninstaller {
                location: app.display_name.clone(),
            })
        }
    };
    split_command(source)
}

/// How an uninstaller run ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOutcome {
    /// Exit code of the started process.
    pub exit_code: Option<u32>,
    /// The timeout was reached (processes left running).
    pub timed_out: bool,
    /// The wait was cancelled (processes left running).
    pub cancelled: bool,
}

/// Starts a program and waits for its whole process tree.
pub trait ProcessRunner: Send + Sync {
    /// Runs `command` and waits until every process of its tree has exited,
    /// `cancel` is set or `timeout` is reached. `on_tick` receives the
    /// processes still running, at each poll (every 250 ms, ENF-06).
    fn run(
        &self,
        command: &Command,
        cancel: &AtomicBool,
        timeout: Duration,
        on_tick: &dyn Fn(&[JobProcess]),
    ) -> CoreResult<RunOutcome>;
}

/// System restore points (EF-15).
pub trait RestorePoints: Send + Sync {
    /// Starts a restore point; returns its sequence number.
    fn begin(&self, description: &str) -> Result<i64, String>;
    /// Ends the restore point (cancelled if `success` is false).
    fn end(&self, sequence: i64, success: bool);
}

/// Refuses to run anything (test engines).
#[derive(Debug, Default)]
pub struct NoProcessRunner;

impl ProcessRunner for NoProcessRunner {
    fn run(
        &self,
        command: &Command,
        _: &AtomicBool,
        _: Duration,
        _: &dyn Fn(&[JobProcess]),
    ) -> CoreResult<RunOutcome> {
        Err(CoreError::InvalidInput(format!(
            "no process runner on this engine: {}",
            command.program.display()
        )))
    }
}

/// No restore point (test engines).
#[derive(Debug, Default)]
pub struct NoRestorePoints;

impl RestorePoints for NoRestorePoints {
    fn begin(&self, _: &str) -> Result<i64, String> {
        Err("restore points unavailable on this engine".into())
    }
    fn end(&self, _: i64, _: bool) {}
}

/// Real runner: `CreateProcessW` (suspended) + Job Object + polling.
#[derive(Debug, Default)]
pub struct JobProcessRunner;

impl ProcessRunner for JobProcessRunner {
    fn run(
        &self,
        command: &Command,
        cancel: &AtomicBool,
        timeout: Duration,
        on_tick: &dyn Fn(&[JobProcess]),
    ) -> CoreResult<RunOutcome> {
        win::run_in_job(command, cancel, timeout, on_tick)
    }
}

/// Real restore points through `SRSetRestorePointW`.
#[derive(Debug, Default)]
pub struct SystemRestorePoints;

impl RestorePoints for SystemRestorePoints {
    fn begin(&self, description: &str) -> Result<i64, String> {
        win::restore_point(description, true, true, 0)
    }
    fn end(&self, sequence: i64, success: bool) {
        let _ = win::restore_point("", false, success, sequence);
    }
}

/// `true` if `path` exists (the uninstaller program must exist).
pub fn program_exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectBasicProcessIdList, QueryInformationJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    };
    use windows::Win32::System::Restore::{
        SRSetRestorePointW, APPLICATION_UNINSTALL, BEGIN_SYSTEM_CHANGE, CANCELLED_OPERATION,
        END_SYSTEM_CHANGE, RESTOREPOINTINFOW, STATEMGRSTATUS,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, ResumeThread, WaitForSingleObject, CREATE_SUSPENDED,
        PROCESS_INFORMATION, STARTUPINFOW,
    };

    use super::{Command, RunOutcome};
    use crate::error::{CoreError, CoreResult};
    use crate::model::JobProcess;

    /// Most process ids read from the Job Object at once.
    const MAX_JOB_PROCESSES: usize = 256;

    /// Processes still in `job`, with their executable names.
    fn job_processes(job: windows::Win32::Foundation::HANDLE) -> Vec<JobProcess> {
        // Header (two u32) then the ids: an aligned u64 buffer.
        let mut buffer = vec![0u64; 1 + MAX_JOB_PROCESSES];
        let raw = buffer
            .as_mut_ptr()
            .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        // SAFETY: the buffer is aligned for the structure and as large as
        // announced; ids are read only up to the count Windows wrote.
        unsafe {
            let ok = QueryInformationJobObject(
                Some(job),
                JobObjectBasicProcessIdList,
                raw.cast(),
                (buffer.len() * 8) as u32,
                None,
            );
            if ok.is_err() {
                return Vec::new();
            }
            let count = ((*raw).NumberOfProcessIdsInList as usize).min(MAX_JOB_PROCESSES);
            let ids = std::slice::from_raw_parts((*raw).ProcessIdList.as_ptr(), count);
            ids.iter()
                .filter_map(|&id| {
                    let pid = u32::try_from(id).ok()?;
                    let name = crate::system::process_path(pid)
                        .and_then(|p| {
                            std::path::Path::new(&p)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                        })
                        .unwrap_or_else(|| format!("processus {pid}"));
                    Some(JobProcess { pid, name })
                })
                .collect()
        }
    }

    const POLL: Duration = Duration::from_millis(250);

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain([0]).collect()
    }

    pub(super) fn run_in_job(
        command: &Command,
        cancel: &AtomicBool,
        timeout: Duration,
        on_tick: &dyn Fn(&[JobProcess]),
    ) -> CoreResult<RunOutcome> {
        let program = wide(command.program.as_os_str());
        let mut line = wide(std::ffi::OsStr::new(&command.command_line()));
        let failed = |e: windows::core::Error| {
            CoreError::InvalidInput(format!("{}: {e}", command.program.display()))
        };
        let mut outcome = RunOutcome::default();
        // SAFETY: NUL-terminated buffers outlive the calls; the command line
        // buffer is mutable as CreateProcessW requires; every handle created
        // here is closed before returning; no shell is involved.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).map_err(failed)?;
            let startup = STARTUPINFOW {
                cb: std::mem::size_of::<STARTUPINFOW>() as u32,
                ..Default::default()
            };
            let mut info = PROCESS_INFORMATION::default();
            let created = CreateProcessW(
                PCWSTR(program.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_SUSPENDED,
                None,
                PCWSTR::null(),
                &startup,
                &mut info,
            );
            if let Err(err) = created {
                let _ = CloseHandle(job);
                return Err(failed(err));
            }
            let in_job = AssignProcessToJobObject(job, info.hProcess).is_ok();
            ResumeThread(info.hThread);
            let started = Instant::now();
            loop {
                if cancel.load(Ordering::Relaxed) {
                    outcome.cancelled = true;
                    break;
                }
                if started.elapsed() >= timeout {
                    outcome.timed_out = true;
                    break;
                }
                let active = if in_job {
                    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                    let ok = QueryInformationJobObject(
                        Some(job),
                        JobObjectBasicAccountingInformation,
                        (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                        std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                        None,
                    );
                    ok.is_err() || accounting.ActiveProcesses > 0
                } else {
                    WaitForSingleObject(info.hProcess, 0).0 != 0
                };
                if !active {
                    break;
                }
                if in_job {
                    on_tick(&job_processes(job));
                }
                std::thread::sleep(POLL);
            }
            let mut code = 0u32;
            if GetExitCodeProcess(info.hProcess, &mut code).is_ok() && code != 259 {
                outcome.exit_code = Some(code);
            }
            let _ = CloseHandle(info.hThread);
            let _ = CloseHandle(info.hProcess);
            // No KILL_ON_JOB_CLOSE: closing the job never kills the uninstaller.
            let _ = CloseHandle(job);
        }
        Ok(outcome)
    }

    /// Begins (`begin`) or ends a restore point; an end with `success ==
    /// false` cancels it.
    pub(super) fn restore_point(
        description: &str,
        begin: bool,
        success: bool,
        sequence: i64,
    ) -> Result<i64, String> {
        // The structure is packed: fill the text in a local buffer first
        // (a reference to a packed field would be misaligned).
        let mut text = [0u16; 256];
        for (slot, unit) in text.iter_mut().zip(description.encode_utf16().take(255)) {
            *slot = unit;
        }
        let spec = RESTOREPOINTINFOW {
            dwEventType: if begin {
                BEGIN_SYSTEM_CHANGE
            } else {
                END_SYSTEM_CHANGE
            },
            dwRestorePtType: if begin || success {
                APPLICATION_UNINSTALL
            } else {
                CANCELLED_OPERATION
            },
            llSequenceNumber: sequence,
            szDescription: text,
        };
        let mut status = STATEMGRSTATUS::default();
        // SAFETY: both structures are initialized and live for the call.
        let ok = unsafe { SRSetRestorePointW(&spec, &mut status) }.as_bool();
        let code = status.nStatus.0;
        let seq = status.llSequenceNumber;
        if ok {
            Ok(seq)
        } else {
            Err(format!(
                "SRSetRestorePointW: erreur {code} (protection système désactivée ?)"
            ))
        }
    }
}

/// Waits for `cancel`, polling (used by fakes in tests).
pub fn wait_until(cancel: &AtomicBool, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if cancel.load(Ordering::Relaxed) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppKind, InstallerKind, RegistryView};

    fn sys(name: &str) -> PathBuf {
        system32().join(name)
    }

    fn cmd(program: impl Into<PathBuf>, args: &[&str]) -> Command {
        Command {
            program: program.into(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    /// 15 real-world forms of UninstallString.
    #[test]
    fn split_real_uninstall_strings() {
        let cases: Vec<(&str, Command)> = vec![
            (
                r#""C:\Program Files\GIMP 2\uninst\unins000.exe""#,
                cmd(r"C:\Program Files\GIMP 2\uninst\unins000.exe", &[]),
            ),
            (
                r#""C:\Program Files\7-Zip\Uninstall.exe" /S"#,
                cmd(r"C:\Program Files\7-Zip\Uninstall.exe", &["/S"]),
            ),
            (
                r"C:\Program Files\Notepad++\uninstall.exe",
                cmd(r"C:\Program Files\Notepad++\uninstall.exe", &[]),
            ),
            (
                r"C:\Program Files (x86)\VideoLAN\VLC\uninstall.exe /S",
                cmd(
                    r"C:\Program Files (x86)\VideoLAN\VLC\uninstall.exe",
                    &["/S"],
                ),
            ),
            (
                "MsiExec.exe /I{23170F69-40C1-2702-2408-000001000000}",
                cmd(
                    sys("MsiExec.exe"),
                    &["/X{23170F69-40C1-2702-2408-000001000000}"],
                ),
            ),
            (
                "MsiExec.exe /X{23170F69-40C1-2702-2408-000001000000}",
                cmd(
                    sys("MsiExec.exe"),
                    &["/X{23170F69-40C1-2702-2408-000001000000}"],
                ),
            ),
            (
                "msiexec /i {AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE} /qb",
                cmd(
                    sys("msiexec.exe"),
                    &["/X", "{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}", "/qb"],
                ),
            ),
            (
                r#"C:\Windows\System32\MsiExec.exe /I{11111111-2222-3333-4444-555555555555}"#,
                cmd(
                    r"C:\Windows\System32\MsiExec.exe",
                    &["/X{11111111-2222-3333-4444-555555555555}"],
                ),
            ),
            (
                r#""C:\Users\Ana\AppData\Local\Discord\Update.exe" --uninstall"#,
                cmd(
                    r"C:\Users\Ana\AppData\Local\Discord\Update.exe",
                    &["--uninstall"],
                ),
            ),
            (
                r#""C:\Program Files\App\uninst.exe" /S _?=C:\Program Files\App"#,
                cmd(
                    r"C:\Program Files\App\uninst.exe",
                    &["/S", r"_?=C:\Program Files\App"],
                ),
            ),
            (
                r#""C:\Program Files\App\setup.exe" -remove -lang "fr FR""#,
                cmd(
                    r"C:\Program Files\App\setup.exe",
                    &["-remove", "-lang", "fr FR"],
                ),
            ),
            (
                r#"rundll32.exe "C:\Program Files\App\lib.dll",Uninstall"#,
                cmd(
                    sys("rundll32.exe"),
                    &[r"C:\Program Files\App\lib.dll,Uninstall"],
                ),
            ),
            (
                "RunDll32 advpack.dll,LaunchINFSection x.inf",
                cmd(
                    sys("RunDll32.exe"),
                    &["advpack.dll,LaunchINFSection", "x.inf"],
                ),
            ),
            (
                r"C:\Program Files\My.exe App\unins.exe /SILENT",
                cmd(r"C:\Program Files\My.exe App\unins.exe", &["/SILENT"]),
            ),
            ("   \"C:\\Tools\\x.exe\"   ", cmd(r"C:\Tools\x.exe", &[])),
        ];
        assert_eq!(cases.len(), 15);
        for (raw, expected) in cases {
            assert_eq!(split_command(raw).unwrap(), expected, "{raw}");
        }
    }

    #[test]
    fn shells_and_empty_strings_are_refused() {
        for raw in [
            r"cmd.exe /c del C:\x",
            r#""C:\Windows\System32\cmd.exe" /c rd /s /q C:\"#,
            "powershell -Command Remove-Item x",
            "wscript uninstall.vbs",
            "mshta http://x",
        ] {
            assert_eq!(
                split_command(raw).unwrap_err().code(),
                "UNSAFE_UNINSTALLER",
                "{raw}"
            );
        }
        assert_eq!(split_command("  ").unwrap_err().code(), "NO_UNINSTALLER");
        assert_eq!(
            split_command("\"C:\\unterminated").unwrap_err().code(),
            "NO_UNINSTALLER"
        );
    }

    #[test]
    fn quoting_follows_msvc_rules() {
        assert_eq!(quote("plain"), "plain");
        assert_eq!(quote("two words"), "\"two words\"");
        assert_eq!(quote(r"C:\dir with space\"), "\"C:\\dir with space\\\\\"");
        assert_eq!(quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote(""), "\"\"");
        let line = cmd(r"C:\Program Files\A\u.exe", &["/S", "a b"]).command_line();
        assert_eq!(line, r#""C:\Program Files\A\u.exe" /S "a b""#);
    }

    fn app(uninstall: Option<&str>, quiet: Option<&str>, msi: Option<&str>) -> InstalledApp {
        InstalledApp {
            id: "id".into(),
            display_name: "App".into(),
            publisher: None,
            version: None,
            install_date: None,
            install_location: None,
            estimated_size_kb: None,
            uninstall_string: uninstall.map(Into::into),
            quiet_uninstall_string: quiet.map(Into::into),
            is_msi: msi.is_some(),
            product_code: msi.map(Into::into),
            registry_view: RegistryView::Hklm64,
            kind: AppKind::Win32,
            hidden_reason: None,
            installer: InstallerKind::Unknown,
            package_full_name: None,
            package_family_name: None,
        }
    }

    #[test]
    fn build_command_for_msi_quiet_and_missing() {
        let guid = "{23170F69-40C1-2702-2408-000001000000}";
        let msi = app(Some("MsiExec.exe /I{x}"), None, Some(guid));
        assert_eq!(
            build_command(&msi, false).unwrap(),
            cmd(sys("msiexec.exe"), &["/x", guid])
        );
        assert_eq!(
            build_command(&msi, true).unwrap(),
            cmd(sys("msiexec.exe"), &["/x", guid, "/qn"])
        );

        let exe = app(Some(r#""C:\A\u.exe""#), Some(r#""C:\A\u.exe" /S"#), None);
        assert_eq!(build_command(&exe, false).unwrap(), cmd(r"C:\A\u.exe", &[]));
        assert_eq!(
            build_command(&exe, true).unwrap(),
            cmd(r"C:\A\u.exe", &["/S"])
        );
        let no_quiet = app(Some(r#""C:\A\u.exe""#), None, None);
        assert_eq!(
            build_command(&no_quiet, true).unwrap(),
            cmd(r"C:\A\u.exe", &[])
        );
        assert_eq!(
            build_command(&app(None, None, None), false)
                .unwrap_err()
                .code(),
            "NO_UNINSTALLER"
        );
    }

    /// Runs a harmless, read-only system program to check the Job Object wait.
    #[test]
    fn job_runner_waits_for_a_real_process() {
        let command = cmd(sys("hostname.exe"), &[]);
        let outcome = JobProcessRunner
            .run(
                &command,
                &AtomicBool::new(false),
                Duration::from_secs(30),
                &|_| {},
            )
            .unwrap();
        assert_eq!(outcome.exit_code, Some(0));
        assert!(!outcome.timed_out && !outcome.cancelled);
        let missing = cmd(r"C:\definitely\missing.exe", &[]);
        assert!(JobProcessRunner
            .run(
                &missing,
                &AtomicBool::new(false),
                Duration::from_secs(1),
                &|_| {}
            )
            .is_err());
    }

    #[test]
    fn test_backends_refuse() {
        assert!(NoProcessRunner
            .run(
                &cmd(sys("hostname.exe"), &[]),
                &AtomicBool::new(false),
                Duration::from_secs(1),
                &|_| {}
            )
            .is_err());
        assert!(NoRestorePoints.begin("x").is_err());
        NoRestorePoints.end(1, true);
        assert!(!wait_until(
            &AtomicBool::new(false),
            Duration::from_millis(20)
        ));
        assert!(wait_until(
            &AtomicBool::new(true),
            Duration::from_millis(20)
        ));
        assert!(program_exists(&sys("hostname.exe")));
    }
}
