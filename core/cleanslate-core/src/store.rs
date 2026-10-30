//! Microsoft Store applications (EF-04): the packages of the current user,
//! listed through `PackageManager::FindPackagesForUser`, and removed for
//! this user only (`RemovePackageAsync`, never for all users) with the real
//! progress of the operation.
//!
//! Engines built for tests get [`NoPackageStore`]: no package, no removal.

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::error::{CoreError, CoreResult};
use crate::model::{AppKind, HiddenReason, InstalledApp, InstallerKind, RegistryView};
use crate::registry::{Hive, KeyView, RegKeyPath, Registry};

/// One package installed for the current user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorePackage {
    /// `PackageFullName` (`Vendor.App_1.2.3.0_x64__8wekyb3d8bbwe`).
    pub full_name: String,
    /// `PackageFamilyName` (`Vendor.App_8wekyb3d8bbwe`).
    pub family_name: String,
    /// Display name.
    pub display_name: String,
    /// Publisher display name.
    pub publisher: String,
    /// `Major.Minor.Build.Revision`.
    pub version: String,
    /// Installation date.
    pub installed_on: Option<NaiveDate>,
    /// Installation folder (usually under `WindowsApps`).
    pub install_path: Option<PathBuf>,
    /// Framework package (runtime shared by applications).
    pub is_framework: bool,
    /// Resource package (language, scale).
    pub is_resource: bool,
    /// Stub package.
    pub is_stub: bool,
    /// Bundle (the Store installs most applications as a bundle plus its
    /// main package: one line per family is shown, D52).
    pub is_bundle: bool,
    /// Signed by Windows itself (`PackageSignatureKind::System`).
    pub is_system: bool,
}

impl StorePackage {
    /// Shown in the Applications page: not a framework, a resource, a stub
    /// nor a system package. The WinRT API has no « NonRemovable » flag:
    /// those built-in applications are signed `System` and excluded here; a
    /// removal Windows still refuses is reported, nothing changes.
    pub fn is_listed(&self) -> bool {
        !(self.is_framework || self.is_resource || self.is_stub || self.is_system)
    }

    /// Published by Microsoft: family `Microsoft*`, `Windows.*`, publisher id
    /// of Microsoft (`8wekyb3d8bbwe`) or of Windows (`cw5n1h2txyewy`), or a
    /// publisher display name starting with « Microsoft ».
    pub fn is_microsoft(&self) -> bool {
        let family = self.family_name.to_ascii_lowercase();
        family.starts_with("microsoft")
            || family.starts_with("windows.")
            || family.ends_with("_8wekyb3d8bbwe")
            || family.ends_with("_cw5n1h2txyewy")
            || self.publisher.to_ascii_lowercase().starts_with("microsoft")
    }

    /// Why the application is hidden and never proposed for removal (D52):
    /// preinstalled for every user (Windows or the PC maker), or Microsoft's.
    /// `None`: installed by the user, removable.
    pub fn hidden_reason(&self, provisioned: &[String]) -> Option<HiddenReason> {
        if provisioned
            .iter()
            .any(|f| f.eq_ignore_ascii_case(&self.family_name))
        {
            Some(HiddenReason::Preinstalled)
        } else if self.is_microsoft() {
            Some(HiddenReason::MicrosoftApp)
        } else {
            None
        }
    }

    /// The package as an application of the inventory.
    pub fn to_app(&self, hidden_reason: Option<HiddenReason>) -> InstalledApp {
        InstalledApp {
            id: store_app_id(&self.family_name),
            display_name: self.display_name.clone(),
            publisher: Some(self.publisher.clone()).filter(|p| !p.is_empty()),
            version: Some(self.version.clone()).filter(|v| !v.is_empty()),
            install_date: self.installed_on,
            install_location: self.install_path.clone(),
            estimated_size_kb: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            is_msi: false,
            product_code: None,
            registry_view: RegistryView::Package,
            kind: AppKind::Store,
            hidden_reason,
            installer: InstallerKind::Msix,
            package_full_name: Some(self.full_name.clone()),
            package_family_name: Some(self.family_name.clone()),
        }
    }
}

