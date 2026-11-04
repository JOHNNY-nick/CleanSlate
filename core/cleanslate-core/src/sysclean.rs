//! System clean-up of the Gestionnaire (D79, EF-26, EF-31), by the official
//! mechanisms of Windows only: DISM for the component store, the services of
//! Windows Update stopped and restarted cleanly around their download cache,
//! the Delivery Optimization cmdlet, Disk Cleanup for `Windows.old`, `powercfg`
//! for the hibernation, `vssadmin` for the restore points. Never a change of
//! owner or of rights. Every return code is checked and logged (ENF-14).
//! Behind [`SystemCleaner`]: a test engine has [`NoSystemCleaner`], which
//! refuses everything; the real one is never run by the tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::fs::{FileSystem, RealFileSystem};
use crate::manager::folder_size;
use crate::safety::KnownFolders;

/// A clean-up of the « Système » tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemTask {
    /// Component store (WinSxS): `DISM /StartComponentCleanup`.
    ComponentStore,
    /// Download cache of Windows Update.
    UpdateCache,
    /// Cache of the Delivery Optimization.
    DeliveryOptimization,
    /// Previous installation of Windows (`Windows.old`).
    WindowsOld,
    /// Hibernation file: turns the hibernation off.
    Hibernation,
    /// Restore points: the old ones (the most recent and those of less than 30 days
    /// are kept, except in expert mode).
    RestorePoints,
}

/// One line of the « Système » tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemItem {
    /// What it is.
    pub task: SystemTask,
    /// Bytes it takes, when known without a long analysis.
    pub size_bytes: Option<u64>,
    /// There is something to clean.
    pub available: bool,
}

/// What a clean-up did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemOutcome {
    /// Bytes freed, measured before and after.
    pub freed_bytes: u64,
    /// Windows asks for a restart to finish.
    pub restart_needed: bool,
    /// Nothing changed: simulation.
    pub simulated: bool,
    /// Commands run and their return codes, for the log.
    pub steps: Vec<String>,
}

/// A request of the « Système » tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemRequest {
    /// Clean-up asked.
    pub task: SystemTask,
    /// Expert mode: the recent restore points may go too.
    #[serde(default)]
    pub expert_mode: bool,
    /// Simulation (default).
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

fn default_true() -> bool {
    true
}

/// The clean-ups of Windows; refused on a test engine.
pub trait SystemCleaner: Send + Sync {
    /// Size of each item, read-only.
    fn inspect(&self) -> CoreResult<Vec<SystemItem>>;
    /// Runs one clean-up.
    fn run(&self, task: SystemTask, expert_mode: bool) -> CoreResult<SystemOutcome>;
}

/// The cleaner of a test engine: refuses everything.
pub struct NoSystemCleaner;

impl SystemCleaner for NoSystemCleaner {
    fn inspect(&self) -> CoreResult<Vec<SystemItem>> {
        Err(CoreError::InvalidInput(
            "system clean-up not available on this engine".into(),
        ))
    }
    fn run(&self, _: SystemTask, _: bool) -> CoreResult<SystemOutcome> {
        Err(CoreError::InvalidInput(
            "system clean-up not available on this engine".into(),
        ))
    }
}

/// A restore point listed by `vssadmin list shadows`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowCopy {
    /// `{GUID}` of the shadow copy.
    pub id: String,
    /// Creation time (local time of the PC, read as UTC: only the order and the age count).
    pub created: DateTime<Utc>,
}

/// Restore points listed by `vssadmin list shadows` (English or French output).
pub fn parse_shadows(output: &str) -> Vec<ShadowCopy> {
    let mut list = Vec::new();
    let mut created: Option<DateTime<Utc>> = None;
    for line in output.lines().map(str::trim) {
        let lower = line.to_lowercase();
        if lower.contains("creation time")
            || lower.contains("heure de cr")
            || lower.contains("date de cr")
        {
            created = line
                .split_once(": ")
                .and_then(|(_, t)| parse_time(t.trim()));
        } else if lower.starts_with("shadow copy id")
            || lower.starts_with("id du cliché")
            || lower.starts_with("id de cliché")
        {
            if let (Some(id), Some(when)) = (
                line.split_once(": ").map(|(_, id)| id.trim().to_owned()),
                created,
            ) {
                list.push(ShadowCopy { id, created: when });
            }
        }
    }
    list
}

fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    [
        "%m/%d/%Y %I:%M:%S %p",
        "%d/%m/%Y %H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ]
    .iter()
    .find_map(|f| NaiveDateTime::parse_from_str(text, f).ok())
    .map(|n| n.and_utc())
}

