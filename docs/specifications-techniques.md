# Spécifications techniques — CleanSlate

Dernière mise à jour : 29 oct. 2026

## 1. Architecture

CleanSlate se compose de trois couches : une interface C# / WinUI 3, une DLL native `cleanslate.dll` qui expose une API C, et un moteur Rust qui seul touche au disque, au registre et aux processus.

```text
┌──────────────────── Interface — C# / .NET 8 / WinUI 3 ────────────────────┐
│  Vues XAML  ──▶  ViewModels  ──▶  IEngineService  ──▶  EngineClient        │
│  (App : pages,   (Presentation,   (interface dans     (Interop : seul      │
│   dialogues)      MVVM Toolkit)    Presentation,       code C# à           │
│                                    impl. dans App)     pointeurs)          │
│                                                            │               │
└────────────────────────────────────────────────────────────┼──────────────┘
                                   P/Invoke : JSON et progression ▲▼
┌──────── cleanslate.dll — API C : poignée opaque, JSON UTF-8, callbacks ────┐
└───────────────────────────────────┬────────────────────────────────────────┘
┌──────────────────────────── Moteur Rust ───┼────────────────────────────────┐
│  cleanslate-ffi : catch_unwind, JSON vers types Rust, ByteBuffer           │
│  ┌──────────────────────── cleanslate-core ──────────────────────────────┐ │
│  │ inventory  │ scan       │ matcher        │ safety                      │ │
│  │ (apps)     │ (disque +  │ (orphelins +   │ (liste noire)               │ │
│  │            │  registre) │  score)        │                             │ │
│  │ uninstall  │ quarantine │ history        │ model                       │ │
│  │ (lance et  │ (restau-   │ (sessions      │ (types partagés)            │ │
│  │  attend)   │  rable 30j)│  SQLite)       │                             │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────┬────────────────────────────────────────┘
                                    │ crates windows et winreg
┌──────────────────────────────── Windows ───────────────────────────────────┐
│  Registre (Uninstall, Software) │ Fichiers (AppData, ProgramData)          │
│  Processus (Job Objects)        │ Données locales (base, quarantaine)      │
└────────────────────────────────────────────────────────────────────────────┘
```

Seul `EngineClient` appelle la DLL ; tout le reste de l'interface ignore Rust. Le moteur peut ainsi être testé seul, et réutilisé par la petite CLI de debug.

## 2. Structure du dépôt et outillage

Un seul dépôt Git contient le moteur Rust, l'interface C# et la documentation ; un script PowerShell compile les deux dans le bon ordre.

Arborescence ; les éléments marqués (Jx) n'existent pas encore et arrivent au jalon indiqué.

```text
cleanslate/
├─ CLAUDE.md                  # consignes permanentes pour Claude Code
├─ README.md
├─ LICENSE                    # MIT
├─ docs/                      # cahier des charges, spécifications, design, prompts, maquettes/
├─ core/                      # workspace Cargo
│  ├─ Cargo.toml              # membres, dépendances et lints communs
│  ├─ clippy.toml             # unwrap/expect autorisés dans les tests seulement
│  ├─ cleanslate-core/        # bibliothèque : toute la logique métier
│  │  └─ src/
│  │     ├─ lib.rs, engine.rs, error.rs, model.rs, fs.rs, registry.rs   # J1
│  │     └─ inventory.rs (J2), scan.rs, matcher.rs, safety.rs (J3),
│  │        quarantine.rs, history.rs (J4), uninstall.rs (J5)
│  ├─ cleanslate-ffi/         # cdylib cleanslate.dll : API C fine au-dessus du core
│  │  ├─ build.rs             # génère app/CleanSlate.Interop/NativeMethods.g.cs via csbindgen
│  │  └─ src/lib.rs
│  └─ cleanslate-cli/         # binaire de debug (version en J1, apps en J2, scan en J3)
├─ app/
│  ├─ CleanSlate.sln          # configurations Debug|x64 et Release|x64 uniquement
│  ├─ Directory.Build.props   # réglages communs (C# 12, nullable, x64, chemin de cleanslate.dll)
│  ├─ CleanSlate.App/         # WinUI 3 non packagée : Views/, Services/ (EngineService), Strings/
│  ├─ CleanSlate.Presentation/ # net8.0 sans WinUI : ViewModels/, Services/ (interfaces)
│  ├─ CleanSlate.Interop/     # net8.0 : NativeMethods.g.cs (généré, versionné) + EngineClient
│  └─ CleanSlate.Tests/       # xUnit v3 + NSubstitute
├─ tests/fixtures/            # arborescences et .reg de test (J3)
├─ scripts/build.ps1          # cargo build puis dotnet build
└─ .github/workflows/ci.yml
```

| Outil | Version de référence | Rôle |
| --- | --- | --- |
| Rust (rustup) | stable du moment (1.99.1 au J1), édition 2021, cible `x86_64-pc-windows-msvc`, composants `rustfmt` et `clippy` | Moteur |
| Visual Studio 2022 (ou Build Tools 2022) | charges « Développement Desktop C++ » (linker MSVC), « Développement .NET Desktop » et « Développement d'applications WinUI » | Compilation, débogage mixte |
| .NET SDK | 8.0 (8.0.427 au J1) | Interface |
| Windows App SDK | 1.8.x (1.8.260921001 au J1) jusqu'à J6, où la ligne 2.x sera évaluée ; application **non packagée** (`WindowsPackageType=None`) | WinUI 3 |
| PowerShell | 7.x (`pwsh`) ; `build.ps1` reste compatible avec Windows PowerShell 5.1 | `scripts/build.ps1` |
| cargo-llvm-cov | dernière version, avec le composant `llvm-tools-preview` (outil de développement, pas une dépendance du code) | Couverture du moteur (ENF-12) |
| Claude Code | dernière version | Développement assisté |
| Git + GitHub | — | Versionnement, CI |

L'application est **non packagée** : un `app.manifest` demande `requireAdministrator`, ce qu'une application packagée ne peut pas faire. Pour déboguer, Visual Studio doit être lancé en administrateur. L'exécutable s'appelle `CleanSlate.App.exe` : aucune assembly .NET ne doit s'appeler `CleanSlate.dll`, car elle écraserait la DLL native `cleanslate.dll` (Windows ne distingue pas la casse). `CleanSlate.Interop` cible `net8.0` (et non `net8.0-windows…`) pour pouvoir être référencé par `CleanSlate.Presentation`. Le fichier `NativeMethods.g.cs` est généré par `build.rs` directement dans `app/CleanSlate.Interop/` et **versionné** dans Git (fins de ligne LF imposées par `.gitattributes`), pour que la solution .NET compile même sans avoir lancé cargo.

