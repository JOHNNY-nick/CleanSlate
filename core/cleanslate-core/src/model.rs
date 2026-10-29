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
}

/// Kind of installed application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// Classic Win32 application, can be uninstalled by CleanSlate.
    Win32,
    /// Microsoft Store application, listed read-only (EF-04).
    Store,
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
}

impl CleanPlan {
    /// Creates a plan in simulation mode (`dry_run = true`).
    pub fn new(item_ids: Vec<String>, origin: Origin) -> Self {
        CleanPlan {
            item_ids,
            dry_run: true,
            origin,
        }
    }
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

/// An item stored in quarantine.
///
/// Provisional: the full manifest entry (SHA-256, ACL…) is defined in J4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantinedItem {
    /// Identifier, unique within the session.
    pub id: String,
    /// Kind of item.
    pub kind: LeftoverKind,
    /// Original path or registry key.
    pub original_location: String,
    /// Where the item is stored in quarantine.
    pub quarantine_path: PathBuf,
    /// Size, in bytes.
    pub size_bytes: u64,
    /// `true` once the item has been restored.
    pub restored: bool,
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
                restored: false,
            }],
            freed_bytes: 512,
            errors: vec![],
            status: SessionStatus::Completed,
        };
        let json = round_trip(&session);
        assert!(json.contains(r#""created_at":"2026-10-22T14:05:00Z""#));
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