/// Restore points to delete: the old ones; the most recent and those of less than 30
/// days stay, unless the expert mode lets every one go (EF-26).
pub fn shadows_to_delete(
    shadows: &[ShadowCopy],
    now: DateTime<Utc>,
    expert_mode: bool,
) -> Vec<ShadowCopy> {
    if expert_mode {
        return shadows.to_vec();
    }
    let newest = shadows.iter().map(|s| s.created).max();
    shadows
        .iter()
        .filter(|s| Some(s.created) != newest && now - s.created > Duration::days(30))
        .cloned()
        .collect()
}

/// The clean-ups of Windows on this PC.
pub struct WinSystemCleaner {
    /// Folders of the machine.
    pub folders: KnownFolders,
}

impl WinSystemCleaner {
    fn system32(&self, exe: &str) -> PathBuf {
        self.folders.windir.join("System32").join(exe)
    }

    fn drive(&self) -> String {
        self.folders
            .windir
            .to_string_lossy()
            .get(..2)
            .unwrap_or("C:")
            .to_owned()
    }

    // Runs a tool of System32 with its arguments, never through a shell (rule 8).
    fn run_tool(&self, exe: &str, args: &[&str], steps: &mut Vec<String>) -> CoreResult<i32> {
        let output = Command::new(self.system32(exe))
            .args(args)
            .output()
            .map_err(|e| CoreError::from_io(self.system32(exe), e))?;
        let code = output.status.code().unwrap_or(-1);
        steps.push(format!("{exe} {} → {code}", args.join(" ")));
        tracing::info!(exe, ?args, code, "system clean-up step");
        Ok(code)
    }

    fn checked(&self, exe: &str, args: &[&str], steps: &mut Vec<String>) -> CoreResult<bool> {
        match self.run_tool(exe, args, steps)? {
            0 => Ok(false),
            3010 => Ok(true), // done, restart needed
            code => Err(CoreError::InvalidInput(format!(
                "{exe} failed with code {code}"
            ))),
        }
    }

    fn size(&self, path: &Path) -> u64 {
        match RealFileSystem.metadata(path) {
            Ok(meta) if meta.is_dir => folder_size(&RealFileSystem, path, |_| true).0,
            Ok(meta) => meta.on_disk,
            Err(_) => 0,
        }
    }

    fn paths(&self) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let drive = PathBuf::from(format!("{}\\", self.drive()));
        (
            self.folders.windir.join(r"SoftwareDistribution\Download"),
            self.folders
                .windir
                .join(r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"),
            drive.join("Windows.old"),
            drive.join("hiberfil.sys"),
        )
    }
}

/// Empties the Windows Update download folder, never following a link (rule 6). A folder
/// that does not exist (fresh Windows) is nothing to empty.
fn empty_update_cache(download: &std::path::Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(download) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let _ = if kind.is_symlink() {
            Ok(()) // a link or junction is left alone
        } else if kind.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
    Ok(())
}

impl SystemCleaner for WinSystemCleaner {
    fn inspect(&self) -> CoreResult<Vec<SystemItem>> {
        let (download, delivery, old, hiberfil) = self.paths();
        let item = |task, size: Option<u64>| SystemItem {
            task,
            available: size.is_none_or(|s| s > 0),
            size_bytes: size,
        };
        Ok(vec![
            item(SystemTask::ComponentStore, None),
            item(SystemTask::UpdateCache, Some(self.size(&download))),
            item(SystemTask::DeliveryOptimization, Some(self.size(&delivery))),
            item(SystemTask::WindowsOld, Some(self.size(&old))),
            item(SystemTask::Hibernation, Some(self.size(&hiberfil))),
            item(SystemTask::RestorePoints, None),
        ])
    }

