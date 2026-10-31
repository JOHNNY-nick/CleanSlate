//! Inventory of installed applications (EF-01, EF-02).
//!
//! Reads the three Uninstall keys **read-only** through the [`Registry`]
//! trait and turns each named entry into an [`InstalledApp`]. Nothing is
//! filtered out except nameless entries: hidden ones (system components,
//! updates) carry a [`HiddenReason`] and the UI decides whether to show them.

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::error::{CoreError, CoreResult};
use crate::model::{AppKind, Caution, HiddenReason, InstalledApp, InstallerKind, RegistryView};
use crate::registry::{Hive, KeyView, RegData, RegKeyPath, Registry};

/// Uninstall key of 64-bit applications (and of HKCU).
pub const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

/// Uninstall key of 32-bit applications, read through its explicit
/// `WOW6432Node` path so that it also works in the test registry.
pub const UNINSTALL_KEY_WOW64: &str =
    r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

/// `ReleaseType` values that mark an update rather than an application.
const UPDATE_RELEASE_TYPES: [&str; 5] = [
    "update",
    "hotfix",
    "security update",
    "service pack",
    "update rollup",
];

/// The three registry keys listed by EF-01, with the view they stand for.
pub fn uninstall_sources() -> [(RegistryView, RegKeyPath); 3] {
    [
        (
            RegistryView::Hklm64,
            RegKeyPath::new(Hive::LocalMachine, UNINSTALL_KEY).with_view(KeyView::Force64),
        ),
        (
            RegistryView::Hklm32,
            RegKeyPath::new(Hive::LocalMachine, UNINSTALL_KEY_WOW64).with_view(KeyView::Force64),
        ),
        (
            RegistryView::Hkcu,
            RegKeyPath::new(Hive::CurrentUser, UNINSTALL_KEY),
        ),
    ]
}

/// Lists every named application of the three Uninstall keys, sorted by name.
///
/// A missing key (HKCU often has none) or an unreadable entry is logged and
/// skipped: it never stops the inventory (ENF-09).
pub fn list_apps(registry: &dyn Registry) -> CoreResult<Vec<InstalledApp>> {
    let mut apps = Vec::new();
    for (view, root) in uninstall_sources() {
        let names = match registry.subkeys(&root) {
            Ok(names) => names,
            Err(CoreError::NotFound { .. }) => continue,
            Err(err) => {
                tracing::warn!(key = %root, %err, "Uninstall key unreadable");
                continue;
            }
        };
        for name in names {
            let key = root.join(&name);
            match registry.values(&key) {
                Ok(values) => apps.extend(parse_entry(view, &name, &values)),
                Err(err) => tracing::warn!(%key, %err, "Uninstall entry unreadable"),
            }
        }
    }
    apps.sort_by_cached_key(|app| app.display_name.to_lowercase());
    tracing::info!(count = apps.len(), "inventory done");
    Ok(apps)
}

