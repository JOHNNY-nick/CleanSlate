<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/branding/cleanslate-logo-horizontal-dark.svg">
  <img alt="CleanSlate" src="assets/branding/cleanslate-logo-horizontal.svg" width="420">
</picture>

# CleanSlate

Dernière mise à jour : 29 oct. 2026

Utilitaire Windows qui désinstalle complètement une application et retrouve les
restes d'applications déjà désinstallées.

## Pourquoi CleanSlate ?

En désinstallant des applications depuis les Paramètres de Windows, j'ai remarqué
que des fichiers restaient sur mon PC : dossiers dans AppData, clés de registre,
raccourcis… Je voulais pouvoir tout supprimer. J'ai donc imaginé une application
qui répond à deux besoins :

1. **Tout supprimer en une seule désinstallation** : l'application et tous les
   fichiers qu'elle laisse derrière elle.
2. **Retrouver les restes d'applications déjà désinstallées** et les supprimer,
   sans avoir besoin de connaître leur nom.

Par sécurité, rien n'est effacé directement : tout passe par une quarantaine
restaurable pendant 30 jours, et le mode simulation est actif par défaut.

> **État : jalon J4 (quarantaine).** Applications, restes orphelins, mise en quarantaine
> restaurable et historique fonctionnent ; le mode simulation est actif par défaut.
> La désinstallation complète arrive en J5.

- Moteur : Rust (`core/`), exposé en `cleanslate.dll` par une API C en JSON.
- Interface : C# 12 / .NET 8 / WinUI 3 non packagée (`app/`), patron MVVM.

## Prérequis

| Outil | Version | Installation |
| --- | --- | --- |
| Windows | 10 22H2 ou 11, x64 | — |
| Rust | stable (1.99.1 au J1), cible `x86_64-pc-windows-msvc`, avec `rustfmt` et `clippy` | [rustup](https://rustup.rs), puis `rustup component add rustfmt clippy` |
| cargo-llvm-cov | dernière version | `rustup component add llvm-tools-preview` puis `cargo install cargo-llvm-cov` |
| Visual Studio 2022 (ou Build Tools 2022) | charges « Développement Desktop C++ », « Développement .NET Desktop » et « Développement d'applications WinUI » | Visual Studio Installer |
| .NET SDK | 8.0 | `winget install Microsoft.DotNet.SDK.8 --source winget` |
| PowerShell | 7.x | `winget install Microsoft.PowerShell --source winget` |
| Git | — | `winget install Git.Git --source winget` |

Le runtime Windows App SDK (1.8) est embarqué dans l'application : rien à installer à part.
Versions de référence des crates et paquets : [spécifications, § 2, § 3.1 et § 8.3](docs/specifications-techniques.md).

**Toutes les commandes ci-dessous se lancent depuis la racine du dépôt.**

## Compiler

```powershell
pwsh scripts/build.ps1 -Configuration Debug     # ou Release
```

Le script compile d'abord le moteur Rust (`cargo build`), ce qui régénère
`app/CleanSlate.Interop/NativeMethods.g.cs`, puis la solution .NET (`dotnet build`).
MSBuild copie alors `cleanslate.dll` à côté de l'application et des tests.
`-Configuration` s'applique aux deux : `Debug` utilise le profil cargo `dev`,
`Release` utilise `--release`.

## Tester

```powershell
cargo fmt --manifest-path core/Cargo.toml --all --check
cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path core/Cargo.toml --workspace
cargo llvm-cov --manifest-path core/Cargo.toml --workspace   # couverture ≥ 80 % (ENF-12)
dotnet test app/CleanSlate.sln -p:Platform=x64
```

Les tests n'écrivent que dans des dossiers temporaires et sous
`HKCU\Software\CleanSlateTest\<uuid>`, supprimée à la fin de chaque test.

## Lancer

```powershell
app\CleanSlate.App\bin\x64\Debug\net8.0-windows10.0.19041.0\win-x64\CleanSlate.App.exe
```

L'exécutable s'appelle `CleanSlate.App.exe` (et non `CleanSlate.exe`) : une assembly
`CleanSlate.dll` écraserait la DLL native `cleanslate.dll`, car Windows ne distingue
pas la casse des noms de fichiers.

L'application demande les droits administrateur (manifeste `requireAdministrator`) ;
pour la déboguer, lancez Visual Studio en administrateur et ouvrez `app/CleanSlate.sln`.
Le mode simulation est actif par défaut.

Pour un essai rapide sans invite UAC, la variable d'environnement
`__COMPAT_LAYER=RunAsInvoker` lance l'application sans élévation :

```powershell
$env:__COMPAT_LAYER = 'RunAsInvoker'
app\CleanSlate.App\bin\x64\Debug\net8.0-windows10.0.19041.0\win-x64\CleanSlate.App.exe
Remove-Item Env:__COMPAT_LAYER
```

> ⚠️ **Réservé au développement.** Sans droits administrateur, l'application ne peut
> pas lire `HKLM` ni `Program Files` correctement : ses résultats seraient incomplets.
> Ne l'utilisez jamais ainsi pour une vraie analyse ou un vrai nettoyage.

CLI de debug (sans interface) :

```powershell
cargo run --manifest-path core/Cargo.toml -p cleanslate-cli -- version --json
cargo run --manifest-path core/Cargo.toml --release -p cleanslate-cli -- apps --json   # lecture seule
cargo run --manifest-path core/Cargo.toml --release -p cleanslate-cli -- scan --dry-run   # lecture seule
```

## Structure du dépôt

```text
core/                     workspace Cargo
  cleanslate-core/        logique métier (modèle, erreurs, accès disque et registre)
  cleanslate-ffi/         cleanslate.dll : API C extern "C", JSON UTF-8
  cleanslate-cli/         binaire de debug
app/
  CleanSlate.App/         WinUI 3 : vues, services Windows (CleanSlate.App.exe)
  CleanSlate.Presentation/ ViewModels et interfaces de services, sans WinUI
  CleanSlate.Interop/     NativeMethods.g.cs (généré, versionné) + EngineClient
  CleanSlate.Tests/       xUnit v3 + NSubstitute
scripts/build.ps1         compilation complète
docs/                     cahier des charges, spécifications, design, maquettes, prompts
.github/workflows/        CI GitHub Actions (fmt, clippy, tests, couverture, build)
```

## Documentation

| Fichier | Contenu |
| --- | --- |
| [`docs/cahier-des-charges.md`](docs/cahier-des-charges.md) | Objectifs, périmètre, exigences EF/ENF, jalons J1 à J6, acceptation |
| [`docs/specifications-techniques.md`](docs/specifications-techniques.md) | Architecture Rust + C#/WinUI 3, API FFI, modèle, algorithme, sécurité, tests, compilation |
| [`docs/design.md`](docs/design.md) | Principes, jetons de couleur, parcours, textes d'interface, accessibilité |
| [`docs/maquettes/`](docs/maquettes/index.html) | 8 écrans en HTML : ouvrir `index.html` dans un navigateur |
| [`docs/prompt-claude-code.md`](docs/prompt-claude-code.md) | Mode d'emploi avec Claude Code et prompts des jalons J1 à J6 |
| [`CLAUDE.md`](CLAUDE.md) | Consignes permanentes pour Claude Code (règles de sécurité, conventions) |

## Licence

[MIT](LICENSE)
