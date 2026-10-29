# CleanSlate

Utilitaire Windows qui retrouve les restes d'applications déjà désinstallées et
désinstalle complètement une application. Tout ce qui est supprimé passe d'abord
par une quarantaine restaurable pendant 30 jours.

> **État : jalon J1 (socle).** L'application affiche la version du moteur ; aucune
> analyse ni suppression n'est encore disponible.

- Moteur : Rust (`core/`), exposé en `cleanslate.dll` par une API C en JSON.
- Interface : C# 12 / .NET 8 / WinUI 3 non packagée (`app/`), patron MVVM.
- Documentation : [cahier des charges](docs/cahier-des-charges.md),
  [spécifications techniques](docs/specifications-techniques.md),
  [design](docs/design.md), [maquettes](docs/maquettes/index.html).

## Prérequis

| Outil | Version | Installation |
| --- | --- | --- |
| Windows | 10 22H2 ou 11, x64 | — |
| Rust | stable, cible `x86_64-pc-windows-msvc` | [rustup](https://rustup.rs), puis `rustup component add rustfmt clippy` |
| Visual Studio 2022 (ou Build Tools) | charges « Développement Desktop C++ », « Développement .NET Desktop » et « Développement d'applications WinUI » | Visual Studio Installer |
| .NET SDK | 8.0 | `winget install Microsoft.DotNet.SDK.8` |
| PowerShell | 7.x | `winget install Microsoft.PowerShell` |
| cargo-llvm-cov (facultatif) | dernière version | `rustup component add llvm-tools-preview` puis `cargo install cargo-llvm-cov` |

Le runtime Windows App SDK est embarqué dans l'application : rien à installer à part.

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
docs/                 cahier des charges, spécifications, design, maquettes
```

## Licence

[MIT](LICENSE)