/// Turns the values of one Uninstall sub-key into an application.
///
/// Returns `None` for entries without a `DisplayName` (EF-02).
pub fn parse_entry(
    view: RegistryView,
    subkey: &str,
    values: &[(String, RegData)],
) -> Option<InstalledApp> {
    let get = |name: &str| {
        values
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    };
    let text = |name: &str| {
        get(name)
            .and_then(RegData::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let number = |name: &str| get(name).and_then(RegData::as_u64);

    let display_name = text("DisplayName")?;
    let uninstall_string = text("UninstallString");
    let is_guid_key = is_guid(subkey);
    let mentions_msiexec = uninstall_string
        .as_deref()
        .is_some_and(|s| s.to_ascii_lowercase().contains("msiexec"));
    // A GUID sub-key alone is not proof of MSI (some EXE installers use one):
    // msiexec must also appear, so that J5 never runs `msiexec /x` wrongly.
    let is_msi = number("WindowsInstaller") == Some(1) || (is_guid_key && mentions_msiexec);
    let product_code = (is_msi && is_guid_key).then(|| subkey.to_owned());

    let hidden_reason = if number("SystemComponent") == Some(1) {
        Some(HiddenReason::SystemComponent)
    } else if is_update(
        &display_name,
        text("ParentKeyName").as_deref(),
        text("ReleaseType").as_deref(),
    ) {
        Some(HiddenReason::Update)
    } else {
        None
    };

    let publisher = text("Publisher");
    let caution = caution_for(&display_name, publisher.as_deref());

    Some(InstalledApp {
        id: stable_id(view, subkey),
        installer: installer_kind(is_msi, subkey, uninstall_string.as_deref()),
        display_name,
        publisher,
        version: text("DisplayVersion"),
        install_date: text("InstallDate").as_deref().and_then(parse_install_date),
        install_location: text("InstallLocation")
            .map(|s| s.trim_matches('"').trim().to_owned())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from),
        estimated_size_kb: number("EstimatedSize"),
        uninstall_string,
        quiet_uninstall_string: text("QuietUninstallString"),
        is_msi,
        product_code,
        registry_view: view,
        kind: AppKind::Win32,
        hidden_reason,
        package_full_name: None,
        package_family_name: None,
        caution,
    })
}

/// Words of a security suite, a driver and a runtime, matched as whole words.
const SECURITY_WORDS: &[&str] = &[
    "antivirus",
    "anti virus",
    "antimalware",
    "anti malware",
    "security",
    "endpoint",
    "defender",
    "firewall",
    "bitdefender",
    "kaspersky",
    "norton",
    "eset",
    "avast",
    "avg",
    "mcafee",
    "malwarebytes",
    "sophos",
    "trend micro",
    "f secure",
    "webroot",
];
const DRIVER_WORDS: &[&str] = &[
    "driver",
    "drivers",
    "pilote",
    "pilotes",
    "chipset",
    "firmware",
    "management engine",
];
const RUNTIME_WORDS: &[&str] = &[
    "runtime",
    "redistributable",
    "vcredist",
    "visual c",
    "net framework",
    "directx",
    "java",
    "jre",
    "jdk",
    "openjdk",
    "webview2",
    "hosting bundle",
];

/// Why uninstalling this application deserves a warning (D65), from its name
/// and publisher: a security suite, a driver, a runtime other programs need,
/// or anything else published by Microsoft. `None` for an ordinary application.
pub fn caution_for(display_name: &str, publisher: Option<&str>) -> Option<Caution> {
    // Lower case, punctuation as spaces, padded: « Intel(R) Chipset » -> « intel r chipset ».
    let words = |text: &str| {
        let cleaned: String = text
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect();
        format!(
            " {} ",
            cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
        )
    };
    let name = words(display_name);
    let has = |list: &[&str]| list.iter().any(|w| name.contains(&format!(" {w} ")));
    if has(SECURITY_WORDS) {
        Some(Caution::Security)
    } else if has(DRIVER_WORDS) {
        Some(Caution::Driver)
    } else if has(RUNTIME_WORDS) {
        Some(Caution::Runtime)
    } else if publisher.is_some_and(|p| words(p).starts_with(" microsoft ")) {
        Some(Caution::Microsoft)
    } else {
        None
    }
}

/// Stable identifier: FNV-1a 64-bit of the view and the lower-cased sub-key,
/// in hexadecimal. Written by hand because `std`'s hasher may change between
/// Rust versions, and ids must survive upgrades (history, exclusions).
pub fn stable_id(view: RegistryView, subkey: &str) -> String {
    let tag = match view {
        RegistryView::Hklm64 => "hklm64",
        RegistryView::Hklm32 => "hklm32",
        RegistryView::Hkcu => "hkcu",
        RegistryView::Package => "package",
    };
    fnv1a_hex(&format!("{tag}|{}", subkey.to_lowercase()))
}

/// FNV-1a 64-bit of `text`, as 16 hexadecimal digits (stable across Rust
/// versions; also used for leftover ids).
pub fn fnv1a_hex(text: &str) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;
    let hash = text
        .bytes()
        .fold(OFFSET, |h, b| (h ^ u64::from(b)).wrapping_mul(PRIME));
    format!("{hash:016x}")
}

