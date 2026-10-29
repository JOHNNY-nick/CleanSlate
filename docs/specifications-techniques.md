# Spécifications techniques — CleanSlate

29 oct. 2026

## 1. Architecture

CleanSlate se compose de trois couches : une interface C# / WinUI 3, une DLL native `cleanslate.dll` qui expose une API C, et un moteur Rust qui seul touche au disque, au registre et aux processus.

```text
┌──────────────────── Interface — C# / .NET 8 / WinUI 3 ────────────────────┐
│  Vues XAML  ──▶  ViewModels  ──▶  Services  ──▶  EngineClient              │
│  (pages,         (MVVM            (IEngineService,  (CleanSlate.Interop)   │
│   dialogues)      Toolkit)         DI)                     │               │
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

```text
cleanslate/
├─ CLAUDE.md                  # consignes permanentes pour Claude Code
├─ README.md
├─ LICENSE                    # MIT
├─ docs/                      # cahier des charges, specs, design (export Markdown)
├─ core/                      # workspace Cargo
│  ├─ Cargo.toml
│  ├─ cleanslate-core/        # bibliothèque : toute la logique métier
│  │  └─ src/{inventory,scan,matcher,safety,uninstall,quarantine,history,model}.rs
│  ├─ cleanslate-ffi/         # cdylib : API C fine au-dessus du core
│  │  ├─ build.rs             # génère NativeMethods.g.cs via csbindgen
│  │  └─ src/lib.rs
│  └─ cleanslate-cli/         # binaire de debug : scan en ligne de commande
├─ app/
│  ├─ CleanSlate.sln
│  ├─ CleanSlate.App/         # WinUI 3 non packagé : vues, ViewModels, services
│  ├─ CleanSlate.Interop/     # P/Invoke généré (NativeMethods.g.cs, versionné) + EngineClient
│  └─ CleanSlate.Tests/       # xUnit
├─ tests/fixtures/            # arborescences et .reg de test
├─ scripts/build.ps1          # cargo build puis dotnet build, copie de la DLL
└─ .github/workflows/ci.yml
```

| Outil | Version | Rôle |
| --- | --- | --- |
| Rust (rustup) | stable, édition 2021, cible `x86_64-pc-windows-msvc` | Moteur |
| Visual Studio 2022 | charges « Développement .NET Desktop », « Développement d'applications WinUI » et « Développement Desktop C++ » (linker MSVC) | Interface, débogage mixte |
| .NET SDK | 8.0 | Interface |
| Windows App SDK | dernière version stable 1.x, application **non packagée** (`WindowsPackageType=None`) | WinUI 3 |
| PowerShell | 7.x (`pwsh`) | `scripts/build.ps1` |
| cargo-llvm-cov | dernière version (outil de développement, pas une dépendance du code) | Couverture du moteur (ENF-12) |
| Claude Code | dernière version | Développement assisté |
| Git + GitHub | — | Versionnement, CI |

L'application est **non packagée** : un `app.manifest` demande `requireAdministrator`, ce qui est incompatible avec un paquet MSIX. Pour déboguer, Visual Studio doit être lancé en administrateur. Le fichier `NativeMethods.g.cs` est généré par `build.rs` directement dans `app/CleanSlate.Interop/` et **versionné** dans Git, pour que la solution .NET compile même sans avoir lancé cargo.

Crate `cleanslate-cli` : un petit binaire (`cleanslate-cli scan --dry-run --json`) qui permet de tester le moteur sans l'interface, pratique pour apprendre Rust étape par étape.

## 3. Moteur Rust

Toute la logique métier vit dans `cleanslate-core`, une bibliothèque Rust sans code FFI ni `unsafe` (hors appels Win32 encapsulés), testable seule.

| Module | Responsabilité | Exigences |
| --- | --- | --- |
| `model` | Types partagés (`InstalledApp`, `Leftover`, `ScanReport`, `Session`…), sérialisation serde | Toutes |
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
| `winreg` | Lecture, export et suppression de clés de registre, vues 32 et 64 bits |
| `walkdir` + `rayon` | Parcours récursif et parallélisme du scan |
| `serde` + `serde_json` | Sérialisation des échanges avec C# |
| `rusqlite` (feature `bundled`) | Historique et réglages |
| `strsim` | Similarité de noms (Jaro-Winkler) |
| `thiserror` / `anyhow` | Erreurs typées dans le core, contexte dans la CLI |
| `tracing` + `tracing-appender` | Journalisation avec rotation |
| `chrono`, `uuid` | Dates et identifiants de session |
| `csbindgen` (build) | Génération des déclarations C# |
| `tempfile`, `criterion` (dev) | Tests et benchmarks |

### 3.2 Conventions Rust

- Aucun `unwrap()` ni `expect()` hors tests ; erreurs remontées avec `?` et un `enum CoreError`.
- Aucune panique ne doit traverser la frontière FFI : chaque fonction exportée est enveloppée dans `std::panic::catch_unwind`.
- Chaque appel `unsafe` est isolé dans une fonction courte, commentée (`// SAFETY: …`).
- Chemins manipulés en `PathBuf` / `OsString`, jamais en `String`, jusqu'à la sérialisation.
- Le mode simulation est un paramètre explicite de chaque opération destructive, `true` par défaut.