Crate `cleanslate-cli` : un petit binaire qui permet de tester le moteur sans l'interface, pratique pour apprendre Rust étape par étape. Commandes : `version [--json]` (J1), `apps [--json] [--all]` (J2), `scan --dry-run [--json]` (J3, refusée sans `--dry-run`).

## 3. Moteur Rust

Toute la logique métier vit dans `cleanslate-core`, une bibliothèque Rust sans code FFI ni `unsafe` (hors appels Win32 encapsulés), testable seule.

| Module | Responsabilité | Exigences |
| --- | --- | --- |
| `model` | Types partagés (`InstalledApp`, `Leftover`, `ScanReport`, `Session`…), sérialisation serde | Toutes |
| `error` | `CoreError` (thiserror) et ses codes stables transmis à l'interface | Toutes |
| `engine` | `EngineConfig` et `Engine`, qui reçoit par injection un `FileSystem` et un `Registry` | Toutes |
| `fs` | Trait `FileSystem` : `RealFileSystem` (lecture seule, ne suit jamais les reparse points) et `FakeFileSystem` (en mémoire, pour les tests) | ENF-03 |
| `registry` | Trait `Registry` : `WinRegistry::system()` (registre réel) et `WinRegistry::sandboxed(…)` (redirigé sous `HKCU\Software\CleanSlateTest\…`, pour les tests) | EF-01, EF-06 |
| `inventory` | Lecture des trois clés Uninstall, normalisation, filtrage des composants système | EF-01 à EF-04 |
| `scan` | Parcours parallèle des dossiers et du registre, sans suivre les jonctions, avec annulation | EF-05 à EF-07, EF-11 |
| `matcher` | Rapprochement élément ↔ application, détection des orphelins, score de confiance, regroupement | EF-08 à EF-10 |
| `safety` | Liste noire, exclusions utilisateur, validation finale de tout chemin avant action | ENF-01, ENF-03, EF-20 |
| `uninstall` | Exécution du désinstalleur officiel, attente de l'arbre de processus, point de restauration | EF-12 à EF-15 |
| `quarantine` | Déplacement des fichiers, export des clés, manifeste, restauration, purge | EF-16 à EF-19 |
| `history` | Persistance des sessions et des réglages (SQLite) | EF-20, ENF-10 |

### 3.1 Crates externes