/// One package per family (D52): the bundle when there is one (removing it
/// removes all its packages), with the best display name found in the family.
pub fn one_per_family(packages: Vec<StorePackage>) -> Vec<StorePackage> {
    let mut families: Vec<StorePackage> = Vec::new();
    for package in packages {
        let family = package.family_name.to_ascii_lowercase();
        match families
            .iter_mut()
            .find(|p| p.family_name.to_ascii_lowercase() == family)
        {
            None => families.push(package),
            Some(kept) => {
                // Best name: with spaces (« Unigram—Telegram for Windows »), then
                // without dots (« UnigramPreview »), then a technical one.
                let rank = |n: &str| u8::from(n.contains(' ')) * 2 + u8::from(!n.contains('.'));
                let name = if rank(&package.display_name) > rank(&kept.display_name) {
                    package.display_name.clone()
                } else {
                    kept.display_name.clone()
                };
                if package.is_bundle && !kept.is_bundle {
                    *kept = package;
                }
                kept.display_name = name;
            }
        }
    }
    families
}

/// Registry key of the packages provisioned for every user.
pub const PROVISIONED_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\Appx\AppxAllUserStore\Applications";

/// Families of the packages provisioned for every user (preinstalled by
/// Windows or the PC maker), read-only from `PROVISIONED_KEY`, whose
/// sub-keys are full names (`Name_Version_Arch_Resource_PublisherId`).
pub fn provisioned_families(registry: &dyn Registry) -> Vec<String> {
    let key = RegKeyPath::new(Hive::LocalMachine, PROVISIONED_KEY).with_view(KeyView::Force64);
    registry
        .subkeys(&key)
        .unwrap_or_default()
        .iter()
        .filter_map(|full| {
            let (name, _) = full.split_once('_')?;
            let (_, publisher) = full.rsplit_once('_')?;
            Some(format!("{name}_{publisher}"))
        })
        .collect()
}

/// Stable id of a Store application, from its family name.
pub fn store_app_id(family_name: &str) -> String {
    format!("store:{}", family_name.to_lowercase())
}

/// `true` for a well-formed `PackageFamilyName`: `<name>_<publisher id>`,
/// the name made of 3 to 50 letters, digits, dots or dashes, the publisher
/// id of exactly 13 lower-case letters or digits.
pub fn is_family_name(text: &str) -> bool {
    let Some((name, publisher)) = text.rsplit_once('_') else {
        return false;
    };
    (3..=50).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && publisher.len() == 13
        && publisher
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
}

/// Name part of a family name (`Vendor.App` of `Vendor.App_8wekyb3d8bbwe`).
pub fn family_label(family_name: &str) -> &str {
    family_name
        .rsplit_once('_')
        .map_or(family_name, |(name, _)| name)
}

/// The packages of the current user, and their removal.
pub trait PackageStore: Send + Sync {
    /// Every package of the current user (frameworks included).
    fn list(&self) -> CoreResult<Vec<StorePackage>>;
    /// Removes `full_name` for the current user only; `progress` receives
    /// the percentage reported by Windows.
    fn remove(&self, full_name: &str, progress: &(dyn Fn(u32) + Sync)) -> CoreResult<()>;
}

/// No package and no removal (test engines).
#[derive(Debug, Default)]
pub struct NoPackageStore;

impl PackageStore for NoPackageStore {
    fn list(&self) -> CoreResult<Vec<StorePackage>> {
        Ok(Vec::new())
    }
    fn remove(&self, full_name: &str, _: &(dyn Fn(u32) + Sync)) -> CoreResult<()> {
        Err(CoreError::InvalidInput(format!(
            "no package store on this engine: {full_name}"
        )))
    }
}

/// `Windows.Management.Deployment.PackageManager`.
#[derive(Debug, Default)]
pub struct WinRtPackageStore;

impl PackageStore for WinRtPackageStore {
    fn list(&self) -> CoreResult<Vec<StorePackage>> {
        winrt::list()
    }
    fn remove(&self, full_name: &str, progress: &(dyn Fn(u32) + Sync)) -> CoreResult<()> {
        winrt::remove(full_name, progress)
    }
}

/// Package name without the publisher prefix the Store adds
/// (`5319275A.WhatsAppDesktop` → `WhatsAppDesktop`), for packages whose
/// display name cannot be resolved.
pub fn readable_name(name: &str) -> String {
    match name.split_once('.') {
        Some((prefix, rest))
            if !rest.is_empty()
                && prefix.len() >= 5
                && prefix.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            rest.to_owned()
        }
        _ => name.to_owned(),
    }
}

/// Common name of a publisher identity (`CN=Vendor Inc., O=Vendor, C=US`
/// → `Vendor Inc.`); the whole text when there is no `CN=`.
pub fn publisher_name(identity: &str) -> String {
    identity
        .split(',')
        .map(str::trim)
        .find_map(|part| {
            part.strip_prefix("CN=")
                .or_else(|| part.strip_prefix("cn="))
        })
        .unwrap_or(identity)
        .trim_matches('"')
        .trim()
        .to_owned()
}