## 4. Interface FFI (Rust ↔ C#)

La frontière est volontairement étroite : une poignée opaque, une dizaine de fonctions `extern "C"`, et des données en JSON UTF-8. Ajouter une fonctionnalité ne change presque jamais la signature, seulement le JSON.

### 4.1 Fonctions exportées (`cleanslate-ffi`)

| Fonction | Entrée | Sortie (`data` du JSON) |
| --- | --- | --- |
| `cs_version()` | — | `{ "version": "0.1.0" }` |
| `cs_engine_new(config, len)` | `EngineConfig` | pointeur `*mut Engine` (null en cas d'erreur) |
| `cs_engine_free(engine)` | poignée | — |
| `cs_list_apps(engine)` | — | `InstalledApp[]` |
| `cs_scan_orphans(engine, opts, len, cb, ctx)` | `ScanOptions` | `ScanReport` |
| `cs_uninstall(engine, req, len, cb, ctx)` | `UninstallRequest` | `UninstallResult` |
| `cs_scan_app_leftovers(engine, req, len, cb, ctx)` | `{ "app_id": … }` | `ScanReport` |
| `cs_clean(engine, plan, len, cb, ctx)` | `CleanPlan` | `Session` |
| `cs_restore(engine, req, len)` | `{ "session_id", "item_ids"? }` | `RestoreResult` |
| `cs_list_sessions(engine)` | — | `Session[]` |
| `cs_purge_quarantine(engine, req, len)` | `{ "older_than_days" }` | `{ "freed_bytes" }` |
| `cs_cancel(engine)` | — | annule l'opération en cours (drapeau atomique) |
| `cs_buffer_free(buf)` | `*mut ByteBuffer` | libère toute réponse renvoyée par Rust |

### 4.2 Conventions

- **Chaînes** : entrées en `(*const u8, i32)` UTF-8 ; sorties en `*mut ByteBuffer { ptr, len, cap }` alloué par Rust et libéré **uniquement** par `cs_buffer_free`.
- **Enveloppe de réponse** : `{ "ok": true, "data": … }` ou `{ "ok": false, "error": { "code": "ACCESS_DENIED", "message": "…", "path": "…" } }`.
- **Paniques** : `catch_unwind` sur chaque fonction ; une panique devient l'erreur `INTERNAL_PANIC`.
- **Progression** : callback `extern "C" fn(ctx: *mut c_void, json: *const u8, len: i32)` appelé au plus toutes les 250 ms avec `{ "phase", "current", "total", "path" }`. Côté C#, méthode statique `[UnmanagedCallersOnly]` + `GCHandle` pour retrouver l'objet, puis passage au thread UI via `DispatcherQueue`.
- **Threads** : les appels longs sont faits depuis `Task.Run` côté C# ; le moteur n'accepte qu'une opération longue à la fois par poignée (erreur `BUSY` sinon).
- **Annulation** : `cs_cancel` lève un `AtomicBool` consulté dans les boucles de scan ; résultat partiel avec `"cancelled": true`.
- **Génération** : `build.rs` appelle `csbindgen` qui écrit `app/CleanSlate.Interop/NativeMethods.g.cs` ; ce fichier n'est jamais modifié à la main.

### 4.3 Wrapper C# (`EngineClient`)

Le projet `CleanSlate.Interop` expose une classe `EngineClient : IDisposable` aux méthodes asynchrones typées (`Task<IReadOnlyList<InstalledApp>> ListAppsAsync()`, `Task<ScanReport> ScanOrphansAsync(ScanOptions, IProgress<ScanProgress>, CancellationToken)`…). Il désérialise le JSON avec `System.Text.Json` (génération de source, propriétés en `snake_case`), transforme `ok: false` en `EngineException`, et relie `CancellationToken` à `cs_cancel`. Aucun autre projet ne touche aux pointeurs.

## 5. Modèle de données

Ces types sont définis une fois en Rust (`model.rs`, serde en `snake_case`) et reproduits à l'identique en C# (records immuables).

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
    pub kind: AppKind,           // Win32 | Store (lecture seule)
}

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

