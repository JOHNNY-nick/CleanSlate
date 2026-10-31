//! Matching of candidates with installed applications, confidence score and
//! grouping (specs § 6.2 and § 6.3, EF-08 to EF-10).

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use strsim::jaro_winkler;

use crate::model::{Confidence, InstalledApp, Leftover, LeftoverGroup, LeftoverKind};

/// Similarity above which a candidate is attached to an installed app.
pub const ATTACH_SIMILARITY: f64 = 0.90;
/// Similarity below which a candidate looks unrelated to every installed app.
pub const UNRELATED_SIMILARITY: f64 = 0.75;
/// Score threshold of [`Confidence::High`] (pre-checked).
pub const HIGH_THRESHOLD: i32 = 60;
/// Score threshold of [`Confidence::Medium`].
pub const MEDIUM_THRESHOLD: i32 = 35;

/// Words dropped by [`normalize`]: architecture mentions and legal suffixes.
const NOISE_WORDS: [&str; 22] = [
    "x64",
    "x86",
    "amd64",
    "arm64",
    "win64",
    "win32",
    "bit",
    "32bit",
    "64bit",
    "inc",
    "ltd",
    "llc",
    "gmbh",
    "corporation",
    "corp",
    "sas",
    "sa",
    "co",
    "limited",
    "srl",
    "bv",
    "ag",
];

/// Generic folder or key names, often shared (−25).
const GENERIC_NAMES: [&str; 18] = [
    "temp", "tmp", "cache", "caches", "common", "shared", "data", "config", "logs", "log",
    "settings", "updates", "update", "backup", "plugins", "default", "user", "profiles",
];