mod winrt {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use windows::core::HSTRING;
    use windows::ApplicationModel::PackageSignatureKind;
    use windows::Management::Deployment::{
        DeploymentProgress, DeploymentResult, PackageManager, PackageTypes,
    };
    use windows_future::{AsyncOperationProgressHandler, AsyncStatus};

    use std::path::PathBuf;

    use chrono::{DateTime, Utc};
    use rayon::prelude::*;

    use super::{publisher_name, readable_name, StorePackage};
    use crate::error::{CoreError, CoreResult};

    fn failed(what: &str) -> impl Fn(windows::core::Error) -> CoreError + '_ {
        move |e| CoreError::InvalidInput(format!("{what}: {e}"))
    }

    pub(super) fn list() -> CoreResult<Vec<StorePackage>> {
        let manager = PackageManager::new().map_err(failed("PackageManager"))?;
        // `FindPackagesForUser` with an empty user SID: the current user only;
        // every package type, so that frameworks and resources also count as
        // installed (their folders are never leftovers).
        let all = PackageTypes(
            PackageTypes::Main.0
                | PackageTypes::Framework.0
                | PackageTypes::Resource.0
                | PackageTypes::Bundle.0
                | PackageTypes::Optional.0,
        );
        let packages = manager
            .FindPackagesByUserSecurityIdWithPackageTypes(&HSTRING::new(), all)
            .map_err(failed("FindPackagesForUser"))?;
        // Cheap identity and flags for every package (the installed set)...
        let mut found = Vec::new();
        let mut shown = Vec::new();
        for package in packages {
            let Ok(id) = package.Id() else { continue };
            let (Ok(full_name), Ok(family_name)) = (id.FullName(), id.FamilyName()) else {
                continue;
            };
            let entry = StorePackage {
                full_name: full_name.to_string(),
                family_name: family_name.to_string(),
                is_framework: package.IsFramework().unwrap_or(true),
                is_resource: package.IsResourcePackage().unwrap_or(true),
                is_bundle: package.IsBundle().unwrap_or(false),
                is_system: package
                    .SignatureKind()
                    .map_or(true, |k| k == PackageSignatureKind::System),
                ..StorePackage::default()
            };
            if entry.is_listed() {
                shown.push((entry, package));
            } else {
                found.push(entry);
            }
        }
        // ...then the slow display data only for the packages shown, in
        // parallel (`Package` is agile): about 1 s sequentially (ENF-05).
        let details: Vec<StorePackage> = shown
            .into_par_iter()
            .map(|(mut entry, package)| {
                entry.is_stub = package.IsStub().unwrap_or(false);
                let Ok(id) = package.Id() else {
                    return entry;
                };
                entry.version = id
                    .Version()
                    .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision))
                    .unwrap_or_default();
                entry.display_name = package
                    .DisplayName()
                    .map(|n| n.to_string())
                    .ok()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| {
                        id.Name()
                            .map(|n| readable_name(&n.to_string()))
                            .unwrap_or_default()
                    });
                // Store-signed packages carry a GUID as identity publisher:
                // the display name first, the identity's `CN=` otherwise.
                entry.publisher = package
                    .PublisherDisplayName()
                    .map(|p| p.to_string())
                    .ok()
                    .filter(|p| !p.is_empty())
                    .or_else(|| id.Publisher().ok().map(|p| publisher_name(&p.to_string())))
                    .unwrap_or_default();
                // Creation date of its folder: instant, where `InstalledDate`
                // costs about 0.6 s for 140 packages (measured).
                entry.install_path = package
                    .InstalledPath()
                    .ok()
                    .map(|p| PathBuf::from(p.to_string()))
                    .filter(|p| !p.as_os_str().is_empty());
                entry.installed_on = entry
                    .install_path
                    .as_ref()
                    .and_then(|p| std::fs::metadata(p).ok()?.created().ok())
                    .map(|t| DateTime::<Utc>::from(t).date_naive());
                entry
            })
            .collect();
        found.extend(details);
        Ok(found)
    }

    pub(super) fn remove(full_name: &str, progress: &(dyn Fn(u32) + Sync)) -> CoreResult<()> {
        let manager = PackageManager::new().map_err(failed("PackageManager"))?;
        // RemovePackageAsync without options: the current user only.
        let operation = manager
            .RemovePackageAsync(&HSTRING::from(full_name))
            .map_err(failed("RemovePackageAsync"))?;
        let percentage = Arc::new(AtomicU32::new(0));
        let shared = Arc::clone(&percentage);
        operation
            .SetProgress(&AsyncOperationProgressHandler::<
                DeploymentResult,
                DeploymentProgress,
            >::new(move |_, value| {
                shared.store(value.percentage, Ordering::Relaxed);
                Ok(())
            }))
            .map_err(failed("SetProgress"))?;
        // Polled every 250 ms (ENF-06) on the calling thread.
        let mut last = u32::MAX;
        loop {
            let status = operation.Status().map_err(failed("Status"))?;
            let now = percentage.load(Ordering::Relaxed);
            if now != last {
                progress(now);
                last = now;
            }
            if status != AsyncStatus::Started {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let result = operation
            .GetResults()
            .map_err(failed("RemovePackageAsync"))?;
        let code = result.ExtendedErrorCode().map(|c| c.0).unwrap_or(0);
        if code != 0 {
            let text = result
                .ErrorText()
                .map(|t| t.to_string())
                .unwrap_or_default();
            return Err(CoreError::InvalidInput(format!(
                "suppression refusée par Windows ({code:#010x}) : {text}"
            )));
        }
        progress(100);
        Ok(())
    }
}

/// In-memory store for the tests: no real package is ever touched.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakePackageStore(pub std::sync::Mutex<Vec<StorePackage>>);

#[cfg(test)]
impl FakePackageStore {
    pub(crate) fn with(packages: Vec<StorePackage>) -> Self {
        FakePackageStore(std::sync::Mutex::new(packages))
    }
}

#[cfg(test)]
impl PackageStore for FakePackageStore {
    fn list(&self) -> CoreResult<Vec<StorePackage>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn remove(&self, full_name: &str, progress: &(dyn Fn(u32) + Sync)) -> CoreResult<()> {
        let mut packages = self.0.lock().unwrap();
        let before = packages.len();
        packages.retain(|p| p.full_name != full_name);
        if packages.len() == before {
            return Err(CoreError::NotFound {
                location: full_name.to_owned(),
            });
        }
        for pct in [0, 40, 100] {
            progress(pct);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn package(family: &str) -> StorePackage {
        let (name, publisher) = family.rsplit_once('_').unwrap();
        StorePackage {
            full_name: format!("{name}_1.2.3.0_x64__{publisher}"),
            family_name: family.into(),
            display_name: family_label(family).replace('.', " "),
            publisher: "Vendor".into(),
            version: "1.2.3.0".into(),
            ..StorePackage::default()
        }
    }

    #[test]
    fn listed_packages_exclude_frameworks_resources_stubs_and_system() {
        let app = package("Vendor.App_abcdefghjk123");
        assert!(app.is_listed());
        for excluded in [
            StorePackage {
                is_framework: true,
                ..app.clone()
            },
            StorePackage {
                is_resource: true,
                ..app.clone()
            },
            StorePackage {
                is_stub: true,
                ..app.clone()
            },
            StorePackage {
                is_system: true,
                ..app.clone()
            },
        ] {
            assert!(!excluded.is_listed());
        }
    }

    #[test]
    fn package_becomes_a_store_application() {
        let app = package("Vendor.App_abcdefghjk123").to_app(None);
        assert_eq!(app.id, "store:vendor.app_abcdefghjk123");
        assert_eq!(app.kind, AppKind::Store);
        assert_eq!(app.installer, InstallerKind::Msix);
        assert_eq!(app.registry_view, RegistryView::Package);
        assert_eq!(
            app.package_full_name.as_deref(),
            Some("Vendor.App_1.2.3.0_x64__abcdefghjk123")
        );
        assert_eq!(app.version.as_deref(), Some("1.2.3.0"));
        assert!(app.uninstall_string.is_none());
    }

    #[test]
    fn one_line_per_family_prefers_the_bundle_and_a_resolved_name() {
        let main = StorePackage {
            display_name: "WhatsApp Messenger".into(),
            ..package("5319275A.WhatsAppDesktop_cv1g1gvanyjgm")
        };
        let bundle = StorePackage {
            full_name: "5319275A.WhatsAppDesktop_2.0.0.0_neutral_~_cv1g1gvanyjgm".into(),
            display_name: "WhatsAppDesktop".into(),
            is_bundle: true,
            ..main.clone()
        };
        let other = package("Vendor.Notes_abcdefghjk123");
        let kept = one_per_family(vec![main, other.clone(), bundle.clone()]);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].full_name, bundle.full_name, "the bundle is removed");
        assert_eq!(
            kept[0].display_name, "WhatsApp Messenger",
            "the resolved name is kept"
        );
        assert_eq!(kept[1], other);
    }

    #[test]
    fn microsoft_and_preinstalled_packages_are_never_proposed() {
        let provisioned = vec!["RealtekSemiconductorCorp.HPAudioControl_dt26b99r8h8gj".to_owned()];
        let oem = package("RealtekSemiconductorCorp.HPAudioControl_dt26b99r8h8gj");
        assert_eq!(
            oem.hidden_reason(&provisioned),
            Some(HiddenReason::Preinstalled)
        );
        for microsoft in [
            package("Microsoft.People_8wekyb3d8bbwe"),
            package("MicrosoftWindows.Client.WebExperience_cw5n1h2txyewy"),
            package("Windows.PrintDialog_cw5n1h2txyewy"),
            package("Clipchamp.Clipchamp_8wekyb3d8bbwe"),
            StorePackage {
                publisher: "Microsoft Corporation".into(),
                ..package("SomeTool.App_abcdefghjk123")
            },
        ] {
            assert_eq!(
                microsoft.hidden_reason(&[]),
                Some(HiddenReason::MicrosoftApp),
                "{}",
                microsoft.family_name
            );
        }
        // Installed by the user: proposed.
        let user = package("5319275A.WhatsAppDesktop_cv1g1gvanyjgm");
        assert_eq!(user.hidden_reason(&provisioned), None);
        assert_eq!(
            user.to_app(Some(HiddenReason::Preinstalled)).hidden_reason,
            Some(HiddenReason::Preinstalled)
        );
    }

    #[test]
    fn provisioned_families_are_read_from_the_registry() {
        use crate::registry::TestRegistryKey;
        let area = TestRegistryKey::new();
        let key = RegKeyPath::new(Hive::LocalMachine, PROVISIONED_KEY);
        for full in [
            "AD2F1837.HPSystemInformation_2.0.0.0_x64__v10z8vjag6ke6",
            "Microsoft.People_10.2202.100.0_neutral_~_8wekyb3d8bbwe",
        ] {
            area.create(&key.join(full));
        }
        let mut families = provisioned_families(&area.registry());
        families.sort();
        assert_eq!(
            families,
            [
                "AD2F1837.HPSystemInformation_v10z8vjag6ke6",
                "Microsoft.People_8wekyb3d8bbwe"
            ]
        );
        // Missing key: nothing provisioned, nothing hidden for that reason.
        assert!(provisioned_families(&TestRegistryKey::new().registry()).is_empty());
    }

    #[test]
    fn family_names_are_checked() {
        for good in [
            "Vendor.App_abcdefghjk123",
            "45273LiamForsyth.PawsforTrello_7pb5ddty8z1pa",
            "1527c705-839a-4832-9118-54d4Bd6a0c89_cw5n1h2txyewy",
        ] {
            assert!(is_family_name(good), "{good}");
        }
        for bad in [
            "Vendor.App",
            "Vendor.App_ABCDEFGHJK123",
            "Vendor.App_abc",
            "ab_abcdefghjk123",
            r"Vendor\App_abcdefghjk123",
            "Vendor App_abcdefghjk123",
            "..\\x_abcdefghjk123",
            "",
        ] {
            assert!(!is_family_name(bad), "{bad}");
        }
        assert_eq!(family_label("Vendor.App_abcdefghjk123"), "Vendor.App");
    }

    #[test]
    fn publishers_and_test_store() {
        assert_eq!(
            publisher_name("CN=Vendor Inc., O=Vendor, L=Paris, C=FR"),
            "Vendor Inc."
        );
        // A quoted name with a comma is cut at the comma, quotes removed.
        assert_eq!(publisher_name(r#"CN="Vendor, Ltd", O=Vendor"#), "Vendor");
        assert_eq!(publisher_name("O=Only"), "O=Only");
        assert_eq!(readable_name("5319275A.WhatsAppDesktop"), "WhatsAppDesktop");
        assert_eq!(
            readable_name("38833FF26BA1D.UnigramPreview"),
            "UnigramPreview"
        );
        assert_eq!(readable_name("Microsoft.People"), "Microsoft.People");
        assert_eq!(readable_name("AD2F1837.HPPrinterDriver"), "HPPrinterDriver");
        assert_eq!(readable_name("Vendor"), "Vendor");
        assert!(NoPackageStore.list().unwrap().is_empty());
        assert!(NoPackageStore.remove("X", &|_| {}).is_err());
    }
}
