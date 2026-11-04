//! Sensitive zones (D79, explicit decision of the owner, ENF-01 rewritten).
//!
//! The list of `safety.rs` (Windows, Program Files, ProgramData, the profile
//! folders themselves, drivers, `HKLM\SYSTEM`, `SAM`, `SECURITY`, `HARDWARE`,
//! `SOFTWARE\Microsoft`, `\Classes`, `Policies`…) no longer blocks everything:
//! it defines the **sensitive zones**. Automatic proposals keep using
//! [`SafetyRules::check_path`] and [`SafetyRules::validate_key`], so they never
//! propose a sensitive item. A **manual** action goes through [`Zones::authorize_path`]
//! or [`Zones::authorize_key`]: in a sensitive zone it needs the expert mode and the
//! word [`CONFIRM_WORD`]. Always refused, even in expert mode: malformed paths
//! (relative, `.` or `..`), a link or junction on the way (rule 6), and the root
//! of a drive or of a hive (wiping a volume: volumes are display-only, A.11).

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::fs::FileSystem;
use crate::registry::RegKeyPath;
use crate::safety::{KnownFolders, SafetyRules};

/// Word typed to act on a sensitive zone in expert mode.
pub const CONFIRM_WORD: &str = "CONFIRMER";

/// Level of precaution of a location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    /// Nothing special: the usual confirmation.
    Normal,
    /// A zone of Windows or of the programs: never proposed automatically;
    /// by hand in expert mode, with « CONFIRMER ».
    Sensitive,
}

/// What the user allowed for a manual action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManualAction {
    /// Expert mode is on in Réglages (off by default).
    pub expert_mode: bool,
    /// Word typed in the confirmation, if any.
    pub confirmation: Option<String>,
}

/// Sensitive zones of the machine.
pub struct Zones {
    // The rules without the user's exclusions: an exclusion means « never
    // propose », not « sensitive ».
    rules: SafetyRules,
    /// Folders of the machine.
    pub folders: KnownFolders,
}

impl Zones {
    /// Zones of the machine described by `folders`.
    pub fn new(folders: &KnownFolders) -> Self {
        Zones {
            rules: SafetyRules::new(folders, &[]),
            folders: folders.clone(),
        }
    }

    /// Zone of a path; malformed paths and drive roots are refused (`PROTECTED`).
    pub fn path_zone(&self, path: &Path) -> CoreResult<Zone> {
        well_formed_path(path)?;
        Ok(if self.rules.check_path(path).is_err() {
            Zone::Sensitive
        } else {
            Zone::Normal
        })
    }

    /// Zone of a registry key; malformed keys and hive roots are refused.
    pub fn key_zone(&self, key: &RegKeyPath) -> CoreResult<Zone> {
        let text = key.to_string();
        let parts: Vec<&str> = text.split('\\').collect();
        if parts.len() < 2
            || parts
                .iter()
                .any(|p| p.is_empty() || *p == "." || *p == "..")
        {
            return Err(protected(text));
        }
        Ok(if self.rules.validate_key(key).is_err() {
            Zone::Sensitive
        } else {
            Zone::Normal
        })
    }

    /// Right before a manual action on a path: well formed, no link or junction on the
    /// way, and in a sensitive zone the expert mode and « CONFIRMER ». Returns the zone.
    pub fn authorize_path(
        &self,
        fs: &dyn FileSystem,
        path: &Path,
        action: &ManualAction,
    ) -> CoreResult<Zone> {
        let zone = self.path_zone(path)?;
        // The zone first: a sensitive path says so even before the disk is read.
        gate(zone, &path.display().to_string(), action)?;
        no_link_on_the_way(fs, path)?;
        Ok(zone)
    }

    /// Right before a manual action on a registry key (see [`Zones::authorize_path`]).
    pub fn authorize_key(&self, key: &RegKeyPath, action: &ManualAction) -> CoreResult<Zone> {
        let zone = self.key_zone(key)?;
        gate(zone, &key.to_string(), action)?;
        Ok(zone)
    }
}

/// A sensitive zone needs the expert mode, then the exact word.
fn gate(zone: Zone, location: &str, action: &ManualAction) -> CoreResult<()> {
    if zone == Zone::Normal {
        return Ok(());
    }
    if !action.expert_mode {
        return Err(CoreError::ExpertRequired {
            location: location.to_owned(),
        });
    }
    if action.confirmation.as_deref().map(str::trim) != Some(CONFIRM_WORD) {
        return Err(CoreError::WordRequired {
            location: location.to_owned(),
            word: CONFIRM_WORD.to_owned(),
        });
    }
    Ok(())
}

fn protected(location: impl Into<String>) -> CoreError {
    CoreError::Protected {
        location: location.into(),
    }
}

/// Absolute, with a drive, no `.` or `..`, and below the root of the drive.
fn well_formed_path(path: &Path) -> CoreResult<()> {
    let raw = path.to_string_lossy();
    let bytes = raw.as_bytes();
    let has_drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    let bad_segment = raw.split(['\\', '/']).any(|s| s == "." || s == "..");
    let below_root = path
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
        > 0;
    if !path.is_absolute() || !has_drive || bad_segment || !below_root {
        return Err(protected(raw));
    }
    Ok(())
}

