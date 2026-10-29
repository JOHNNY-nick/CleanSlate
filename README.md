# CleanSlate

Dernière mise à jour : 29 oct. 2026

Utilitaire Windows qui retrouve les restes d'applications déjà désinstallées et
désinstalle complètement une application. Tout ce qui est supprimé passe d'abord
par une quarantaine restaurable pendant 30 jours.

> **État : jalon J1 (socle).** L'application affiche la version du moteur ; aucune
> analyse ni suppression n'est encore disponible.

- Moteur : Rust (`core/`), exposé en `cleanslate.dll` par une API C en JSON.
- Interface : C# 12 / .NET 8 / WinUI 3 non packagée (`app/`), patron MVVM.

## Documentation

| Fichier | Contenu |
| --- | --- |
| [`docs/cahier-des-charges.md`](docs/cahier-des-charges.md) | Objectifs, périmètre, exigences EF/ENF, jalons J1 à J6, acceptation |
| [`docs/specifications-techniques.md`](docs/specifications-techniques.md) | Architecture Rust + C#/WinUI 3, API FFI, modèle, algorithme, sécurité, tests, compilation |
| [`docs/design.md`](docs/design.md) | Principes, jetons de couleur, parcours, textes d'interface, accessibilité |
| [`docs/maquettes/`](docs/maquettes/index.html) | 8 écrans en HTML : ouvrir `index.html` dans un navigateur |
| [`docs/prompt-claude-code.md`](docs/prompt-claude-code.md) | Mode d'emploi avec Claude Code et prompts des jalons J1 à J6 |
| [`CLAUDE.md`](CLAUDE.md) | Consignes permanentes pour Claude Code (règles de sécurité, conventions) |

## Prérequis

| Outil | Version | Installation |
| --- | --- | --- |
| Windows | 10 22H2 ou 11, x64 | — |
| Rust | stable (1.99.1 au J1), cible `x86_64-pc-windows-msvc` | [rustup](https://rustup.rs), puis `rustup component add rustfmt clippy` |
| Visual Studio 2022 (ou Build Tools 2022) | charges « Développement Desktop C++ », « Développement .NET Desktop » et « Développement d'applications WinUI » | Visual Studio Installer |
| .NET SDK | 8.0 | `winget install Microsoft.DotNet.SDK.8 --source winget` |
| PowerShell | 7.x | `winget install Microsoft.PowerShell --source winget` |
| Git | — | `winget install Git.Git --source winget` |
| cargo-llvm-cov (facultatif) | dernière version | `rustup component add llvm-tools-preview` puis `cargo install cargo-llvm-cov` |

Le runtime Windows App SDK (1.8) est embarqué dans l'application : rien à installer à part.
Versions de référence des crates et paquets : [spécifications, § 2, § 3.1 et § 8.3](docs/specifications-techniques.md).

## Compiler

```powershell
pwsh scripts/build.ps1 -Configuration Debug     # ou Release
```

Le script compile d'abord le moteur Rust (`cargo build`), ce qui régénère
`app/CleanSlate.Interop/NativeMethods.g.cs`, puis la solution .NET (`dotnet build`).
MSBuild copie alors `cleanslate.dll` à côté de l'application et des tests.
`-Configuration` s'applique aux deux : `Debug` utilise le profil cargo `dev`,
`Release` utilise `--release`.

## Lancer

```powershell
app\CleanSlate.App\bin\x64\Debug\net8.0-windows10.0.19041.0\win-x64\CleanSlate.App.exe
```

L'application demande les droits administrateur (manifeste `requireAdministrator`) ;
pour la déboguer, lancez Visual Studio en administrateur et ouvrez `app/CleanSlate.sln`.
Le mode simulation est actif par défaut.

CLI de debug (sans interface) :

```powershell
cd core
cargo run -p cleanslate-cli -- version --json
```

## Tester

```powershell
cd core
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo llvm-cov --workspace          # couverture (objectif ≥ 80 %, ENF-12)
cd ..
dotnet test app/CleanSlate.sln -p:Platform=x64
```

Les tests n'écrivent que dans des dossiers temporaires et sous
`HKCU\Software\CleanSlateTest\<uuid>`, supprimée à la fin de chaque test.

## Structure

```text
core/                 workspace Cargo
  cleanslate-core/    logique métier (modèle, erreurs, accès disque et registre)
  cleanslate-ffi/     cleanslate.dll : API C extern "C", JSON UTF-8
  cleanslate-cli/     binaire de debug
app/
  CleanSlate.App/     WinUI 3 : vues, ViewModels, services
  CleanSlate.Interop/ NativeMethods.g.cs (généré, versionné) + EngineClient
  CleanSlate.Tests/   xUnit
scripts/build.ps1     compilation complète
docs/                 cahier des charges, spécifications, design, maquettes, prompts
.github/workflows/    CI GitHub Actions (fmt, clippy, tests, couverture, build)
```

## Licence

[MIT](LICENSE)
