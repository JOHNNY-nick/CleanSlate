//! Shared data types (specifications § 5).
//!
//! These types are serialized to JSON (`snake_case`) to cross the FFI
//! boundary; the C# side mirrors them as immutable records.

use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::CoreError;

/// Registry view an application was found in (EF-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryView {
    /// `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall` (64-bit view).
    Hklm64,
    /// `HKLM\SOFTWARE\WOW6432Node\…\Uninstall` (32-bit view).
    Hklm32,
    /// `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`.
    Hkcu,
    /// Not in the registry: a package of the current user (Microsoft Store, EF-04).
    Package,
}

/// Kind of installed application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// Classic Win32 application, can be uninstalled by CleanSlate.
    Win32,
    /// Microsoft Store application (MSIX package of the current user, EF-04).
    Store,
}

/// Why an application is hidden by default in the list (EF-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenReason {
    /// `SystemComponent = 1`.
    SystemComponent,
    /// Update, hotfix or service pack of another product.
    Update,
}

/// Technology of the installer, deduced from the Uninstall entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallerKind {
    /// Windows Installer (MSI).
    Msi,
    /// Inno Setup (sub-key ending with `_is1`).
    InnoSetup,
    /// Nullsoft Scriptable Install System.
    Nsis,
    /// MSIX / AppX package (Microsoft Store).
    Msix,
    /// Anything else.
    Unknown,
}

/// An application listed in one of the Uninstall registry keys (EF-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledApp {
    /// Stable hash of the registry view and the Uninstall sub-key name.
    pub id: String,
    /// `DisplayName`.
    pub display_name: String,
    /// `Publisher`.
    pub publisher: Option<String>,
    /// `DisplayVersion`.
    pub version: Option<String>,
    /// `InstallDate` (`YYYYMMDD` in the registry).
    pub install_date: Option<NaiveDate>,
    /// `InstallLocation`.
    pub install_location: Option<PathBuf>,
    /// `EstimatedSize`, in KiB.
    pub estimated_size_kb: Option<u64>,
    /// `UninstallString`.
    pub uninstall_string: Option<String>,
    /// `QuietUninstallString`.
    pub quiet_uninstall_string: Option<String>,
    /// `WindowsInstaller = 1` or the sub-key name is a product code GUID.
    pub is_msi: bool,
    /// MSI product code (`{GUID}`).
    pub product_code: Option<String>,
    /// Registry view the entry was read from.
    pub registry_view: RegistryView,
    /// Win32 or Store.
    pub kind: AppKind,
    /// Set when the entry is hidden by default (EF-02); `None` means visible.
    pub hidden_reason: Option<HiddenReason>,
    /// Installer technology.
    pub installer: InstallerKind,
    /// `PackageFullName` of a Store application.
    #[serde(default)]
    pub package_full_name: Option<String>,
    /// `PackageFamilyName` of a Store application.
    #[serde(default)]
    pub package_family_name: Option<String>,
}

/// Kind of leftover item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftoverKind {
    /// A folder and everything below it.
    Folder,
    /// A single file.
    File,
    /// A registry key and all its sub-keys.
    RegistryKey,
    /// A single registry value.
    RegistryValue,
    /// A `.lnk` shortcut whose target is gone.
    Shortcut,
    /// A scheduled task whose executable is gone.
    ScheduledTask,
    /// A service whose executable is gone.
    Service,
}

/// Confidence that an item really is a leftover (EF-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Score ≥ 60, pre-checked in the UI.
    High,
    /// Score 35 to 59.
    Medium,
    /// Score < 35.
    Low,
}

/// One item proposed for cleaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leftover {
    /// Identifier, unique within a scan report.
    pub id: String,
    /// Kind of item.
    pub kind: LeftoverKind,
    /// Full path or registry key (`HKCU\Software\…`).
    pub location: String,
    /// Total size, in bytes.
    pub size_bytes: u64,
    /// Most recent modification found below the item.
    pub last_modified: Option<DateTime<Utc>>,
    /// Confidence level.
    pub confidence: Confidence,
    /// Human-readable reasons, shown to the user.
    pub reasons: Vec<String>,
    /// Application this item probably belonged to.
    pub guessed_app: Option<String>,
}