pub struct CleanPlan { pub item_ids: Vec<String>, pub dry_run: bool, pub origin: Origin } // Orphans | App(app_id)

pub struct Session {
    pub id: Uuid, pub created_at: DateTime<Utc>, pub origin: Origin,
    pub items: Vec<QuarantinedItem>, pub freed_bytes: u64, pub errors: Vec<ItemError>,
    pub status: SessionStatus,   // Completed | Partial | Restored | Purged
}
```

**Stockage** : `%LOCALAPPDATA%\CleanSlate\` contient `cleanslate.db` (SQLite : tables `sessions`, `items`, `exclusions`, `settings`), `quarantine\<session_id>\` (fichiers déplacés, fichiers `.reg` exportés, `manifest.json`) et `logs\`.

## 6. Algorithme de détection des restes

Un élément est orphelin s'il ne peut être rattaché à aucune application installée ; il est ensuite noté de 0 à 100, et seule la confiance « Élevée » (≥ 60) est précochée.

### 6.1 Étapes du scan des orphelins

1. **Construire l'ensemble connu** à partir de l'inventaire : noms et éditeurs normalisés, `InstallLocation`, dossiers déduits de `DisplayIcon` et `UninstallString`, plus les exécutables des services et processus en cours.
2. **Énumérer les candidats** : enfants directs de chaque racine (profondeur 1), et profondeur 2 sous un dossier « éditeur » (ex. `AppData\Roaming\Mozilla\Firefox`). Registre : sous-clés de profondeur 1 et 2 de `HKCU\Software`, `HKLM\SOFTWARE`, `HKLM\SOFTWARE\WOW6432Node`.
3. **Écarter** ce qui est en liste noire ou en exclusions utilisateur (`safety`), et toute jonction ou lien symbolique.
4. **Rapprocher** chaque candidat de l'ensemble connu (6.2). Rattaché → ignoré. Non rattaché → orphelin.
5. **Noter** l'orphelin (6.3), calculer sa taille et sa date de dernière modification.
6. **Regrouper** les orphelins par nom normalisé ou éditeur commun, en un `LeftoverGroup` par application déduite.

### 6.2 Rapprochement

Normalisation : minuscules, suppression des accents, des numéros de version, des mentions d'architecture (`x64`, `64-bit`), des suffixes juridiques (`Inc`, `Ltd`, `LLC`, `GmbH`, `Corporation`, `SAS`) et de la ponctuation.

Un candidat est **rattaché** si au moins une condition est vraie :

- son chemin est égal à, ou contenu dans, un `InstallLocation` connu ;
- similarité Jaro-Winkler ≥ 0,90 avec un nom ou un éditeur installé, ou tous les mots du candidat contenus dans un nom installé ;
- il contient l'exécutable d'un service, d'un processus en cours ou d'une entrée Uninstall ;
- c'est un dossier éditeur et au moins une application de cet éditeur est encore installée.

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

Niveaux : **Élevée** ≥ 60, **Moyenne** 35 à 59, **Faible** < 35. Chaque signal retenu est ajouté en clair à `reasons` pour que l'utilisateur comprenne la proposition. Ces seuils sont des valeurs de départ, à ajuster sur le jeu de test d'acceptation.

### 6.4 Restes d'une application précise

Avant de lancer le désinstalleur, le moteur capture une **empreinte** de l'application : nom, éditeur, `InstallLocation`, exécutables, `ProductCode`. Après désinstallation, il cherche dans les mêmes racines tout ce qui correspond à cette empreinte : `InstallLocation` restant (Élevée), nom exact ou similarité ≥ 0,85 (Élevée), dossier éditeur sans autre application de cet éditeur (Moyenne), clés portant le `ProductCode` (Élevée), raccourcis pointant vers l'ancien dossier (Élevée).

## 7. Désinstallation, quarantaine et restauration

Rien n'est jamais supprimé directement : chaque élément passe par la quarantaine, d'où il peut revenir à l'identique pendant 30 jours.

### 7.1 Désinstallation complète

1. Capturer l'empreinte de l'application (6.4).
2. Créer un point de restauration (`SRSetRestorePointW`) si la protection système est active ; un échec est signalé mais n'empêche pas de continuer après confirmation.
3. Construire la commande : `msiexec.exe /x {ProductCode}` pour un MSI, sinon `UninstallString` découpée proprement (exécutable entre guillemets + arguments, jamais passée à `cmd /c`). Mode silencieux : `QuietUninstallString` ou `/qn` pour MSI.
4. Lancer le processus dans un **Job Object** et attendre que tous les processus du job soient terminés (certains désinstalleurs se copient dans `%TEMP%` et rendent la main immédiatement). Délai maximal : 30 min.
5. Vérifier que l'entrée Uninstall a disparu. Si elle est toujours là : l'utilisateur a peut-être annulé → proposer de la retirer avec les restes (confiance Moyenne).
6. Lancer `cs_scan_app_leftovers` et afficher le résultat.

### 7.2 Quarantaine

- **Fichiers et dossiers** : déplacés avec `MoveFileExW` vers `quarantine\<session>\files\<id>\` si même volume ; sinon copie vérifiée puis suppression de l'original.
- **Clés de registre** : export récursif en `.reg` (format `regedit` UTF-16 LE, re-importable) avant suppression.
- **Raccourcis, tâches, services** : raccourci déplacé comme un fichier ; tâche exportée en XML avant suppression ; service : configuration sauvegardée en JSON avant `DeleteService`.
- **Manifeste** : `manifest.json` liste chaque élément (chemin d'origine, chemin en quarantaine, empreinte SHA-256 des fichiers, ACL d'origine) ; il est écrit **avant** chaque déplacement et mis à jour après, pour survivre à un plantage.
- **Fichier verrouillé** : proposé à la suppression au prochain redémarrage (`MOVEFILE_DELAY_UNTIL_REBOOT`), listé comme tel.

### 7.3 Restauration et purge

La restauration rejoue le manifeste à l'envers : fichiers remis à leur chemin d'origine (refus si un fichier y existe déjà, sauf confirmation), clés réimportées, tâches et services recréés. La purge supprime définitivement les sessions plus vieilles que le délai réglé (30 jours par défaut), au démarrage de l'application et sur demande.

## 8. Interface C# / WinUI 3

L'interface ne contient aucune logique métier : elle affiche, demande confirmation et délègue tout au moteur via `EngineClient`.

### 8.1 Organisation MVVM

| Vue (Page) | ViewModel | Rôle |
| --- | --- | --- |
| `HomePage` | `HomeViewModel` | Tableau de bord : nombre d'apps, dernier scan, espace récupérable, bouton « Analyser » |
| `AppsPage` | `AppsViewModel` | Liste des applications, recherche, tri, filtre, « Désinstaller complètement » |
| `OrphansPage` | `OrphansViewModel` | Scan sans nom, résultats groupés, cases à cocher, filtre par confiance |
| `UninstallWizardPage` | `UninstallWizardViewModel` | Assistant en 4 étapes : confirmation, désinstallation, restes, résultat |
| `HistoryPage` | `HistoryViewModel` | Sessions passées, restauration totale ou partielle, purge |
| `SettingsPage` | `SettingsViewModel` | Exclusions, délai de quarantaine, thème, mode simulation, journaux |
| `CleanConfirmDialog` | — | `ContentDialog` récapitulatif obligatoire avant suppression |

### 8.2 Services et injection

- `Microsoft.Extensions.DependencyInjection` + `Host` générique dans `App.xaml.cs`.
- `IEngineService` (façade de `EngineClient`, singleton), `INavigationService`, `IDialogService`, `ISettingsService`, `ILocalizationService`.
- Navigation par `NavigationView` (volet gauche) + `Frame`.
- `CommunityToolkit.Mvvm` : `[ObservableProperty]`, `[RelayCommand]` asynchrones, `IProgress<T>` pour la progression.

### 8.3 Règles d'interface

- Listes longues en `ItemsRepeater` / `ListView` virtualisées (plusieurs milliers d'éléments).
- Matériau Mica, thème clair / sombre suivant Windows, icônes Segoe Fluent Icons.
- Textes dans des fichiers `.resw` (`fr-FR` par défaut, `en-US` prêt).
- Badges de confiance : vert « Élevée », orange « Moyenne », gris « Faible », toujours accompagnés du mot (jamais la couleur seule).
- Les maquettes et la charte détaillée sont dans le dossier de design.

## 9. Garde-fous de sécurité

Le module `safety` est le dernier rempart : toute action destructive appelle `safety::validate(target)` juste avant d'agir, même si le scan l'avait déjà filtrée.

### 9.1 Liste noire (codée en dur, non désactivable)

- **Chemins** : `%WINDIR%` et tout son contenu ; racine de chaque lecteur ; `Program Files`, `Program Files (x86)`, `ProgramData`, `AppData\Roaming`, `AppData\Local`, `AppData\LocalLow` eux-mêmes (seuls leurs sous-dossiers sont candidats) ; `Users\<nom>` et ses dossiers connus (Documents, Bureau, Téléchargements…) eux-mêmes ; `System Volume Information`, `$Recycle.Bin`, `Recovery`.
- **Noms de sous-dossiers** : `Microsoft`, `Windows`, `Windows Defender`, `Windows NT`, `Common Files`, `Packages` (Store), `WindowsApps`, `Temp`, `Microsoft.NET`, `dotnet`, `Intel`, `NVIDIA Corporation`, `AMD` (pilotes), `Package Cache`, `CleanSlate`.
- **Registre** : `HKLM\SYSTEM`, `HKLM\SAM`, `HKLM\SECURITY`, `HKLM\HARDWARE`, toute clé `Microsoft`, `Classes`, `Policies`, `Clients`, `RegisteredApplications`, `Wow6432Node` elle-même, `Windows`, `Uninstall` d'une application encore présente.

### 9.2 Règles d'exécution

- Chemins canonisés (`GetFinalPathNameByHandleW`) puis comparés sans tenir compte de la casse ; un chemin qui sort de sa racine après canonisation est refusé.
- `FILE_ATTRIBUTE_REPARSE_POINT` détecté → l'élément lui-même peut être traité, mais jamais traversé.
- Plafond par session : au-delà de 20 Go ou 50 000 fichiers, confirmation supplémentaire avec saisie du mot « SUPPRIMER ».
- Mode simulation actif par défaut au premier lancement, jusqu'à ce que l'utilisateur le désactive dans les réglages.
- Tests obligatoires : un test unitaire par entrée de la liste noire, un test avec une jonction piégée vers `C:\Windows`, un test `../` dans un nom.

## 10. Tests, CI, compilation et packaging

Le moteur est testé sans jamais toucher au vrai système : racines de scan et ruche de registre sont injectables, et pointent vers des données de test.

### 10.1 Stratégie de test

| Niveau | Outil | Contenu |
| --- | --- | --- |
| Unitaire Rust | `cargo test` | Normalisation, similarité, score, liste noire, découpage d'`UninstallString`, manifeste |
| Intégration Rust | `cargo test` + `tempfile` | Arborescences factices (`tests/fixtures`), clés créées sous `HKCU\Software\CleanSlateTest\` puis supprimées, cycle quarantaine → restauration à l'identique |
| FFI | `cargo test` + xUnit | Aller-retour JSON, libération mémoire, panique convertie en erreur, annulation |
| C# | xUnit + Moq | ViewModels avec un `IEngineService` simulé |
| Performance | `criterion` | Scan de 500 000 fichiers factices (ENF-05) |
| Acceptation | Machine virtuelle Windows (Hyper-V ou Windows Sandbox) | Scénario des 5 applications (cahier des charges § 6.3) |

Abstractions pour les tests : trait `FileSystem` (réel / racines factices) et trait `Registry` (réel / sous-clé de test HKCU) injectés dans `Engine` via `EngineConfig`.

### 10.2 Compilation

`pwsh scripts/build.ps1 [-Configuration Debug|Release]` ; la configuration choisie s'applique **à la fois** à cargo (`Debug` → profil `dev`, `Release` → `--release`) et à dotnet :

1. `cargo build -p cleanslate-ffi [--release]` (la crate fixe `[lib] name = "cleanslate"` et `crate-type = ["cdylib"]` ; génère aussi `NativeMethods.g.cs`) ;
2. copie de `cleanslate.dll` dans le dossier de sortie de `CleanSlate.App` (cible MSBuild `<Content>` avec `CopyToOutputDirectory`) ;
3. `dotnet build app/CleanSlate.sln -c <Configuration> -p:Platform=x64`.

### 10.3 Intégration continue (GitHub Actions, `windows-latest`)

`cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test` → `cargo llvm-cov` (rapport de couverture du moteur, seuil 80 %) → build .NET → `dotnet test` → artefact zip de l'application.

### 10.4 Packaging

v1.0 : installeur **Inno Setup** (réalisé en J6) pour une application WinUI 3 non packagée ; le runtime Windows App SDK est embarqué (`WindowsAppSDKSelfContained`) ou installé par l'installeur ; manifeste d'application `requireAdministrator`. Le format MSIX est écarté, car incompatible avec `requireAdministrator`. Signature de code (certificat) avant toute diffusion publique, sinon SmartScreen bloquera l'exécutable.