/// Parses `InstallDate`: `YYYYMMDD` (standard) or `YYYY-MM-DD`.
pub fn parse_install_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    NaiveDate::parse_from_str(raw, "%Y%m%d")
        .or_else(|_| NaiveDate::parse_from_str(raw, "%Y-%m-%d"))
        .ok()
        .filter(|_| raw.len() == 8 || raw.len() == 10)
}

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`.
pub fn is_guid(s: &str) -> bool {
    let Some(inner) = s.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return false;
    };
    let groups: Vec<&str> = inner.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `true` for updates, hotfixes and service packs (hidden by default).
fn is_update(name: &str, parent_key: Option<&str>, release_type: Option<&str>) -> bool {
    if parent_key.is_some() {
        return true;
    }
    if release_type.is_some_and(|t| {
        UPDATE_RELEASE_TYPES
            .iter()
            .any(|u| t.eq_ignore_ascii_case(u))
    }) {
        return true;
    }
    contains_kb_number(name)
}

/// Detects a Windows update reference such as `(KB5034441)`.
fn contains_kb_number(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.match_indices("KB").any(|(i, _)| {
        let preceded_ok = i == 0 || !upper.as_bytes()[i - 1].is_ascii_alphanumeric();
        let digits = upper[i + 2..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        preceded_ok && digits >= 6
    })
}

/// Deduces the installer technology (best effort, display only).
fn installer_kind(is_msi: bool, subkey: &str, uninstall: Option<&str>) -> InstallerKind {
    if is_msi {
        return InstallerKind::Msi;
    }
    if subkey.to_ascii_lowercase().ends_with("_is1") {
        return InstallerKind::InnoSetup;
    }
    match uninstall.and_then(executable_name).as_deref() {
        Some(exe) if exe.starts_with("unins0") => InstallerKind::InnoSetup,
        Some("uninst.exe" | "uninstall.exe") => InstallerKind::Nsis,
        _ => InstallerKind::Unknown,
    }
}

/// Lower-cased file name of the executable of a command line.
fn executable_name(command: &str) -> Option<String> {
    let command = command.trim();
    let path = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        let lower = command.to_ascii_lowercase();
        let end = lower.find(".exe").map_or(command.len(), |i| i + 4);
        &command[..end]
    };
    let name = path.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::TestRegistryKey;

    fn s(v: &str) -> RegData {
        RegData::String(v.into())
    }

    fn entry(view: RegistryView, subkey: &str, values: &[(&str, RegData)]) -> Option<InstalledApp> {
        let values: Vec<(String, RegData)> = values
            .iter()
            .map(|(n, v)| ((*n).to_owned(), v.clone()))
            .collect();
        parse_entry(view, subkey, &values)
    }

    /// D65: applications other programs or Windows depend on get a warning before uninstall.
    #[test]
    fn important_applications_get_a_caution() {
        let ms = Some("Microsoft Corporation");
        let cases: [(&str, Option<&str>, Option<Caution>); 16] = [
            ("PowerShell 7-x64", ms, Some(Caution::Microsoft)),
            (
                "Microsoft Edge WebView2 Runtime",
                ms,
                Some(Caution::Runtime),
            ),
            (
                "Microsoft Visual C++ 2015-2022 Redistributable (x64) - 14.40.33810",
                ms,
                Some(Caution::Runtime),
            ),
            (
                "Microsoft Windows Desktop Runtime - 8.0.11 (x64)",
                ms,
                Some(Caution::Runtime),
            ),
            (
                "Java 8 Update 431",
                Some("Oracle Corporation"),
                Some(Caution::Runtime),
            ),
            (
                "NVIDIA Graphics Driver 566.36",
                Some("NVIDIA Corporation"),
                Some(Caution::Driver),
            ),
            (
                "Intel(R) Chipset Device Software",
                Some("Intel(R) Corporation"),
                Some(Caution::Driver),
            ),
            (
                "Intel® Management Engine Components",
                Some("Intel Corporation"),
                Some(Caution::Driver),
            ),
            (
                "Pilote audio Realtek",
                Some("Realtek"),
                Some(Caution::Driver),
            ),
            (
                "Bitdefender Total Security",
                Some("Bitdefender"),
                Some(Caution::Security),
            ),
            (
                "Malwarebytes version 5.2",
                Some("Malwarebytes"),
                Some(Caution::Security),
            ),
            (
                "ESET Security",
                Some("ESET, spol. s r.o."),
                Some(Caution::Security),
            ),
            (
                "Microsoft Defender for Endpoint",
                ms,
                Some(Caution::Security),
            ),
            ("VLC media player", Some("VideoLAN"), None),
            ("Audacity 3.7", Some("Audacity Team"), None),
            ("GIMP 3.2.6", None, None),
        ];
        for (name, publisher, expected) in cases {
            assert_eq!(caution_for(name, publisher), expected, "{name}");
        }
        // Whole words only: « avg » is not in « Navigateur », « eset » not in « Reset ».
        assert_eq!(caution_for("Navigateur Reset Tool", Some("Example")), None);
    }

    #[test]
    fn inno_setup_entry() {
        let app = entry(
            RegistryView::Hklm64,
            "GIMP-2_is1",
            &[
                ("DisplayName", s("GIMP 2.10.38")),
                ("Publisher", s("The GIMP Team")),
                ("DisplayVersion", s("2.10.38")),
                ("InstallDate", s("20250618")),
                ("InstallLocation", s(r#""C:\Program Files\GIMP 2\""#)),
                ("EstimatedSize", RegData::Dword(421_888)),
                (
                    "UninstallString",
                    s(r#""C:\Program Files\GIMP 2\uninst\unins000.exe""#),
                ),
                (
                    "QuietUninstallString",
                    s(r#""C:\Program Files\GIMP 2\uninst\unins000.exe" /SILENT"#),
                ),
            ],
        )
        .unwrap();
        assert_eq!(app.display_name, "GIMP 2.10.38");
        assert_eq!(app.installer, InstallerKind::InnoSetup);
        assert_eq!(app.install_date, NaiveDate::from_ymd_opt(2025, 6, 18));
        assert_eq!(
            app.install_location,
            Some(PathBuf::from(r"C:\Program Files\GIMP 2\"))
        );
        assert_eq!(app.estimated_size_kb, Some(421_888));
        assert!(!app.is_msi);
        assert_eq!(app.product_code, None);
        assert_eq!(app.hidden_reason, None);
        assert_eq!(app.kind, AppKind::Win32);
        assert!(app.quiet_uninstall_string.is_some());
    }

    #[test]
    fn msi_entry() {
        let guid = "{23170F69-40C1-2702-2408-000001000000}";
        let app = entry(
            RegistryView::Hklm64,
            guid,
            &[
                ("DisplayName", s("7-Zip 24.08 (x64 edition)")),
                ("WindowsInstaller", RegData::Dword(1)),
                ("UninstallString", s(&format!("MsiExec.exe /I{guid}"))),
            ],
        )
        .unwrap();
        assert!(app.is_msi);
        assert_eq!(app.product_code.as_deref(), Some(guid));
        assert_eq!(app.installer, InstallerKind::Msi);
    }

    #[test]
    fn guid_key_without_msi_markers_is_not_msi() {
        let app = entry(
            RegistryView::Hklm64,
            "{AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE}",
            &[
                ("DisplayName", s("Some EXE app")),
                ("UninstallString", s(r"C:\Apps\Some\setup.exe /remove")),
            ],
        )
        .unwrap();
        assert!(!app.is_msi);
        assert_eq!(app.product_code, None);
        assert_eq!(app.installer, InstallerKind::Unknown);
    }

    #[test]
    fn nsis_entry_with_unquoted_path() {
        let app = entry(
            RegistryView::Hklm32,
            "Notepad++",
            &[
                ("DisplayName", s("Notepad++ (64-bit x64)")),
                (
                    "UninstallString",
                    s(r"C:\Program Files\Notepad++\uninstall.exe"),
                ),
                ("InstallLocation", s("   ")),
            ],
        )
        .unwrap();
        assert_eq!(app.installer, InstallerKind::Nsis);
        assert_eq!(app.install_location, None);
        assert_eq!(app.registry_view, RegistryView::Hklm32);
    }

    #[test]
    fn hidden_entries_are_flagged_not_dropped() {
        let system = entry(
            RegistryView::Hklm64,
            "Comp",
            &[
                ("DisplayName", s("Runtime")),
                ("SystemComponent", RegData::Dword(1)),
            ],
        )
        .unwrap();
        assert_eq!(system.hidden_reason, Some(HiddenReason::SystemComponent));

        let child = entry(
            RegistryView::Hklm64,
            "U1",
            &[("DisplayName", s("Patch")), ("ParentKeyName", s("Office"))],
        )
        .unwrap();
        assert_eq!(child.hidden_reason, Some(HiddenReason::Update));

        let hotfix = entry(
            RegistryView::Hklm64,
            "U2",
            &[("DisplayName", s("Fix")), ("ReleaseType", s("Hotfix"))],
        )
        .unwrap();
        assert_eq!(hotfix.hidden_reason, Some(HiddenReason::Update));

        let kb = entry(
            RegistryView::Hklm64,
            "U3",
            &[("DisplayName", s("Security Update for X (KB5034441)"))],
        )
        .unwrap();
        assert_eq!(kb.hidden_reason, Some(HiddenReason::Update));
    }

    #[test]
    fn nameless_entries_are_dropped() {
        assert!(entry(RegistryView::Hklm64, "x", &[]).is_none());
        assert!(entry(RegistryView::Hklm64, "x", &[("DisplayName", s("  "))]).is_none());
    }

    #[test]
    fn kb_detection_needs_a_real_reference() {
        assert!(contains_kb_number("Update (KB123456)"));
        assert!(!contains_kb_number("KBox Player"));
        assert!(!contains_kb_number("Network KB12 tool"));
        assert!(!contains_kb_number("ZKB1234567"));
    }

    #[test]
    fn install_date_formats() {
        assert_eq!(
            parse_install_date("20261022"),
            NaiveDate::from_ymd_opt(2026, 10, 22)
        );
        assert_eq!(
            parse_install_date("2026-10-22"),
            NaiveDate::from_ymd_opt(2026, 10, 22)
        );
        assert_eq!(parse_install_date("20261322"), None);
        assert_eq!(parse_install_date("10/22/2026"), None);
        assert_eq!(parse_install_date(""), None);
    }

    #[test]
    fn guid_detection() {
        assert!(is_guid("{23170F69-40C1-2702-2408-000001000000}"));
        assert!(!is_guid("23170F69-40C1-2702-2408-000001000000"));
        assert!(!is_guid("{23170F69-40C1-2702-2408-00000100000}"));
        assert!(!is_guid("{GGGGGGGG-40C1-2702-2408-000001000000}"));
        assert!(!is_guid("{X}_is1"));
    }

    #[test]
    fn executable_name_parsing() {
        assert_eq!(
            executable_name(r#""C:\A B\unins000.exe" /SILENT"#).as_deref(),
            Some("unins000.exe")
        );
        assert_eq!(
            executable_name(r"C:\A B\Uninstall.exe /S").as_deref(),
            Some("uninstall.exe")
        );
        assert_eq!(executable_name("  ").as_deref(), None);
    }

    #[test]
    fn stable_id_is_deterministic_and_case_insensitive() {
        let a = stable_id(RegistryView::Hklm64, "7-Zip");
        assert_eq!(a, stable_id(RegistryView::Hklm64, "7-zip"));
        assert_eq!(a.len(), 16);
        assert_ne!(a, stable_id(RegistryView::Hklm32, "7-Zip"));
        // Pinned value: must never change, ids are stored in the history.
        assert_eq!(stable_id(RegistryView::Hkcu, ""), "dc0d1899da7b1454");
    }

    #[test]
    fn list_apps_reads_the_three_views() {
        let area = TestRegistryKey::new();
        let [(_, hklm64), (_, hklm32), (_, hkcu)] = uninstall_sources();

        let gimp = area.create(&hklm64.join("GIMP-2_is1"));
        gimp.set_value("DisplayName", &"GIMP 2.10.38").unwrap();
        gimp.set_value("EstimatedSize", &421_888u32).unwrap();
        let comp = area.create(&hklm64.join("Runtime"));
        comp.set_value("DisplayName", &"VC Runtime").unwrap();
        comp.set_value("SystemComponent", &1u32).unwrap();
        area.create(&hklm64.join("Nameless"));

        let vlc = area.create(&hklm32.join("VLC media player"));
        vlc.set_value("DisplayName", &"VLC media player").unwrap();
        vlc.set_value(
            "UninstallString",
            &r"C:\Program Files (x86)\VideoLAN\VLC\uninstall.exe",
        )
        .unwrap();

        let user = area.create(&hkcu.join("Discord"));
        user.set_value("DisplayName", &"Discord").unwrap();

        let apps = list_apps(&area.registry()).unwrap();
        let names: Vec<&str> = apps.iter().map(|a| a.display_name.as_str()).collect();
        assert_eq!(
            names,
            ["Discord", "GIMP 2.10.38", "VC Runtime", "VLC media player"]
        );

        let by_name = |n: &str| apps.iter().find(|a| a.display_name == n).unwrap();
        assert_eq!(by_name("Discord").registry_view, RegistryView::Hkcu);
        assert_eq!(
            by_name("VLC media player").registry_view,
            RegistryView::Hklm32
        );
        assert_eq!(by_name("VLC media player").installer, InstallerKind::Nsis);
        assert_eq!(by_name("GIMP 2.10.38").installer, InstallerKind::InnoSetup);
        assert_eq!(
            by_name("VC Runtime").hidden_reason,
            Some(HiddenReason::SystemComponent)
        );
    }

    #[test]
    fn list_apps_tolerates_missing_keys() {
        let area = TestRegistryKey::new();
        assert!(list_apps(&area.registry()).unwrap().is_empty());
    }
}