/// Leftovers grouped by guessed application (EF-09).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftoverGroup {
    /// Group title, e.g. "Audacity".
    pub label: String,
    /// Common publisher, if known.
    pub publisher: Option<String>,
    /// Items of the group.
    pub items: Vec<Leftover>,
    /// Sum of `size_bytes` of the items.
    pub total_bytes: u64,
}

/// Error on one item; it never stops the whole operation (ENF-09).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemError {
    /// Stable code, see [`CoreError::code`].
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Path or registry key concerned.
    pub path: Option<String>,
}

impl From<&CoreError> for ItemError {
    fn from(err: &CoreError) -> Self {
        ItemError {
            code: err.code().to_owned(),
            message: err.to_string(),
            path: err.location(),
        }
    }
}

/// Result of a scan (orphans or leftovers of one application).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    /// Leftovers grouped by guessed application.
    pub groups: Vec<LeftoverGroup>,
    /// Items that could not be read.
    pub errors: Vec<ItemError>,
    /// Scan duration, in milliseconds.
    pub duration_ms: u64,
    /// `true` when the scan was cancelled: results are partial.
    pub cancelled: bool,
}

/// Options of a scan of orphan leftovers (`cs_scan_orphans`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Simulation: a scan never modifies anything; kept explicit and `true`
    /// by default like every operation (safety rule 5).
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// Also scan the registry (EF-06).
    #[serde(default = "default_true")]
    pub include_registry: bool,
    /// Also look for dead shortcuts (EF-07).
    #[serde(default = "default_true")]
    pub include_shortcuts: bool,
    /// Also look for scheduled tasks and services whose program is missing
    /// (EF-07); detection only, they are never quarantined.
    #[serde(default = "default_true")]
    pub include_tasks_and_services: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            dry_run: true,
            include_registry: true,
            include_shortcuts: true,
            include_tasks_and_services: true,
        }
    }
}

/// Phase of a scan, reported with the progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    /// Reading the inventory and listing candidates.
    Listing,
    /// Measuring and scoring candidates.
    Analyzing,
    /// Finished (or cancelled).
    Done,
}

/// Progress event sent at most once per 250 ms, and at least once per
/// 250 ms while work advances (ENF-06).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    /// Current phase.
    pub phase: ScanPhase,
    /// Candidates analyzed so far.
    pub current: u64,
    /// Total number of candidates (0 while listing).
    pub total: u64,
    /// Folder or key being processed.
    pub path: String,
}

/// What started a cleaning session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// Scan of orphan leftovers, without an application name.
    Orphans,
    /// Full uninstallation of one application.
    App {
        /// [`InstalledApp::id`] of the application.
        app_id: String,
    },
}

fn default_true() -> bool {
    true
}

/// Items the user chose to move to quarantine (EF-16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanPlan {
    /// [`Leftover::id`] of the selected items.
    pub item_ids: Vec<String>,
    /// Simulation: list what would be done without touching anything.
    /// `true` when absent from the JSON (safety rule 5).
    #[serde(default = "default_true")]
    pub dry_run: bool,
    /// What started the session.
    pub origin: Origin,
    /// Word typed by the user above 20 GB or 50 000 files (« QUARANTAINE »).
    #[serde(default)]
    pub confirmation: Option<String>,
}

impl CleanPlan {
    /// Creates a plan in simulation mode (`dry_run = true`).
    pub fn new(item_ids: Vec<String>, origin: Origin) -> Self {
        CleanPlan {
            item_ids,
            dry_run: true,
            origin,
            confirmation: None,
        }
    }
}

fn default_false() -> bool {
    false
}