| Crate | Usage |
| --- | --- |
| `windows` | API Win32 : dossiers connus (`SHGetKnownFolderPath`), Job Objects, point de restauration, raccourcis |
| `windows-future` | Types asynchrones WinRT (gestionnaire de progression de `RemovePackageAsync`, D48 ; même famille que `windows`, accord du porteur) |
| `winreg` | Lecture, export et suppression de clés de registre, vues 32 et 64 bits |
| `rayon` | Parallélisme du scan (le parcours récursif passe par le trait `FileSystem`, testable : `walkdir` n'est pas utilisé, D19) |
| `serde` + `serde_json` | Sérialisation des échanges avec C# |
| `rusqlite` (feature `bundled`) | Historique et réglages |
| `strsim` | Similarité de noms (Jaro-Winkler) |
| `thiserror` / `anyhow` | Erreurs typées dans le core, contexte dans la CLI |
| `tracing` + `tracing-appender` | Journalisation dans `%LOCALAPPDATA%\CleanSlate\logs`, rotation quotidienne, 7 derniers fichiers conservés (ENF-10) |
| `chrono`, `uuid` | Dates et identifiants de session |
| `csbindgen` (build) | Génération des déclarations C# |
| `tempfile`, `criterion` (dev) | Tests et benchmarks |

### 3.2 Conventions Rust

- Aucun `unwrap()` ni `expect()` hors tests ; erreurs remontées avec `?` et un `enum CoreError`. Les lints `clippy::unwrap_used` et `clippy::expect_used` sont refusés dans tout le workspace (`core/Cargo.toml`), et autorisés dans les tests par `core/clippy.toml`.
- Aucune panique ne doit traverser la frontière FFI : chaque fonction exportée est enveloppée dans `std::panic::catch_unwind`.
- Chaque appel `unsafe` est isolé dans une fonction courte, commentée (`// SAFETY: …`).
- Chemins manipulés en `PathBuf` / `OsString`, jamais en `String`, jusqu'à la sérialisation.
- Le mode simulation est un paramètre explicite de chaque opération destructive, `true` par défaut.

## 4. Interface FFI (Rust ↔ C#)

La frontière est volontairement étroite : une poignée opaque, une dizaine de fonctions `extern "C"`, et des données en JSON UTF-8. Ajouter une fonctionnalité ne change presque jamais la signature, seulement le JSON.

### 4.1 Fonctions exportées (`cleanslate-ffi`)

| Fonction | Entrée | Sortie (`data` du JSON) | Jalon |
| --- | --- | --- | --- |
| `cs_version()` | — | `{ "version": "0.1.0" }` | J1 ✔ |
| `cs_engine_new(config, len, out_error)` | `EngineConfig` (longueur 0 : valeurs par défaut, donc simulation) ; `out_error: *mut *mut ByteBuffer` | pointeur opaque `*mut EngineHandle`. En cas d'échec : null, et `*out_error` reçoit l'enveloppe `{ ok: false, error }` (à libérer par `cs_buffer_free`) ; en cas de succès, `*out_error` vaut null. `out_error` peut être null si l'appelant ne veut pas le détail | J1 ✔ |
| `cs_engine_free(engine)` | poignée (null ignoré) | — | J1 ✔ |
| `cs_buffer_free(buf)` | `*mut ByteBuffer` (null ignoré) | libère toute réponse renvoyée par Rust | J1 ✔ |
| `cs_list_apps(engine)` | — | `InstalledApp[]` (entrées masquées comprises), puis les applications du Store de l'utilisateur courant (`kind: store`, `package_full_name`, `package_family_name`, D48) | J2 ✔, compléments ✔ |
| `cs_scan_orphans(engine, opts, len, cb, ctx)` | `ScanOptions` (longueur 0 : défauts) ; rappel facultatif, appelable depuis un thread de travail, jamais après le retour | `ScanReport` (partiel et `cancelled: true` si annulé) | J3 ✔ |
| `cs_cancel(engine)` | — | annule l'opération en cours (drapeau atomique, remis à zéro à l'opération suivante) | J3 ✔ |
| `cs_clean(engine, plan, len, cb, ctx)` | `CleanPlan` (éléments du dernier scan uniquement ; rappel réservé) | `Session` (`simulated: true` si le moteur ou le plan est en simulation, D27) | J4 ✔ |
| `cs_restore(engine, req, len)` | `RestoreRequest { session_id, item_ids?, dry_run }` | `Session` (`errors` = éléments non restaurés) | J4 ✔ |
| `cs_list_sessions(engine)` | — | `Session[]` (sessions interrompues reprises au passage) | J4 ✔ |
| `cs_purge_quarantine(engine, req, len)` | `PurgeRequest { older_than_days, dry_run }` | `PurgeResult { freed_bytes, simulated }` | J4 ✔ |
| `cs_uninstall(engine, req, len, cb, ctx)` | `UninstallRequest { app_id, quiet, create_restore_point, dry_run }` | `UninstallResult` (commande, point de restauration, code de sortie, `still_installed`, `simulated`) ; le rappel reçoit un `UninstallProgress` JSON à chaque étape et au plus toutes les 250 ms (EF-21, D49) ; `cs_cancel` arrête l'attente d'un `.exe` sans le tuer, et demande à Windows Installer d'annuler (retour arrière) une désinstallation MSI | J5 ✔, compléments ✔ |
| `cs_scan_app_leftovers(engine, req, len, cb, ctx)` | `AppLeftoversRequest { app_id, uninstaller_missing }` | `ScanReport` (vide si l'application est toujours installée ; avec `uninstaller_missing`, restes cherchés quand même et plafonnés à « Moyenne », refusé si le désinstalleur existe, D45) | J5 ✔ |

Toutes les requêtes portent `dry_run`, vrai par défaut : une action réelle exige `EngineConfig.dry_run = false` **et** `dry_run = false` dans la requête (D27).

### 4.2 Conventions

- **Chaînes** : entrées en `(*const u8, i32)` UTF-8 ; sorties en `*mut ByteBuffer { ptr: *mut u8, len: usize, cap: usize }` (`nuint` côté C#) alloué par Rust et libéré **uniquement** par `cs_buffer_free`.
- **Enveloppe de réponse** : `{ "ok": true, "data": … }` ou `{ "ok": false, "error": { "code": "ACCESS_DENIED", "message": "…", "path": "…" | null } }`.
- **Codes d'erreur** : côté moteur, `NOT_FOUND`, `ACCESS_DENIED`, `IO_ERROR`, `REGISTRY_ERROR`, `INVALID_INPUT`, `PROTECTED`, `BUSY`, `CANCELLED` (`CoreError::code`), plus `INTERNAL_PANIC` et `SERIALIZATION_ERROR` produits par `cleanslate-ffi`. Côté C#, `EngineClient` ajoute `ENGINE_CREATE_FAILED` (échec sans détail), `NULL_RESPONSE` et `INVALID_RESPONSE`.
- **Paniques** : `catch_unwind` sur chaque fonction ; une panique devient l'erreur `INTERNAL_PANIC`.
- **Progression** : callback `extern "C" fn(ctx: *mut c_void, json: *const u8, len: i32)` recevant `{ "phase", "current", "total", "path" }`. Le moteur n'envoie jamais plus d'un événement de progression par 250 ms, et en envoie au moins un toutes les 250 ms tant que le travail avance (ENF-06). Côté C#, méthode statique `[UnmanagedCallersOnly]` + `GCHandle` pour retrouver l'objet, puis passage au thread UI via `DispatcherQueue`.
- **Threads** : les appels longs sont faits depuis `Task.Run` côté C# ; le moteur n'accepte qu'une opération longue à la fois par poignée (erreur `BUSY` sinon).
- **Annulation** : `cs_cancel` lève un `AtomicBool` consulté dans les boucles de scan ; résultat partiel avec `"cancelled": true`.
- **Génération** : `build.rs` appelle `csbindgen` qui écrit `app/CleanSlate.Interop/NativeMethods.g.cs` (réécrit seulement si son contenu change) ; ce fichier est versionné et n'est jamais modifié à la main. La CI échoue s'il n'est pas à jour.

### 4.3 Wrapper C# (`EngineClient`)

Le projet `CleanSlate.Interop` expose une classe `EngineClient : IDisposable`, construite avec un `EngineConfig` facultatif (simulation par défaut). La poignée native est tenue par un `SafeHandle` qui appelle `cs_engine_free`, et chaque `ByteBuffer` est libéré dans un bloc `finally`. Méthodes asynchrones typées : `Task<string> VersionAsync(CancellationToken)` (J1), puis `Task<IReadOnlyList<InstalledApp>> ListAppsAsync(CancellationToken)` (J2), `Task<ScanReport> ScanOrphansAsync(ScanOptions, IProgress<ScanProgress>, CancellationToken)` (J3)… Il désérialise le JSON avec `System.Text.Json` (génération de source, propriétés en `snake_case`), transforme `ok: false` en `EngineException` (propriétés `Code` et `Path`), y compris à la création du moteur (le contenu de `out_error` donne le vrai code, par exemple `INVALID_INPUT`), et relie `CancellationToken` à `cs_cancel` (J3). Aucun autre projet ne touche aux pointeurs ; `CleanSlate.Presentation` ne le référence que pour les records du modèle.

## 5. Modèle de données

Ces types sont définis une fois en Rust (`model.rs`, serde en `snake_case`) et reproduits à l'identique en C# (records immuables) au fil des jalons. Au J1, C# ne reproduit que `EngineConfig`, `EngineVersion` et `EngineError` (`CleanSlate.Interop/EngineModels.cs`).

```rust
pub struct InstalledApp {
    pub id: String,              // hash stable : vue registre + nom de la sous-clé Uninstall
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_date: Option<NaiveDate>,
    pub install_location: Option<PathBuf>,
    pub estimated_size_kb: Option<u64>,
    pub uninstall_string: Option<String>,
    pub quiet_uninstall_string: Option<String>,
    pub is_msi: bool,            // WindowsInstaller = 1 ou ProductCode GUID
    pub product_code: Option<String>,
    pub registry_view: RegistryView, // Hklm64 | Hklm32 | Hkcu
    pub kind: AppKind,           // Win32 | Store (lecture seule, v1.1)
    pub hidden_reason: Option<HiddenReason>, // masquée par défaut (EF-02) ; None = visible
    pub installer: InstallerKind,
    pub package_full_name: Option<String>, pub package_family_name: Option<String>, // Store (EF-04)
}

// Le moteur renvoie toutes les entrées nommées ; le filtre « composants système » est côté ViewModel.
// Les entrées sans DisplayName sont écartées par le moteur (rien à afficher).
pub enum HiddenReason { SystemComponent, Update } // SystemComponent = 1 | ParentKeyName / ReleaseType / KB…
pub enum InstallerKind { Msi, InnoSetup, Nsis, Unknown } // déduit de WindowsInstaller, du suffixe « _is1 », de l'UninstallString

pub enum LeftoverKind { Folder, File, RegistryKey, RegistryValue, Shortcut, ScheduledTask, Service }
pub enum Confidence { High, Medium, Low }

pub struct Leftover {
    pub id: String,
    pub kind: LeftoverKind,
    pub location: String,        // chemin ou clé complète (HKCU\Software\…)
    pub size_bytes: u64,
    pub last_modified: Option<DateTime<Utc>>,
    pub confidence: Confidence,
    pub reasons: Vec<String>,    // explications lisibles : « aucune app installée de cet éditeur »
    pub guessed_app: Option<String>,
}

pub struct LeftoverGroup { pub label: String, pub publisher: Option<String>, pub items: Vec<Leftover>, pub total_bytes: u64 }
pub struct ScanReport { pub groups: Vec<LeftoverGroup>, pub errors: Vec<ItemError>, pub duration_ms: u64, pub cancelled: bool }

pub struct ItemError { pub code: String, pub message: String, pub path: Option<String> } // code de CoreError

pub enum Origin { Orphans, App { app_id: String } } // JSON : {"kind":"orphans"} | {"kind":"app","app_id":"…"}
pub struct CleanPlan { pub item_ids: Vec<String>, pub dry_run: bool, pub origin: Origin, pub confirmation: Option<String> } // dry_run absent = true ; « QUARANTAINE » au-delà du plafond
pub struct RestoreRequest { pub session_id: Uuid, pub item_ids: Option<Vec<String>>, pub dry_run: bool } // dry_run absent = true
pub struct PurgeRequest { pub older_than_days: u32, pub dry_run: bool }                           // dry_run absent = true
pub struct PurgeResult { pub freed_bytes: u64, pub simulated: bool }

// Désinstallation (J5).
pub struct UninstallRequest { pub app_id: String, pub quiet: bool, pub create_restore_point: bool, pub dry_run: bool } // false, true, true par défaut
pub struct UninstallResult {
    pub app_id: String, pub display_name: String, pub command: String, pub simulated: bool,
    pub restore_point_created: bool, pub restore_point_error: Option<String>,
    pub exit_code: Option<u32>, pub timed_out: bool, pub cancelled: bool, pub still_installed: bool,
}
pub struct AppLeftoversRequest { pub app_id: String, #[serde(default)] pub uninstaller_missing: bool }

pub struct Session {
    pub id: Uuid, pub created_at: DateTime<Utc>, pub origin: Origin,
    pub items: Vec<QuarantinedItem>, pub freed_bytes: u64, pub errors: Vec<ItemError>,
    pub status: SessionStatus,   // Completed | Partial | Restored | Purged
    pub simulated: bool,         // simulation : rien n'a été déplacé ni enregistré
}

pub enum QuarantineStatus { Quarantined, Restored, Purged, PendingReboot, Failed }

pub struct QuarantinedItem {
    pub id: String,                       // = Leftover.id, unique dans la session
    pub kind: LeftoverKind,
    pub original_location: String,        // chemin ou clé complète
    pub quarantine_path: PathBuf,         // files\<id>\… ou <id>.reg / .xml / .json
    pub size_bytes: u64,
    pub sha256: Option<String>,           // fichier : son empreinte ; dossier : empreinte de la liste
                                          // triée (chemin relatif, SHA-256) de ses fichiers
    pub original_acl: Option<String>,     // descripteur de sécurité d'origine, format SDDL
    pub quarantined_at: DateTime<Utc>,
    pub restored_at: Option<DateTime<Utc>>,
    pub status: QuarantineStatus,
    pub error: Option<ItemError>,         // rempli si status = Failed
}

// engine.rs : configuration envoyée par l'interface à cs_engine_new.
pub struct EngineConfig {
    pub dry_run: bool,               // absent du JSON = true (règle de sécurité)
    pub data_dir: Option<PathBuf>,   // None = %LOCALAPPDATA%\CleanSlate
    pub exclusions: Vec<String>,     // dossiers ou clés jamais proposés (EF-20)
}

// Scan (J3).
pub struct ScanOptions { pub dry_run: bool, pub include_registry: bool, pub include_shortcuts: bool, pub include_tasks_and_services: bool } // tous true par défaut
pub enum UninstallStep { RestorePoint, Launching, Waiting, Verifying, Done }
pub struct JobProcess { pub pid: u32, pub name: String }
pub struct UninstallProgress { pub step: UninstallStep, pub elapsed_ms: u64, pub percent: Option<f64>, pub action: Option<String>, pub processes: Vec<JobProcess> } // EF-21
pub enum ScanPhase { Listing, Analyzing, Done }
pub struct ScanProgress { pub phase: ScanPhase, pub current: u64, pub total: u64, pub path: String }
```

**Stockage** : `%LOCALAPPDATA%\CleanSlate\` contient `cleanslate.db` (SQLite : tables `sessions` et `items` ; les tables `exclusions` et `settings` existent et sont testées, mais l'application v1.0 ne les utilise pas), `settings.json` (réglages de l'application et exclusions, transmises au moteur dans `EngineConfig.exclusions`, décision D40), `quarantine\<session_id>\` (fichiers déplacés, fichiers `.reg` exportés, `manifest.json`) et `logs\` (fichiers quotidiens `cleanslate.AAAA-MM-JJ.log`, 7 conservés, ENF-10).

## 6. Algorithme de détection des restes

Un élément est orphelin s'il ne peut être rattaché à aucune application installée ; il est ensuite noté de 0 à 100, et seule la confiance « Élevée » (≥ 60) est précochée.

### 6.1 Étapes du scan des orphelins

1. **Construire l'ensemble connu** à partir de l'inventaire : noms et éditeurs normalisés, `InstallLocation`, dossiers déduits de `DisplayIcon` et `UninstallString`, plus les exécutables des services et processus en cours.
2. **Énumérer les candidats** : enfants directs de chaque racine (profondeur 1), et profondeur 2 sous un dossier « éditeur » (ex. `AppData\Roaming\Mozilla\Firefox`). Registre : sous-clés de profondeur 1 et 2 de `HKCU\Software`, `HKLM\SOFTWARE`, `HKLM\SOFTWARE\WOW6432Node`.
3. **Écarter** ce qui est en liste noire ou en exclusions utilisateur (`safety`), et toute jonction ou lien symbolique.
4. **Rapprocher** chaque candidat de l'ensemble connu (6.2). Rattaché → ignoré. Non rattaché → orphelin.
5. **Noter** l'orphelin (6.3), calculer sa taille et sa date de dernière modification.
6. **Regrouper** les orphelins par nom normalisé ou éditeur commun, en un `LeftoverGroup` par application déduite.

Candidats ajoutés par les compléments de la v1.0 :

- **Raccourcis** des Bureaux et des deux racines du menu Démarrer (D20) : morts seulement selon `shortcut::verdict` ; un sous-dossier du menu Démarrer qui ne contient que des raccourcis morts est proposé à part, en Moyenne.
- **Tâches planifiées** (`%WINDIR%\System32\Tasks`, XML) et **services** (`ImagePath`) dont le programme a disparu (D47) : tâche Élevée seulement si le programme manque depuis plus de 30 jours (date du plus proche dossier parent encore présent), sinon Moyenne ; service plafonné à Moyenne ; Faible si le dossier appartient à une application installée.
- **Données de paquets du Store** dont le paquet n'est plus installé (D48) : Moyenne ; après la désinstallation d'une application du Store par CleanSlate, ses données sont Élevées.

### 6.2 Rapprochement

Normalisation : minuscules, suppression des accents, des numéros de version, des mentions d'architecture (`x64`, `64-bit`), des suffixes juridiques (`Inc`, `Ltd`, `LLC`, `GmbH`, `Corporation`, `SAS`) et de la ponctuation.

Un candidat est **rattaché** si au moins une condition est vraie :

- son chemin est égal à, ou contenu dans, un `InstallLocation` connu (ou le dossier de l'`UninstallString`) — ou, à l'inverse, il contient un tel dossier (D25) ;
- similarité Jaro-Winkler ≥ 0,90 avec un nom ou un éditeur installé, ou tous les mots du candidat contenus dans un nom installé ;
- il contient l'exécutable d'un service, d'un processus en cours ou d'une entrée Uninstall ;
- c'est un dossier éditeur et au moins une application de cet éditeur est encore installée ;
- son nom est celui d'un **programme présent sur le disque** sans entrée Uninstall : dossier de `Program Files`, `Program Files (x86)` ou `%LOCALAPPDATA%\Programs` contenant un `.exe` sur 3 niveaux (D22) ;
- son nom normalisé est vide (trop générique pour être jugé).

Les reparse points ne sont jamais proposés, ni traversés (D18).

### 6.3 Score de confiance

| Signal | Points |
| --- | --- |
| Aucun nom ni éditeur installé avec similarité ≥ 0,75 | +40 |
| Dernière modification il y a plus de 30 jours | +20 |
| Raccourci dont la cible n'existe plus | +60 |
| Le nom correspond à une application désinstallée par CleanSlate (historique) | +30 |
| Ne contient aucun exécutable | +10 |
| Modifié il y a moins de 7 jours | −30 |
| Nom générique (`Temp`, `Cache`, `Common`, `Shared`, `Data`, `Config`, `Logs`…) | −25 |
| Contient des `.exe` dans `Program Files` sans entrée Uninstall (logiciel portable probable) | −25 |
| Clé de registre contenant moins de 3 valeurs et aucune sous-clé | −10 |

Plafonds appliqués après le score (D21, D23) : un élément de `Documents` est toujours **Faible** (dossier personnel possible) ; un élément de profondeur 2 sous le dossier d'un éditeur encore installé est au plus **Moyenne**. Le signal « historique » (+30) arrive avec l'historique en J4.

Niveaux : **Élevée** ≥ 60, **Moyenne** 35 à 59, **Faible** < 35. Chaque signal retenu est ajouté en clair à `reasons` pour que l'utilisateur comprenne la proposition. Ces seuils sont des valeurs de départ, à ajuster sur le jeu de test d'acceptation.

### 6.4 Restes d'une application précise

Avant de lancer le désinstalleur, le moteur capture une **empreinte** de l'application : nom, éditeur, `InstallLocation`, exécutables, `ProductCode`. Après désinstallation, il cherche dans les mêmes racines tout ce qui correspond à cette empreinte : `InstallLocation` restant (Élevée), nom exact ou similarité ≥ 0,85 (Élevée), dossier éditeur sans autre application de cet éditeur (Moyenne), clés portant le `ProductCode` (Élevée), raccourcis pointant vers l'ancien dossier (Élevée).

## 7. Désinstallation, quarantaine et restauration

Rien n'est jamais supprimé directement : chaque élément passe par la quarantaine, d'où il peut revenir à l'identique pendant 30 jours.

### 7.1 Désinstallation complète

1. Capturer l'empreinte de l'application (6.4).
2. Créer un point de restauration (`SRSetRestorePointW`) si la protection système est active ; un échec est signalé mais n'empêche pas de continuer après confirmation.
3. MSI : `MsiConfigureProductExW(INSTALLSTATE_ABSENT)` avec un gestionnaire d'interface externe (`MsiSetExternalUIRecord`, interface interne `NONE`, `REBOOT=ReallySuppress`) qui donne le pourcentage et l'action en cours (EF-21, D49) ; si l'API échoue, repli sur la commande ci-dessous. Store : `RemovePackageAsync` (utilisateur courant seulement, progression réelle, pas de point de restauration, D48). Construire la commande : `msiexec.exe /x {ProductCode}` pour un MSI (D16), sinon `UninstallString` découpée proprement (exécutable entre guillemets + arguments, jamais passée à `cmd /c`). Mode silencieux : `QuietUninstallString` ou `/qn` pour MSI ; pour Inno Setup (`InstallerKind::InnoSetup`), les options `/SILENT`, `/VERYSILENT`, `/SUPPRESSMSGBOXES` et `/NORESTART` déclarées sont remplacées par `/VERYSILENT /NORESTART` ; jamais `/SUPPRESSMSGBOXES`, qui répondrait aux questions par la valeur par défaut choisie par l'auteur, éventuellement « supprimer les réglages », hors quarantaine (D60). Les noms nus (`MsiExec.exe`, `rundll32`) sont résolus dans `System32`, jamais par le `PATH` ; un chemin non guillemeté ambigu prend le premier candidat qui existe ; `MsiExec /I{GUID}` devient `/X{GUID}` ; `cmd`, PowerShell et les hôtes de scripts sont refusés (`UNSAFE_UNINSTALLER`, D34) ; programme absent → `NO_UNINSTALLER`.
4. Lancer le processus (créé suspendu, puis placé dans un **Job Object**) et attendre que tous les processus du job soient terminés (certains désinstalleurs se copient dans `%TEMP%` et rendent la main immédiatement). Délai maximal : 30 min. Ni le délai ni l'annulation ne tuent le désinstalleur (D35). Toutes les 250 ms, la liste des processus du job (`JobObjectBasicProcessIdList`, nom par `QueryFullProcessImageNameW`) est envoyée à l'interface ; aucun pourcentage n'est inventé pour un `.exe` (EF-21).
5. Vérifier que l'entrée Uninstall a disparu. Si elle est toujours là : l'utilisateur a peut-être annulé → proposer de la retirer avec les restes (confiance Moyenne).
6. Lancer `cs_scan_app_leftovers` et afficher le résultat.

### 7.2 Quarantaine

- **Fichiers et dossiers** : déplacés avec `MoveFileExW` vers `quarantine\<session>\files\<id>\` si même volume ; sinon copie vérifiée puis suppression de l'original.
- **Clés de registre** : export récursif en `.reg` (format `regedit` UTF-16 LE, re-importable) avant suppression.
- **Raccourcis, tâches, services** : raccourci (Bureaux, menu Démarrer, D20) déplacé comme un fichier ; sous-dossier du menu Démarrer seulement une fois vide, traité après ses raccourcis et restauré avant eux. Tâche : XML complet lu par le Planificateur (`ITaskService`) et écrit dans `tasks\<id>.xml` avant `DeleteTask` ; restauration par `RegisterTask` (même chemin, même nom, `TASK_CREATE`). Service : `ServiceConfig` complète (type, démarrage et démarrage différé, contrôle d'erreur, chemin, groupe, dépendances, compte, nom, description, actions de récupération) et clé `HKLM\SYSTEM\CurrentControlSet\Services\<nom>` (`.json` + `.reg`) dans `services\<id>.json` avant `DeleteService` ; restauration par `CreateServiceW`, `ChangeServiceConfig2W` puis réimport de la clé (D47). Les trois passent par des traits injectables (`TaskStore`, `ServiceControl`, `PackageStore`) : un moteur de test refuse tout.
- **Données d'un paquet du Store** : dossier `%LOCALAPPDATA%\Packages\<famille>` déplacé comme un dossier, clés `AppContainer\Storage\<pfn>` et `AppModel\SystemAppData\<pfn>` comme des clés, après validation contre la liste des paquets lue juste avant (D48).
- **Manifeste** : `manifest.json` liste chaque élément sous forme de `QuarantinedItem` (§ 5 : chemin d'origine, chemin en quarantaine, taille, SHA-256, ACL d'origine en SDDL, horodatages, statut), plus, pour un dossier, l'empreinte SHA-256 détaillée de chacun de ses fichiers (chemin relatif, taille, SHA-256) ; la `Session` renvoyée à l'interface ne contient que l'empreinte globale. Il est écrit **avant** chaque déplacement et mis à jour après, pour survivre à un plantage.
- **SHA-256** : calculé avec l'API Windows CNG (`BCryptHash`) via la crate `windows`, sans dépendance supplémentaire.
- **Fichier verrouillé** : proposé à la suppression au prochain redémarrage (`MOVEFILE_DELAY_UNTIL_REBOOT`, droits administrateur), listé comme tel (statut `PendingReboot`).
- **Refus** (élément laissé en place, erreur listée) : chemin ou clé protégés au moment d'agir, dossier contenant un lien ou une jonction (D29), tâche ou service ne satisfaisant plus les règles de D47 au moment d'agir, type non pris en charge (valeur de registre seule).
- **Clés** : la restauration réécrit l'instantané JSON (types exacts, D30) ; le `.reg` permet une restauration manuelle avec `regedit`.
- **ACL** : enregistrée en SDDL ; conservée par un déplacement sur le même volume (D31).
- **Reprise** : `manifest.json` porte l'élément en cours (`pending`) ; au prochain accès, il est marqué en quarantaine si le déplacement a eu lieu, sinon « Non déplacé » (copie partielle supprimée).

### 7.3 Restauration et purge

La restauration rejoue le manifeste à l'envers : fichiers remis à leur chemin d'origine (refus si un fichier y existe déjà, sauf confirmation), clés réimportées, tâches et services recréés. La purge supprime définitivement les sessions plus vieilles que le délai réglé (30 jours par défaut), au démarrage de l'application et sur demande.

## 8. Interface C# / WinUI 3

L'interface ne contient aucune logique métier : elle affiche, demande confirmation et délègue tout au moteur via `IEngineService`, dont l'implémentation appelle `EngineClient`.

Trois projets se partagent l'interface :

| Projet | Cible | Contenu |
| --- | --- | --- |
| `CleanSlate.App` | `net8.0-windows10.0.19041.0`, WinUI 3 | `App.xaml` (jetons), `MainWindow`, pages (`Views/`), implémentations Windows des services (`Services/EngineService`…), textes `.resw` |
| `CleanSlate.Presentation` | `net8.0`, sans WinUI | ViewModels (`ViewModels/`) et interfaces de services (`Services/`) ; testable par xUnit sans interface graphique |
| `CleanSlate.Interop` | `net8.0` | `NativeMethods.g.cs`, `EngineClient`, records du modèle |

### 8.1 Organisation MVVM

| Vue (Page) | ViewModel | Rôle |
| --- | --- | --- |
| `HomePage` | `HomeViewModel` | Tableau de bord : nombre d'apps, dernier scan, espace récupérable, bouton « Analyser » |
| `AppsPage` | `AppsViewModel` | Liste des applications, recherche, tri, filtre « composants système » (côté ViewModel), « Désinstallation classique » (désinstalleur officiel seul : EF-12 sans EF-13) et « Désinstaller complètement », tous deux visibles mais désactivés jusqu'à J5 |
| `OrphansPage` | `OrphansViewModel` | Scan sans nom, résultats groupés, cases à cocher, filtre par confiance |
| `UninstallWizardPage` | `UninstallWizardViewModel` | Assistant en 4 étapes : confirmation, désinstallation, restes, résultat |
| `HistoryPage` | `HistoryViewModel` | Sessions passées, restauration totale ou partielle, purge |
| `SettingsPage` | `SettingsViewModel` | Exclusions, délai de quarantaine, thème, mode simulation, journaux |
| `CleanConfirmDialog` | — | `ContentDialog` récapitulatif obligatoire avant suppression |

Pages dans `CleanSlate.App/Views/`, ViewModels dans `CleanSlate.Presentation/ViewModels/`, textes dans `CleanSlate.App/Strings/fr-FR/Resources.resw`. Au J1, les cinq pages de navigation existent, mais seule `HomePage` est remplie. `UninstallWizardPage` arrive en J5 et `CleanConfirmDialog` en J4.

### 8.2 Services et injection

- `Microsoft.Extensions.Hosting` (`Host` générique et injection de dépendances) dans `App.xaml.cs` ; chaque page obtient son ViewModel par `App.GetService<T>()`.
- Interfaces dans `CleanSlate.Presentation/Services/`, implémentations dans `CleanSlate.App/Services/`, enregistrées dans le conteneur. `IEngineService` (façade de `EngineClient`, implémentée par `EngineService`, singleton créé en simulation ; seule `GetVersionAsync` existe au J1). À créer au fil des jalons : `INavigationService`, `IDialogService`, `ISettingsService`. Pas de service de localisation : les textes sont dans les `.resw` de `CleanSlate.App`, et les formats fr-FR (tailles en Ko / Mo / Go avec virgule, dates « 22 oct. 2026 ») sont produits par `CleanSlate.Presentation`.
- Navigation par `NavigationView` (volet gauche, « Réglages » en pied de volet) + `Frame`, dans `MainWindow`.
- `CommunityToolkit.Mvvm` : `[ObservableProperty]`, `[RelayCommand]` asynchrones, `IProgress<T>` pour la progression.

### 8.3 Paquets .NET

| Paquet | Version au J1 | Projet |
| --- | --- | --- |
| `Microsoft.WindowsAppSDK` | 1.8.260921001 (runtime embarqué, `WindowsAppSDKSelfContained`) | CleanSlate.App |
| `CommunityToolkit.Mvvm` | 8.4.2 | CleanSlate.Presentation |
| `Microsoft.Extensions.Hosting` | 8.x (8.0.1, alignée sur .NET 8) | CleanSlate.App |
| `xunit.v3`, `xunit.runner.visualstudio`, `Microsoft.NET.Test.Sdk` | 3.2.2, 4.0.1, 18.11.0 | CleanSlate.Tests |
| `NSubstitute`, `NSubstitute.Analyzers.CSharp` | 6.2.0, 1.0.17 | CleanSlate.Tests |

### 8.4 Règles d'interface

- Listes longues en `ItemsRepeater` / `ListView` virtualisées (plusieurs milliers d'éléments).
- Matériau Mica, thème clair / sombre suivant Windows, icônes Segoe Fluent Icons.
- Textes dans des fichiers `.resw` (`fr-FR` par défaut, `en-US` prêt).
- Badges de confiance : vert « Élevée », orange « Moyenne », gris « Faible », toujours accompagnés du mot (jamais la couleur seule).
- Les maquettes et la charte détaillée sont dans [docs/design.md](design.md) et [docs/maquettes/](maquettes/index.html).

## 9. Garde-fous de sécurité

Le module `safety` est le dernier rempart : toute action destructive appelle `safety::validate(target)` juste avant d'agir, même si le scan l'avait déjà filtrée.

### 9.1 Liste noire (codée en dur, non désactivable)

- **Chemins** : `%WINDIR%` et tout son contenu ; racine de chaque lecteur ; `Program Files`, `Program Files (x86)`, `ProgramData`, `AppData\Roaming`, `AppData\Local`, `AppData\LocalLow` eux-mêmes (seuls leurs sous-dossiers sont candidats) ; `Users\<nom>` et ses dossiers connus (Documents, Bureau, Téléchargements…) eux-mêmes ; `System Volume Information`, `$Recycle.Bin`, `Recovery`.
- **Racines de scan elles-mêmes** : chaque racine de EF-05 (`Program Files`, `Program Files (x86)`, `%APPDATA%`, `%LOCALAPPDATA%`, `%LOCALAPPDATA%\Programs`, `%PROGRAMDATA%`, `Documents`) et de EF-06 (`HKCU\Software`, `HKLM\SOFTWARE`, `HKLM\SOFTWARE\WOW6432Node`) est protégée en tant que telle ; seuls ses enfants sont candidats. Cela évite par exemple que `%LOCALAPPDATA%\Programs`, enfant direct de `%LOCALAPPDATA%`, soit proposé en bloc comme reste.
- **Noms de sous-dossiers** : `Microsoft`, `Windows`, `Windows Defender`, `Windows NT`, `Common Files`, `Packages` (Store), `WindowsApps`, `Temp`, `Microsoft.NET`, `dotnet`, `Intel`, `NVIDIA Corporation`, `AMD` (pilotes), `Package Cache`, `CleanSlate`.
- **Registre** : `HKLM\SYSTEM`, `HKLM\SAM`, `HKLM\SECURITY`, `HKLM\HARDWARE`, toute clé `Microsoft`, `Classes`, `Policies`, `Clients`, `RegisteredApplications`, `Wow6432Node` elle-même, `Windows`, `Uninstall` d'une application encore présente, `CleanSlate` et `CleanSlateTest` (zone de test, D24).

### 9.1 bis Exceptions étroites (décisions du porteur)

La liste noire ci-dessus n'est ni modifiée ni contournée : `check_path` et `validate_key` refusent toujours ces emplacements. Chaque exception est une fonction de validation **dédiée** de `safety.rs`, propre à un type d'élément et à une forme exacte, appelée juste avant d'agir ; les règles générales sont appliquées en premier (`validate_item_path`). Un test par cas autorisé et un test par cas voisin refusé.

| Exception | Fonctions | Autorisé exactement | Toujours refusé |
| --- | --- | --- | --- |
| Raccourci mort du menu Démarrer (D20) | `check/validate_start_menu_shortcut`, `check/validate_start_menu_folder` | fichier `.lnk` strictement sous `%APPDATA%` ou `%PROGRAMDATA%\Microsoft\Windows\Start Menu\Programs` ; sous-dossier seulement vide | racines, autres extensions, hors `Programs`, noms protégés et dossiers d'outils Windows (`WINDOWS_START_MENU_FOLDERS`), `..`, exclusions |
| Raccourci « mort » (`shortcut.rs`) | `verdict` | cible locale absente sur un lecteur fixe présent | MSI annoncé (Darwin), AppUserModelID `Famille!App`, réseau (UNC, lecteur réseau), amovible, lettre absente, variable non résolue |
| Tâche planifiée (D47) | `check_task_path`, `validate_task` | hors `\Microsoft\`, actions toutes `Exec`, chaque exécutable local absolu absent hors `%WINDIR%` | `\Microsoft\` (toute casse, partout), handler COM, courriel, message, commande relative, exécutable présent ou sous `%WINDIR%` |
| Service (D47) | `check_service_key`, `validate_service` | Win32 arrêté, exécutable local absent hors `%WINDIR%`, sans dépendant, compte intégré ; clé `HKLM\SYSTEM\CurrentControlSet\Services\<nom>` exactement | pilotes, service en cours, `svchost.exe`, `ServiceDll`, `%SystemRoot%`, dépendants, compte utilisateur ; la clé `Services`, une sous-clé, une autre clé de `HKLM\SYSTEM` |
| Données d'un paquet du Store (D48) | `check/validate_package_folder`, `check/validate_package_key` | `%LOCALAPPDATA%\Packages\<famille>` et `…\AppContainer\Storage\<pfn>`, `…\AppModel\SystemAppData\<pfn>` exactement, famille bien formée absente des paquets installés | `Packages` et les clés parentes, sous-dossiers et sous-clés, paquet installé, liste vide, familles `Microsoft*`, `Windows.*`, éditeur `cw5n1h2txyewy` |

### 9.2 Règles d'exécution

- Chemins canonisés (`GetFinalPathNameByHandleW`) puis comparés sans tenir compte de la casse ; un chemin qui sort de sa racine après canonisation est refusé.
- `FILE_ATTRIBUTE_REPARSE_POINT` détecté → l'élément lui-même peut être traité, mais jamais traversé.
- Plafond par session : au-delà de 20 Go ou 50 000 fichiers, confirmation supplémentaire avec saisie du mot « QUARANTAINE » (vocabulaire fixe de design § 5 : jamais « supprimer » tant que l'élément est restaurable).
- Mode simulation actif par défaut au premier lancement, jusqu'à ce que l'utilisateur le désactive dans les réglages.
- Tests obligatoires : un test unitaire par entrée de la liste noire et par racine de scan, un test avec une jonction piégée vers `C:\Windows`, un test `../` dans un nom.

## 10. Tests, CI, compilation et packaging

Le moteur est testé sans jamais toucher au vrai système : racines de scan et ruche de registre sont injectables, et pointent vers des données de test.

### 10.1 Stratégie de test

| Niveau | Outil | Contenu |
| --- | --- | --- |
| Unitaire Rust | `cargo test` | Normalisation, similarité, score, liste noire, découpage d'`UninstallString`, manifeste |
| Intégration Rust | `cargo test` + `tempfile` | Arborescences factices (`tests/fixtures`), clés créées sous `HKCU\Software\CleanSlateTest\` puis supprimées, cycle quarantaine → restauration à l'identique |
| FFI | `cargo test` + xUnit v3 | Aller-retour JSON, libération mémoire, panique convertie en erreur, annulation |
| C# | xUnit v3 + NSubstitute | `EngineClient` contre la vraie `cleanslate.dll` ; ViewModels de `CleanSlate.Presentation` avec un `IEngineService` simulé |
| Couverture | `cargo llvm-cov` | Moteur Rust ≥ 80 % des lignes (ENF-12), vérifiée en CI |
| Performance | `criterion` | Scan de 500 000 fichiers factices (ENF-05) |
| Acceptation | Machine virtuelle Windows (Hyper-V ou Windows Sandbox) | Scénario des 5 applications (cahier des charges § 6.3) |

Abstractions pour les tests : `Engine::new(config, fs, registry)` reçoit par injection un trait `FileSystem` (`RealFileSystem` sur un dossier `tempfile`, ou `FakeFileSystem` en mémoire) et un trait `Registry` (`WinRegistry::sandboxed`, qui redirige toutes les ruches sous `HKCU\Software\CleanSlateTest\<uuid>`). `EngineConfig` ne porte que les réglages (`dry_run`, `data_dir`). L'aide de test `TestRegistryKey` crée cette sous-clé et la supprime à la fin du test, même en cas d'échec.

### 10.2 Compilation

`pwsh scripts/build.ps1 [-Configuration Debug|Release]` ; la configuration choisie s'applique **à la fois** à cargo (`Debug` → profil `dev`, `Release` → `--release`) et à dotnet :

1. `cargo build --workspace [--release]` depuis `core/` (la crate `cleanslate-ffi` fixe `[lib] name = "cleanslate"` et `crate-type = ["cdylib", "rlib"]`, `rlib` servant à ses tests ; son `build.rs` régénère `NativeMethods.g.cs`) ;
2. `dotnet build app/CleanSlate.sln -c <Configuration> -p:Platform=x64`. `cleanslate.dll` est copiée depuis `core/target/<debug|release>/` par un élément MSBuild `<Content>` (`CopyToOutputDirectory`) déclaré dans `CleanSlate.Interop.csproj`, le chemin étant défini dans `app/Directory.Build.props`. Elle arrive ainsi à côté de l'application et des tests ; la compilation s'arrête avec un message clair si la DLL manque.

Sortie : `app/CleanSlate.App/bin/x64/<Configuration>/net8.0-windows10.0.19041.0/win-x64/CleanSlate.App.exe`.

### 10.3 Intégration continue (GitHub Actions, `windows-latest`)

`.github/workflows/ci.yml` : `cargo fmt --all --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace` → `cargo llvm-cov --workspace --fail-under-lines 80` (ENF-12) → `scripts/build.ps1 -Configuration Release` → vérification que `NativeMethods.g.cs` est à jour → `dotnet test` → artefact de l'application (`CleanSlate-win-x64`).

### 10.4 Packaging

v1.0 : installeur **Inno Setup 6** (réalisé en J6) pour une application WinUI 3 non packagée ; manifeste d'application `requireAdministrator`. Une distribution packagée est écartée, car incompatible avec `requireAdministrator`. Signature de code (certificat) avant toute diffusion publique, sinon SmartScreen bloquera l'exécutable.

`pwsh scripts/package.ps1 [-SkipBuild]` :

1. `scripts/build.ps1 -Configuration Release` ;
2. `dotnet publish app/CleanSlate.App -c Release -p:Platform=x64 -r win-x64 --self-contained true -o artifacts/publish` : le runtime .NET 8 et le runtime Windows App SDK (`WindowsAppSDKSelfContained=true`) sont embarqués, le PC cible n'a rien à installer. La cible `PublishXamlResources` copie les `.xbf` et `CleanSlate.App.pri`, que la publication non packagée oublie (D41) ;
3. `ISCC installer/cleanslate.iss` avec `AppVersion` (lue dans `core/Cargo.toml`) et `PublishDir`.

Script `installer/cleanslate.iss` : `SetupIconFile=..\assets\branding\cleanslate.ico`, `PrivilegesRequired=admin`, installation dans `{autopf}\CleanSlate` (64 bits, Windows 10 2004 minimum), raccourci dans le menu Démarrer et, en option, sur le bureau, licence MIT affichée, assistant en français. La désinstallation de CleanSlate garde `%LOCALAPPDATA%\CleanSlate` (quarantaine restaurable, D43).

Sortie : `artifacts/installer/CleanSlate-<version>-setup.exe` (environ 59 Mo).