/// No reparse point (link, junction) on the way, and the path is what it says.
fn no_link_on_the_way(fs: &dyn FileSystem, path: &Path) -> CoreResult<()> {
    for step in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if step.parent().is_none() {
            continue; // the drive root
        }
        if fs.metadata(step)?.is_reparse_point {
            return Err(protected(path.display().to_string()));
        }
    }
    let canonical = fs.canonicalize(path)?;
    let same = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    if same(&canonical) != same(path) {
        return Err(protected(path.display().to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::FakeFileSystem;
    use crate::registry::Hive;

    fn zones() -> Zones {
        Zones::new(&KnownFolders::under(r"C:\"))
    }

    fn expert(word: Option<&str>) -> ManualAction {
        ManualAction {
            expert_mode: true,
            confirmation: word.map(Into::into),
        }
    }

    /// A.13: what the blacklist used to block is now a sensitive zone, for every path
    /// and key that the blacklist tests named (Windows, programs, drivers, profiles…).
    #[test]
    fn the_former_blacklist_is_now_the_sensitive_zones() {
        let z = zones();
        for path in [
            r"C:\Windows",
            r"C:\Windows\System32\drivers\etc",
            r"C:\Program Files",
            r"C:\Program Files\Common Files\Microsoft Shared",
            r"C:\Program Files (x86)",
            r"C:\ProgramData",
            r"C:\Users\Ana",
            r"C:\Program Files\WindowsPowerShell",
            r"C:\Users\Ana\AppData\Local\Microsoft",
        ] {
            assert_eq!(
                z.path_zone(Path::new(path)).unwrap(),
                Zone::Sensitive,
                "{path}"
            );
        }
        for path in [
            r"C:\Users\Ana\AppData\Roaming\OldTool",
            r"C:\Users\Ana\Videos\Vacances 2024",
            r"C:\Users\Ana\Downloads\setup.exe",
            r"D:\Jeux\Ancien",
        ] {
            assert_eq!(
                z.path_zone(Path::new(path)).unwrap(),
                Zone::Normal,
                "{path}"
            );
        }
        for (hive, key) in [
            (Hive::LocalMachine, r"SYSTEM\CurrentControlSet\Services"),
            (Hive::LocalMachine, r"SAM\SAM"),
            (Hive::LocalMachine, r"SECURITY\Policy"),
            (Hive::LocalMachine, r"HARDWARE\DESCRIPTION"),
            (Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows"),
            (Hive::CurrentUser, r"Software\Classes\.txt"),
            (Hive::CurrentUser, r"Software\Policies\Vendor"),
        ] {
            assert_eq!(
                z.key_zone(&RegKeyPath::new(hive, key)).unwrap(),
                Zone::Sensitive,
                "{key}"
            );
        }
        assert_eq!(
            z.key_zone(&RegKeyPath::new(Hive::CurrentUser, r"Software\OldTool"))
                .unwrap(),
            Zone::Normal
        );
    }

    /// A.13: a manual action on a sensitive zone is impossible outside expert mode, then
    /// needs « CONFIRMER »; a normal path only needs the usual confirmation.
    #[test]
    fn a_sensitive_zone_needs_the_expert_mode_and_the_word() {
        let z = zones();
        let mut fs = FakeFileSystem::new();
        fs.add_dir(r"C:\Windows\Temp\Old")
            .add_dir(r"C:\Users\Ana\Videos\Clip");
        let sensitive = Path::new(r"C:\Windows\Temp\Old");

        assert_eq!(
            z.authorize_path(&fs, sensitive, &ManualAction::default())
                .unwrap_err()
                .code(),
            "EXPERT_REQUIRED"
        );
        assert_eq!(
            z.authorize_path(&fs, sensitive, &expert(None))
                .unwrap_err()
                .code(),
            "CONFIRMATION_REQUIRED"
        );
        assert_eq!(
            z.authorize_path(&fs, sensitive, &expert(Some("confirmer")))
                .unwrap_err()
                .code(),
            "CONFIRMATION_REQUIRED"
        );
        assert_eq!(
            z.authorize_path(&fs, sensitive, &expert(Some(" CONFIRMER ")))
                .unwrap(),
            Zone::Sensitive
        );

        let personal = Path::new(r"C:\Users\Ana\Videos\Clip");
        assert_eq!(
            z.authorize_path(&fs, personal, &ManualAction::default())
                .unwrap(),
            Zone::Normal
        );

        let key = RegKeyPath::new(Hive::LocalMachine, r"SOFTWARE\Microsoft\Old");
        assert_eq!(
            z.authorize_key(&key, &ManualAction::default())
                .unwrap_err()
                .code(),
            "EXPERT_REQUIRED"
        );
        assert_eq!(
            z.authorize_key(&key, &expert(Some("CONFIRMER"))).unwrap(),
            Zone::Sensitive
        );
    }

    /// Never, even in expert mode: drive or hive roots, malformed paths, links.
    #[test]
    fn roots_malformed_paths_and_links_are_always_refused() {
        let z = zones();
        let mut fs = FakeFileSystem::new();
        fs.add_dir(r"C:\Data").add_reparse_point(r"C:\Data\Link");
        let all = expert(Some("CONFIRMER"));
        for path in [
            r"C:\",
            r"D:\",
            r"relative\path",
            r"C:\Data\..\Windows",
            r"C:\Data\.\x",
            r"\\server\share\x",
        ] {
            assert_eq!(
                z.authorize_path(&fs, Path::new(path), &all)
                    .unwrap_err()
                    .code(),
                "PROTECTED",
                "{path}"
            );
        }
        assert_eq!(
            z.authorize_path(&fs, Path::new(r"C:\Data\Link"), &all)
                .unwrap_err()
                .code(),
            "PROTECTED"
        );
        assert_eq!(
            z.authorize_key(&RegKeyPath::new(Hive::LocalMachine, ""), &all)
                .unwrap_err()
                .code(),
            "PROTECTED"
        );
    }
}