/// Request of `cs_uninstall` (EF-12 to EF-15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallRequest {
    /// [`InstalledApp::id`] of the application.
    pub app_id: String,
    /// Silent mode (`QuietUninstallString`, `/qn` for MSI) when available (EF-14).
    #[serde(default = "default_false")]
    pub quiet: bool,
    /// Create a system restore point first (EF-15), on by default.
    #[serde(default = "default_true")]
    pub create_restore_point: bool,
    /// Simulation (safety rule 5): `true` when absent.
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

/// Result of `cs_uninstall`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallResult {
    /// Application id.
    pub app_id: String,
    /// Application name.
    pub display_name: String,
    /// Command line run (or that would be run).
    pub command: String,
    /// Nothing was run.
    pub simulated: bool,
    /// A restore point was created.
    pub restore_point_created: bool,
    /// Why the restore point failed, if it did (never blocking).
    pub restore_point_error: Option<String>,
    /// Exit code of the uninstaller.
    pub exit_code: Option<u32>,
    /// The 30-minute limit was reached; the uninstaller may still run.
    pub timed_out: bool,
    /// The wait was cancelled by the user.
    pub cancelled: bool,
    /// The Uninstall entry is still present (uninstaller cancelled or failed).
    pub still_installed: bool,
}

/// Step of an uninstall shown by the wizard (EF-21, design § 4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallStep {
    /// Creating the restore point (EF-15).
    RestorePoint,
    /// Starting the official uninstaller.
    Launching,
    /// Waiting for the uninstaller and its whole process tree.
    Waiting,
    /// Checking that the application is gone.
    Verifying,
    /// Finished.
    Done,
}

/// One process of the uninstaller's Job Object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobProcess {
    /// Process id.
    pub pid: u32,
    /// Executable name (`unins000.exe`).
    pub name: String,
}

/// Progress event of `cs_uninstall`: sent at each step change and at most
/// every 250 ms while waiting (ENF-06). `percent` and `action` are real
/// values from Windows Installer or the Store; for `.exe` uninstallers they
/// stay empty (no invented percentage) and `processes` lists the Job Object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UninstallProgress {
    /// Current step.
    pub step: UninstallStep,
    /// Milliseconds since the uninstall started.
    pub elapsed_ms: u64,
    /// 0 to 100, when known.
    pub percent: Option<f64>,
    /// Action in progress (MSI `ActionStart` description), when known.
    pub action: Option<String>,
    /// Processes still running in the Job Object.
    pub processes: Vec<JobProcess>,
}
/// Request of `cs_scan_app_leftovers` (EF-13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppLeftoversRequest {
    /// [`InstalledApp::id`] of the (uninstalled) application.
    pub app_id: String,
    /// The application is still listed but its uninstaller program is
    /// missing (design § 5, D45): leftovers are searched anyway, never with
    /// a confidence above Medium. Refused if the uninstaller exists.
    #[serde(default)]
    pub uninstaller_missing: bool,
}

/// Request of `cs_restore` (EF-18).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreRequest {
    /// Session to restore.
    pub session_id: Uuid,
    /// Items to restore; `None` means all.
    #[serde(default)]
    pub item_ids: Option<Vec<String>>,
    /// Simulation (safety rule 5): `true` when absent.
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

/// Request of `cs_purge_quarantine` (EF-19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeRequest {
    /// Sessions older than this are deleted for good.
    pub older_than_days: u32,
    /// Simulation (safety rule 5): `true` when absent.
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

/// Result of `cs_purge_quarantine`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurgeResult {
    /// Bytes deleted for good (or that would be, in simulation).
    pub freed_bytes: u64,
    /// `true` when nothing was deleted.
    pub simulated: bool,
}

/// State of a cleaning session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Every item was moved to quarantine.
    Completed,
    /// Some items failed; see [`Session::errors`].
    Partial,
    /// The session was restored.
    Restored,
    /// The quarantine of the session was purged.
    Purged,
}

/// State of one quarantined item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineStatus {
    /// Stored in quarantine, restorable.
    Quarantined,
    /// Put back at its original location.
    Restored,
    /// Deleted for good by the purge.
    Purged,
    /// Locked file: will be moved at the next reboot.
    PendingReboot,
    /// Could not be quarantined; see [`QuarantinedItem::error`].
    Failed,
}