    fn run(&self, task: SystemTask, expert_mode: bool) -> CoreResult<SystemOutcome> {
        let drive = self.drive();
        let free =
            || crate::native::volume_space(Path::new(&format!("{drive}\\"))).map_or(0, |v| v.free);
        let before = free();
        let mut outcome = SystemOutcome::default();
        let steps = &mut outcome.steps;
        let restart = match task {
            SystemTask::ComponentStore => self.checked(
                "dism.exe",
                &["/Online", "/Cleanup-Image", "/StartComponentCleanup"],
                steps,
            )?,
            SystemTask::UpdateCache => {
                // Stopped cleanly, emptied, restarted even if the emptying failed.
                self.run_tool("sc.exe", &["stop", "wuauserv"], steps)?;
                self.run_tool("sc.exe", &["stop", "bits"], steps)?;
                let (download, ..) = self.paths();
                let emptied = empty_update_cache(&download);
                self.run_tool("sc.exe", &["start", "bits"], steps)?;
                self.run_tool("sc.exe", &["start", "wuauserv"], steps)?;
                emptied.map_err(|e| CoreError::from_io(download, e))?;
                false
            }
            SystemTask::DeliveryOptimization => self.checked(
                r"WindowsPowerShell\v1.0\powershell.exe",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "Delete-DeliveryOptimizationCache -Force",
                ],
                steps,
            )?,
            SystemTask::WindowsOld => {
                // Disk Cleanup, the documented way: the « Previous Installations » handler
                // is selected for the set 99, then run without a window.
                self.checked(
                    "reg.exe",
                    &[
                        "add",
                        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\Previous Installations",
                        "/v",
                        "StateFlags0099",
                        "/t",
                        "REG_DWORD",
                        "/d",
                        "2",
                        "/f",
                    ],
                    steps,
                )?;
                self.checked("cleanmgr.exe", &["/sagerun:99"], steps)?
            }
            SystemTask::Hibernation => {
                self.checked("powercfg.exe", &["/hibernate", "off"], steps)?
            }
            SystemTask::RestorePoints => {
                let list = Command::new(self.system32("vssadmin.exe"))
                    .args(["list", "shadows", &format!("/for={drive}")])
                    .output()
                    .map_err(|e| CoreError::from_io(self.system32("vssadmin.exe"), e))?;
                let shadows = parse_shadows(&String::from_utf8_lossy(&list.stdout));
                for shadow in shadows_to_delete(&shadows, Utc::now(), expert_mode) {
                    self.checked(
                        "vssadmin.exe",
                        &[
                            "delete",
                            "shadows",
                            &format!("/shadow={}", shadow.id),
                            "/quiet",
                        ],
                        steps,
                    )?;
                }
                false
            }
        };
        outcome.restart_needed = restart;
        outcome.freed_bytes = free().saturating_sub(before);
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_cache_is_emptied_and_a_missing_folder_is_nothing_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let download = dir.path().join("Download");
        assert!(empty_update_cache(&download).is_ok()); // fresh Windows: no folder
        std::fs::create_dir_all(download.join("sub")).unwrap();
        std::fs::write(download.join("a.cab"), b"x").unwrap();
        std::fs::write(download.join("sub").join("b.cab"), b"y").unwrap();
        empty_update_cache(&download).unwrap();
        assert!(download.exists());
        assert_eq!(std::fs::read_dir(&download).unwrap().count(), 0);
    }
    use chrono::TimeZone;

    #[test]
    fn restore_points_are_read_from_vssadmin_in_english_and_french() {
        let english = "Contents of shadow copy set ID: {a}\n   Contained 1 shadow copies at creation time: 10/01/2026 9:15:02 AM\n      Shadow Copy ID: {11111111-1111-1111-1111-111111111111}\n";
        let french = "Contenu de l'ID de jeu de clichés instantanés : {b}\n   1 cliché(s) instantané(s) contenu(s) à l'heure de création : 01/11/2026 09:15:02\n      ID du cliché instantané : {22222222-2222-2222-2222-222222222222}\n";
        let en = parse_shadows(english);
        assert_eq!(en.len(), 1);
        assert_eq!(en[0].id, "{11111111-1111-1111-1111-111111111111}");
        assert_eq!(
            en[0].created,
            Utc.with_ymd_and_hms(2026, 10, 1, 9, 15, 2).unwrap()
        );
        let fr = parse_shadows(&french.replace(" : ", ": "));
        assert_eq!(
            fr[0].created,
            Utc.with_ymd_and_hms(2026, 11, 1, 9, 15, 2).unwrap()
        );
    }

    /// EF-26: the most recent point and those of less than 30 days stay, except in expert mode.
    #[test]
    fn recent_restore_points_are_kept_unless_expert() {
        let now = Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap();
        let point = |id: &str, days: i64| ShadowCopy {
            id: id.into(),
            created: now - Duration::days(days),
        };
        let all = vec![point("old", 90), point("older", 120), point("recent", 10)];
        let gone: Vec<String> = shadows_to_delete(&all, now, false)
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(gone, vec!["old".to_string(), "older".to_string()]);
        let only_old = vec![point("lonely", 400)];
        assert!(
            shadows_to_delete(&only_old, now, false).is_empty(),
            "the most recent stays"
        );
        assert_eq!(shadows_to_delete(&all, now, true).len(), 3);
    }

    #[test]
    fn a_test_engine_never_cleans_windows() {
        assert!(NoSystemCleaner.inspect().is_err());
        assert!(NoSystemCleaner.run(SystemTask::Hibernation, true).is_err());
        let request: SystemRequest = serde_json::from_str(r#"{"task":"windows_old"}"#).unwrap();
        assert!(request.dry_run);
        assert!(!request.expert_mode);
    }
}