/// Normalizes a name (specs § 6.2): lower case, no accents, no version
/// numbers, no architecture or legal suffix, no punctuation.
pub fn normalize(name: &str) -> String {
    let folded: String = name
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'ò' | 'õ' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ÿ' | 'ý' => 'y',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect();
    folded
        .split_whitespace()
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .filter(|w| !is_version_word(w))
        .filter(|w| !NOISE_WORDS.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `v2`, `v10`… (version markers left once dots are removed).
fn is_version_word(word: &str) -> bool {
    word.strip_prefix('v')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
}

/// What is known about the installed applications (specs § 6.1, step 1).
#[derive(Debug, Default, Clone)]
pub struct KnownSet {
    names: Vec<String>,
    publishers: Vec<String>,
    locations: Vec<String>,
    executables: Vec<String>,
}

/// Lower-cased path with `\` separators and no trailing separator.
fn path_key(path: &str) -> String {
    path.replace('/', r"\")
        .to_lowercase()
        .trim_end_matches('\\')
        .to_owned()
}

fn within(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!(r"{root}\"))
}

impl KnownSet {
    /// Builds the set from the inventory (hidden entries included: they are
    /// still installed) plus executables of services and running processes.
    pub fn new(apps: &[InstalledApp], extra_executables: &[String]) -> Self {
        let mut set = KnownSet::default();
        for app in apps {
            push_unique(&mut set.names, normalize(&app.display_name));
            if let Some(publisher) = &app.publisher {
                push_unique(&mut set.publishers, normalize(publisher));
            }
            if let Some(location) = &app.install_location {
                push_unique(&mut set.locations, path_key(&location.to_string_lossy()));
            }
            for command in [&app.uninstall_string, &app.quiet_uninstall_string]
                .into_iter()
                .flatten()
            {
                if let Some(exe) = command_executable(command) {
                    if let Some(dir) = Path::new(&exe).parent() {
                        push_unique(&mut set.locations, path_key(&dir.to_string_lossy()));
                    }
                    push_unique(&mut set.executables, path_key(&exe));
                }
            }
        }
        for exe in extra_executables {
            push_unique(&mut set.executables, path_key(exe));
        }
        set.names.retain(|n| !n.is_empty());
        set.publishers.retain(|p| !p.is_empty());
        // Folders that only hold an uninstaller (e.g. `…\GIMP 2\uninst`)
        // would otherwise also attach `C:\Program Files`: drop roots.
        set.locations.retain(|l| l.matches('\\').count() >= 2);
        set
    }

    /// Adds the name of a program found on disk without an Uninstall entry
    /// (folder with executables in Program Files or `Local\Programs`).
    pub fn add_present_program(&mut self, name: &str) {
        let normalized = normalize(name);
        if !normalized.is_empty() {
            push_unique(&mut self.names, normalized);
        }
    }

    /// Best similarity of `normalized` with an installed name or publisher.
    pub fn best_similarity(&self, normalized: &str) -> f64 {
        self.names
            .iter()
            .chain(&self.publishers)
            .map(|known| jaro_winkler(normalized, known))
            .fold(0.0, f64::max)
    }

    /// `true` if `normalized` is the publisher of an installed application.
    pub fn is_installed_publisher(&self, normalized: &str) -> bool {
        !normalized.is_empty()
            && self
                .publishers
                .iter()
                .any(|p| jaro_winkler(normalized, p) >= ATTACH_SIMILARITY)
    }

    /// Specs § 6.2: returns why `candidate` belongs to an installed
    /// application, or `None` when it is an orphan.
    pub fn attach(&self, name: &str, path: Option<&str>, executables: &[String]) -> Option<String> {
        let normalized = normalize(name);
        if normalized.is_empty() {
            return Some("nom trop générique pour être rattaché".into());
        }
        if let Some(path) = path.map(path_key) {
            if let Some(loc) = self
                .locations
                .iter()
                .find(|loc| within(&path, loc) || within(loc, &path))
            {
                return Some(format!("dossier d'installation connu : {loc}"));
            }
            if self.executables.iter().any(|exe| within(exe, &path)) {
                return Some(
                    "contient l'exécutable d'une application, d'un service ou d'un processus"
                        .into(),
                );
            }
        }
        if executables
            .iter()
            .any(|exe| self.executables.contains(&path_key(exe)))
        {
            return Some("contient un exécutable connu".into());
        }
        if let Some(known) = self
            .names
            .iter()
            .chain(&self.publishers)
            .find(|known| jaro_winkler(&normalized, known) >= ATTACH_SIMILARITY)
        {
            return Some(format!("nom proche d'une application installée : {known}"));
        }
        let words: Vec<&str> = normalized.split(' ').collect();
        // D68: `pnpm cache` belongs to `pnpm`; the whole first word must be the name.
        if let Some(known) = self.names.iter().find(|known| {
            known.len() >= 2 && !known.contains(' ') && words.first() == Some(&known.as_str())
        }) {
            return Some(format!(
                "son nom commence par celui d'un programme présent : {known}"
            ));
        }
        if self
            .names
            .iter()
            .any(|known| words.iter().all(|w| known.split(' ').any(|k| k == *w)))
        {
            return Some("tous ses mots figurent dans le nom d'une application installée".into());
        }
        None
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

/// Executable path of a command line (`"C:\a b\x.exe" /S` or `C:\a\x.exe /S`).
pub fn command_executable(command: &str) -> Option<String> {
    let command = command.trim();
    let path = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?.to_owned()
    } else {
        let lower = command.to_ascii_lowercase();
        let end = lower.find(".exe").map(|i| i + 4)?;
        command[..end].to_owned()
    };
    // `MsiExec.exe /X{…}` has no folder: it tells nothing about a location.
    (path.contains('\\') && path.to_ascii_lowercase().ends_with(".exe")).then_some(path)
}

/// Facts gathered by the scan about one orphan candidate.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// Most recent modification found below the item.
    pub last_modified: Option<DateTime<Utc>>,
    /// The item contains at least one `.exe`.
    pub has_executable: bool,
    /// The item lies in `Program Files` or `Program Files (x86)`.
    pub in_program_files: bool,
    /// Registry key: number of values and sub-keys.
    pub registry_counts: Option<(usize, usize)>,
    /// Shortcut whose target no longer exists.
    pub dead_shortcut: bool,
    /// The item lies in a personal folder (Documents): never pre-checked.
    pub personal_folder: bool,
    /// The item lies in the folder of a publisher that still has an
    /// installed application: at most Medium (never pre-checked).
    pub under_installed_publisher: bool,
    /// The folder holds a database, a vault, a mailbox or a virtual disk:
    /// at most Medium, never pre-checked (D68).
    pub contains_data: bool,
}

/// Computes the confidence score of an orphan (specs § 6.3) with its
/// human-readable reasons, in French.
pub fn score(
    kind: LeftoverKind,
    name: &str,
    similarity: f64,
    facts: &Facts,
    now: DateTime<Utc>,
) -> (i32, Confidence, Vec<String>) {
    let mut points = 0;
    let mut reasons = Vec::new();
    let mut add = |delta: i32, reason: &str| {
        points += delta;
        reasons.push(reason.to_owned());
    };

    if similarity < UNRELATED_SIMILARITY {
        add(
            40,
            "Aucune application installée ne porte un nom ou un éditeur proche",
        );
    }
    if let Some(modified) = facts.last_modified {
        if now - modified > Duration::days(30) {
            add(20, "Non modifié depuis plus de 30 jours");
        } else if now - modified < Duration::days(7) {
            add(-30, "Modifié il y a moins de 7 jours");
        }
    }
    if facts.dead_shortcut {
        add(60, "Raccourci dont la cible n'existe plus");
    }
    let is_file_like = matches!(kind, LeftoverKind::Folder | LeftoverKind::File);
    if is_file_like && !facts.has_executable {
        add(10, "Ne contient aucun exécutable");
    }
    if GENERIC_NAMES.contains(&normalize(name).as_str()) {
        add(-25, "Nom générique, souvent partagé entre logiciels");
    }
    // Program Files, or AppData for per-user installs (Squirrel, portable): D68.
    if is_file_like && facts.has_executable && !facts.dead_shortcut {
        add(
            -25,
            "Contient des programmes sans entrée de désinstallation (logiciel portable ou installé pour l'utilisateur probable)",
        );
    }
    if let Some((values, subkeys)) = facts.registry_counts {
        if values < 3 && subkeys == 0 {
            add(-10, "Clé de registre presque vide");
        }
    }

    let mut confidence = match points {
        p if p >= HIGH_THRESHOLD => Confidence::High,
        p if p >= MEDIUM_THRESHOLD => Confidence::Medium,
        _ => Confidence::Low,
    };
    if facts.under_installed_publisher && confidence == Confidence::High {
        confidence = Confidence::Medium;
        reasons.push(
            "Dans le dossier d'un éditeur dont une application est encore installée : jamais précoché"
                .to_owned(),
        );
    }
    if facts.contains_data && confidence == Confidence::High {
        confidence = Confidence::Medium;
    }
    if facts.contains_data {
        reasons.push(
            "Contient des données (base de données, coffre, messagerie ou disque virtuel) : jamais précoché"
                .to_owned(),
        );
    }
    if facts.personal_folder {
        confidence = Confidence::Low;
        reasons.push("Dossier personnel possible : jamais précoché".to_owned());
    }
    (points, confidence, reasons)
}

/// Groups orphans by guessed application (specs § 6.1, step 6), largest
/// groups first.
pub fn group(leftovers: Vec<(Option<String>, Leftover)>) -> Vec<LeftoverGroup> {
    let mut groups: BTreeMap<String, LeftoverGroup> = BTreeMap::new();
    for (publisher, item) in leftovers {
        let label = item
            .guessed_app
            .clone()
            .unwrap_or_else(|| item.location.clone());
        let key = match normalize(&label) {
            k if k.is_empty() => label.to_lowercase(),
            k => k,
        };
        let group = groups.entry(key).or_insert_with(|| LeftoverGroup {
            label: label.clone(),
            publisher: publisher.clone(),
            items: Vec::new(),
            total_bytes: 0,
        });
        group.total_bytes += item.size_bytes;
        group.items.push(item);
    }
    let mut groups: Vec<LeftoverGroup> = groups.into_values().collect();
    for group in &mut groups {
        group
            .items
            .sort_by_key(|item| std::cmp::Reverse(item.size_bytes));
    }
    groups.sort_by(|a, b| {
        b.total_bytes
            .cmp(&a.total_bytes)
            .then(a.label.cmp(&b.label))
    });
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppKind, InstallerKind, RegistryView};
    use chrono::TimeZone;
    use std::path::PathBuf;

    fn app(name: &str, publisher: Option<&str>, location: Option<&str>) -> InstalledApp {
        InstalledApp {
            id: name.into(),
            display_name: name.into(),
            publisher: publisher.map(Into::into),
            version: None,
            install_date: None,
            install_location: location.map(PathBuf::from),
            estimated_size_kb: None,
            uninstall_string: None,
            quiet_uninstall_string: None,
            is_msi: false,
            product_code: None,
            registry_view: RegistryView::Hklm64,
            kind: AppKind::Win32,
            hidden_reason: None,
            installer: InstallerKind::Unknown,
            package_full_name: None,
            package_family_name: None,
            caution: None,
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
    }

    #[test]
    fn normalization_rules() {
        assert_eq!(normalize("Notepad++ (64-bit x64)"), "notepad");
        assert_eq!(normalize("GIMP 2.10.38"), "gimp");
        assert_eq!(normalize("The GIMP Team"), "the gimp team");
        assert_eq!(normalize("Éditions Créatives SAS"), "editions creatives");
        assert_eq!(normalize("VideoLAN Inc."), "videolan");
        assert_eq!(normalize("Audacity v3"), "audacity");
        assert_eq!(normalize("7-Zip 24.08"), "zip");
        assert_eq!(normalize("1.0"), "");
    }

    #[test]
    fn attach_by_install_location_both_ways() {
        let known = KnownSet::new(&[app("GIMP", None, Some(r"C:\Program Files\GIMP 2\"))], &[]);
        assert!(known
            .attach("lib", Some(r"C:\Program Files\GIMP 2\lib"), &[])
            .is_some());
        assert!(known
            .attach("Other", Some(r"C:\Program Files\Other"), &[])
            .is_none());
        let deep = KnownSet::new(
            &[app("Tool", None, Some(r"C:\Program Files\Vendor\Tool"))],
            &[],
        );
        assert!(deep
            .attach("Vendor", Some(r"C:\Program Files\Vendor"), &[])
            .is_some());
    }

    #[test]
    fn attach_by_uninstaller_folder_and_executables() {
        let mut a = app("Old name", None, None);
        a.uninstall_string = Some(r#""C:\Apps\Thing\uninst.exe" /S"#.into());
        let known = KnownSet::new(&[a], &[r"C:\Svc\Agent\agent.exe".into()]);
        assert!(known.attach("Thing", Some(r"C:\Apps\Thing"), &[]).is_some());
        assert!(known.attach("Agent", Some(r"C:\Svc\Agent"), &[]).is_some());
        assert!(known
            .attach(
                "Other",
                Some(r"C:\Other"),
                &[r"C:\Svc\Agent\agent.exe".into()]
            )
            .is_some());
    }

    #[test]
    fn attach_by_similar_name_or_publisher() {
        let known = KnownSet::new(&[app("Mozilla Firefox", Some("Mozilla"), None)], &[]);
        assert!(known.attach("Mozilla", None, &[]).is_some());
        assert!(known.attach("firefox", None, &[]).is_some());
        assert!(known.attach("Audacity", None, &[]).is_none());
        assert!(known.is_installed_publisher("mozilla"));
        assert!(!known.is_installed_publisher("videolan"));
    }

    /// D68: `pnpm-cache` and `pnpm-state` belong to `pnpm`; a one-word name must be the
    /// whole first word, so `codeblocks` is not `code`, nor `uvicorn` `uv`.
    #[test]
    fn attach_when_the_name_starts_with_a_present_program() {
        let mut known = KnownSet::default();
        known.add_present_program("pnpm");
        known.add_present_program("code");
        known.add_present_program("uv");
        assert!(known.attach("pnpm-cache", None, &[]).is_some());
        assert!(known.attach("pnpm-state", None, &[]).is_some());
        assert!(known.attach("Code Cache", None, &[]).is_some());
        assert!(known.attach("CodeBlocks", None, &[]).is_none());
        assert!(known.attach("uvicorn", None, &[]).is_none());
        assert!(known.attach("Audacity", None, &[]).is_none());
    }

    #[test]
    fn empty_normalized_name_is_never_proposed() {
        let known = KnownSet::default();
        assert!(known.attach("1.0", None, &[]).is_some());
    }

    #[test]
    fn command_executable_parsing() {
        assert_eq!(
            command_executable(r#""C:\A B\unins000.exe" /SILENT"#).as_deref(),
            Some(r"C:\A B\unins000.exe")
        );
        assert_eq!(
            command_executable(r"C:\A B\Uninstall.exe /S").as_deref(),
            Some(r"C:\A B\Uninstall.exe")
        );
        assert_eq!(command_executable("MsiExec.exe /X{1}"), None);
        assert_eq!(command_executable("rundll32 foo"), None);
    }

    #[test]
    fn old_unrelated_folder_is_high() {
        let facts = Facts {
            last_modified: Some(now() - Duration::days(200)),
            ..Facts::default()
        };
        let (points, confidence, reasons) =
            score(LeftoverKind::Folder, "audacity", 0.4, &facts, now());
        assert_eq!(points, 70);
        assert_eq!(confidence, Confidence::High);
        assert_eq!(reasons.len(), 3);
    }

    #[test]
    fn recent_or_generic_or_portable_is_lower() {
        let recent = Facts {
            last_modified: Some(now() - Duration::days(2)),
            ..Facts::default()
        };
        assert_eq!(
            score(LeftoverKind::Folder, "x", 0.4, &recent, now()).1,
            Confidence::Low
        );

        let generic = score(LeftoverKind::Folder, "Cache", 0.4, &Facts::default(), now());
        assert_eq!(generic.0, 25);

        let portable = Facts {
            has_executable: true,
            in_program_files: true,
            last_modified: Some(now() - Duration::days(100)),
            ..Facts::default()
        };
        let (points, confidence, _) = score(LeftoverKind::Folder, "Tool", 0.4, &portable, now());
        assert_eq!(points, 35);
        assert_eq!(confidence, Confidence::Medium);

        // D68: the same in AppData (per-user installs such as Postman): never pre-checked.
        let per_user = Facts {
            in_program_files: false,
            ..portable
        };
        let (points, confidence, _) = score(LeftoverKind::Folder, "Postman", 0.4, &per_user, now());
        assert_eq!(points, 35);
        assert_eq!(confidence, Confidence::Medium);
    }

    #[test]
    fn registry_and_shortcut_signals() {
        let key = Facts {
            registry_counts: Some((1, 0)),
            ..Facts::default()
        };
        assert_eq!(
            score(LeftoverKind::RegistryKey, "Vendor", 0.4, &key, now()).0,
            30
        );
        let lnk = Facts {
            dead_shortcut: true,
            has_executable: true,
            ..Facts::default()
        };
        assert_eq!(
            score(LeftoverKind::Shortcut, "App.lnk", 0.9, &lnk, now()).1,
            Confidence::High
        );
    }

    #[test]
    fn items_under_an_installed_publisher_are_at_most_medium() {
        let facts = Facts {
            last_modified: Some(now() - Duration::days(400)),
            under_installed_publisher: true,
            ..Facts::default()
        };
        let (_, confidence, reasons) = score(LeftoverKind::Folder, "NGX", 0.1, &facts, now());
        assert_eq!(confidence, Confidence::Medium);
        assert!(reasons.last().unwrap().contains("éditeur"));
    }

    #[test]
    fn present_programs_attach_their_data() {
        let mut known = KnownSet::default();
        assert!(known.attach("obsidian", None, &[]).is_none());
        known.add_present_program("Obsidian");
        assert!(known.attach("obsidian", None, &[]).is_some());
        known.add_present_program("1.0");
        assert!(known.attach("Other", None, &[]).is_none());
    }

    #[test]
    fn personal_folders_are_never_high() {
        let facts = Facts {
            last_modified: Some(now() - Duration::days(400)),
            personal_folder: true,
            ..Facts::default()
        };
        let (points, confidence, reasons) =
            score(LeftoverKind::Folder, "Thesis", 0.1, &facts, now());
        assert_eq!(points, 70);
        assert_eq!(confidence, Confidence::Low);
        assert!(reasons.last().unwrap().contains("personnel"));
    }

    fn leftover(name: &str, location: &str, size: u64) -> Leftover {
        Leftover {
            id: location.into(),
            kind: LeftoverKind::Folder,
            location: location.into(),
            size_bytes: size,
            last_modified: None,
            confidence: Confidence::High,
            reasons: vec![],
            guessed_app: Some(name.into()),
        }
    }

    #[test]
    fn grouping_by_normalized_name_largest_first() {
        let groups = group(vec![
            (None, leftover("audacity", r"C:\R\audacity", 10)),
            (None, leftover("Audacity", r"HKCU\Software\Audacity", 1)),
            (
                Some("VideoLAN".into()),
                leftover("VLC", r"C:\R\VideoLAN\VLC", 100),
            ),
        ]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].label, "VLC");
        assert_eq!(groups[0].publisher.as_deref(), Some("VideoLAN"));
        assert_eq!(groups[1].items.len(), 2);
        assert_eq!(groups[1].total_bytes, 11);
    }
}