/// An item stored in quarantine (also one entry of `manifest.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantinedItem {
    /// Identifier, unique within the session (= [`Leftover::id`]).
    pub id: String,
    /// Kind of item.
    pub kind: LeftoverKind,
    /// Original path or registry key.
    pub original_location: String,
    /// Where the item is stored in quarantine.
    pub quarantine_path: PathBuf,
    /// Size, in bytes.
    pub size_bytes: u64,
    /// File: its SHA-256. Folder: SHA-256 of the sorted list of
    /// (relative path, SHA-256) of its files. Hex, lower case.
    pub sha256: Option<String>,
    /// Original security descriptor, SDDL format.
    pub original_acl: Option<String>,
    /// When the item was moved to quarantine.
    pub quarantined_at: DateTime<Utc>,
    /// When the item was restored, if it was.
    pub restored_at: Option<DateTime<Utc>>,
    /// Current state.
    pub status: QuarantineStatus,
    /// Set when `status` is [`QuarantineStatus::Failed`].
    pub error: Option<ItemError>,
}

/// A cleaning session, kept in the history (EF-20).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Session identifier (also the quarantine folder name).
    pub id: Uuid,
    /// Creation date.
    pub created_at: DateTime<Utc>,
    /// What started the session.
    pub origin: Origin,
    /// Items moved to quarantine.
    pub items: Vec<QuarantinedItem>,
    /// Space freed, in bytes.
    pub freed_bytes: u64,
    /// Items that failed.
    pub errors: Vec<ItemError>,
    /// Current state.
    pub status: SessionStatus,
    /// `true` for a simulation: nothing was moved and nothing is saved.
    #[serde(default)]
    pub simulated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde::de::DeserializeOwned;
    use std::fmt::Debug;

    /// Serializes `value`, parses it back and checks nothing was lost.
    fn round_trip<T>(value: &T) -> String
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_string(value).unwrap();
        let back: T = serde_json::from_str(&json).unwrap();
        assert_eq!(&back, value);
        json
    }

    fn sample_app() -> InstalledApp {
        InstalledApp {
            id: "a1b2".into(),
            display_name: "GIMP 2.10.38".into(),
            publisher: Some("The GIMP Team".into()),
            version: Some("2.10.38".into()),
            install_date: NaiveDate::from_ymd_opt(2026, 10, 22),
            install_location: Some(PathBuf::from(r"C:\Program Files\GIMP 2")),
            estimated_size_kb: Some(421_888),
            uninstall_string: Some(r#""C:\Program Files\GIMP 2\uninst\unins000.exe""#.into()),
            quiet_uninstall_string: None,
            is_msi: false,
            product_code: None,
            registry_view: RegistryView::Hklm64,
            kind: AppKind::Win32,
            hidden_reason: Some(HiddenReason::SystemComponent),
            installer: InstallerKind::InnoSetup,
            package_full_name: None,
            package_family_name: None,
        }
    }

    fn sample_leftover() -> Leftover {
        Leftover {
            id: "l1".into(),
            kind: LeftoverKind::Folder,
            location: r"C:\Users\Ana\AppData\Roaming\audacity".into(),
            size_bytes: 388 * 1024 * 1024,
            last_modified: Some(Utc.with_ymd_and_hms(2026, 3, 1, 10, 0, 0).unwrap()),
            confidence: Confidence::High,
            reasons: vec!["Aucune application de cet éditeur installée".into()],
            guessed_app: Some("Audacity".into()),
        }
    }

    #[test]
    fn installed_app_round_trip_uses_snake_case() {
        let json = round_trip(&sample_app());
        assert!(json.contains(r#""display_name":"GIMP 2.10.38""#));
        assert!(json.contains(r#""registry_view":"hklm64""#));
        assert!(json.contains(r#""kind":"win32""#));
        assert!(json.contains(r#""install_date":"2026-10-22""#));
        assert!(json.contains(r#""hidden_reason":"system_component""#));
        assert!(json.contains(r#""installer":"inno_setup""#));
    }

    #[test]
    fn scan_report_round_trip() {
        let report = ScanReport {
            groups: vec![LeftoverGroup {
                label: "Audacity".into(),
                publisher: None,
                total_bytes: 388 * 1024 * 1024,
                items: vec![sample_leftover()],
            }],
            errors: vec![ItemError {
                code: "ACCESS_DENIED".into(),
                message: "access denied".into(),
                path: Some(r"C:\ProgramData\X".into()),
            }],
            duration_ms: 1234,
            cancelled: false,
        };
        let json = round_trip(&report);
        assert!(json.contains(r#""confidence":"high""#));
        assert!(json.contains(r#""kind":"folder""#));
    }

    #[test]
    fn every_enum_variant_round_trips() {
        for kind in [
            LeftoverKind::Folder,
            LeftoverKind::File,
            LeftoverKind::RegistryKey,
            LeftoverKind::RegistryValue,
            LeftoverKind::Shortcut,
            LeftoverKind::ScheduledTask,
            LeftoverKind::Service,
        ] {
            round_trip(&kind);
        }
        for c in [Confidence::High, Confidence::Medium, Confidence::Low] {
            round_trip(&c);
        }
        for v in [
            RegistryView::Hklm64,
            RegistryView::Hklm32,
            RegistryView::Hkcu,
        ] {
            round_trip(&v);
        }
        for s in [
            SessionStatus::Completed,
            SessionStatus::Partial,
            SessionStatus::Restored,
            SessionStatus::Purged,
        ] {
            round_trip(&s);
        }
        assert_eq!(
            serde_json::to_string(&LeftoverKind::ScheduledTask).unwrap(),
            r#""scheduled_task""#
        );
    }

    #[test]
    fn origin_is_tagged() {
        assert_eq!(round_trip(&Origin::Orphans), r#"{"kind":"orphans"}"#);
        assert_eq!(
            round_trip(&Origin::App { app_id: "x".into() }),
            r#"{"kind":"app","app_id":"x"}"#
        );
    }

    #[test]
    fn clean_plan_defaults_to_dry_run() {
        let plan: CleanPlan =
            serde_json::from_str(r#"{"item_ids":["a"],"origin":{"kind":"orphans"}}"#).unwrap();
        assert!(plan.dry_run);
        assert!(CleanPlan::new(vec![], Origin::Orphans).dry_run);
        round_trip(&plan);
    }

    #[test]
    fn session_round_trip() {
        let session = Session {
            id: Uuid::new_v4(),
            created_at: Utc.with_ymd_and_hms(2026, 10, 22, 14, 5, 0).unwrap(),
            origin: Origin::App {
                app_id: "a1b2".into(),
            },
            items: vec![QuarantinedItem {
                id: "l1".into(),
                kind: LeftoverKind::RegistryKey,
                original_location: r"HKCU\Software\Audacity".into(),
                quarantine_path: PathBuf::from(r"C:\q\l1.reg"),
                size_bytes: 512,
                sha256: Some("ab".repeat(32)),
                original_acl: Some("O:BAG:SYD:(A;;KA;;;SY)".into()),
                quarantined_at: Utc.with_ymd_and_hms(2026, 10, 22, 14, 5, 1).unwrap(),
                restored_at: None,
                status: QuarantineStatus::PendingReboot,
                error: None,
            }],
            freed_bytes: 512,
            errors: vec![],
            status: SessionStatus::Completed,
            simulated: false,
        };
        let json = round_trip(&session);
        assert!(json.contains(r#""created_at":"2026-10-22T14:05:00Z""#));
        assert!(json.contains(r#""status":"pending_reboot""#));
    }

    #[test]
    fn item_error_from_core_error() {
        let err = CoreError::AccessDenied {
            location: r"C:\X".into(),
        };
        let item = ItemError::from(&err);
        assert_eq!(item.code, "ACCESS_DENIED");
        assert_eq!(item.path.as_deref(), Some(r"C:\X"));
    }
}
