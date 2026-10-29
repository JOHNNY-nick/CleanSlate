# CleanSlate — consignes pour Claude Code

## Le projet
CleanSlate est un utilitaire Windows qui (1) détecte les restes d'applications
déjà désinstallées sans que l'utilisateur donne de nom, et (2) désinstalle
complètement une application : désinstalleur officiel puis suppression des restes.
Tout ce qui est supprimé passe d'abord par une quarantaine restaurable 30 jours.

C'est aussi un projet d'apprentissage de Rust : le propriétaire débute en Rust.

## Documents de référence (source de vérité)
- docs/cahier-des-charges.md : exigences EF-xx et ENF-xx, jalons J1 à J6
- docs/specifications-techniques.md : architecture, API FFI, modèle, algorithme, sécurité
- docs/design.md et docs/maquettes/ : jetons, écrans, parcours, textes d'interface
Lis la section concernée avant de coder. En cas de contradiction ou de manque,
demande-moi au lieu d'inventer. Cite l'ID d'exigence dans les commits et les tests.

## Architecture
- core/cleanslate-core : bibliothèque Rust, toute la logique métier
  (model, inventory, scan, matcher, safety, uninstall, quarantine, history)
- core/cleanslate-ffi : cdylib, [lib] name = "cleanslate" -> cleanslate.dll,
  API C extern "C", JSON UTF-8, ByteBuffer libéré par cs_buffer_free
- core/cleanslate-cli : binaire de debug
- app/CleanSlate.App : WinUI 3 non packagé (WindowsPackageType=None), .NET 8, MVVM
  (CommunityToolkit.Mvvm), app.manifest requireAdministrator ; installeur Inno Setup en J6
- app/CleanSlate.Interop : NativeMethods.g.cs (généré par csbindgen, versionné dans Git,
  ne jamais l'éditer) + EngineClient, seul code C# qui touche aux pointeurs
- app/CleanSlate.Tests : xUnit

## Commandes
- Compiler tout : pwsh scripts/build.ps1 -Configuration Debug
  (-Configuration s'applique à cargo et à dotnet : Debug = profil dev, Release = --release)
- Tests Rust : cargo test --workspace (depuis core/)
- Lint Rust : cargo fmt --all --check ; cargo clippy --workspace --all-targets -- -D warnings
- Tests C# : dotnet test app/CleanSlate.sln -p:Platform=x64
- Couverture Rust : cargo llvm-cov --workspace (depuis core/ ; outil de dev, objectif ≥ 80 %, ENF-12)
- CLI : cargo run -p cleanslate-cli -- scan --dry-run --json
Avant de dire qu'une tâche est finie : fmt, clippy et tous les tests passent.

## Règles de sécurité (non négociables)
1. Ne jamais exécuter d'opération destructive sur le vrai système de cette machine.
   Pas de suppression, déplacement ou écriture de registre hors des zones de test,
   et jamais cleanslate-cli ou l'application sans --dry-run, sauf si je le demande
   explicitement dans le message en cours.
2. Tests : uniquement des dossiers temporaires (tempfile) et la clé
   HKCU\Software\CleanSlateTest\<uuid>, créée puis supprimée par le test.
   Le moteur reçoit ses racines et sa ruche via EngineConfig (traits FileSystem, Registry).
3. Toute action destructive appelle safety::validate juste avant d'agir.
4. Ne jamais affaiblir la liste noire de safety.rs. Toute modification de ce fichier
   doit être ajoutée avec des tests et signalée explicitement dans ton résumé.
5. dry_run vaut true par défaut dans tous les types et toutes les API.
6. Ne jamais suivre de lien symbolique ni de point de jonction (reparse point).
7. Jamais de suppression directe : tout passe par quarantine avec manifeste écrit
   avant le déplacement.
8. UninstallString : découpage exécutable + arguments, jamais via cmd /c ni un shell.

## Conventions Rust
- Édition 2021, toolchain stable, cible x86_64-pc-windows-msvc.
- Pas de unwrap() ni expect() hors tests ; erreurs typées avec thiserror (CoreError).
- Chaque fonction FFI est enveloppée dans std::panic::catch_unwind.
- Chaque bloc unsafe est court et précédé d'un commentaire // SAFETY: ...
- Chemins en PathBuf / OsString jusqu'à la sérialisation.
- Journalisation avec tracing, jamais println! dans le core.
- Fonctions publiques documentées (///), tests unitaires dans le même fichier.

## Conventions C#
- C# 12, nullable activé, MVVM strict : aucune logique métier dans les vues.
- Aucune couleur en dur : ressources de App.xaml (jetons de docs/design.md).
- Textes d'interface en français dans Strings/fr-FR/Resources.resw.
- Appels au moteur via IEngineService, en async, avec CancellationToken.

## Façon de travailler
- Commence chaque jalon par un plan court (fichiers, étapes, tests) et attends mon accord.
- Avance par petites étapes qui compilent ; un commit par étape (Conventional Commits,
  ex. "feat(scan): parcours des racines AppData (EF-05)").
- Je débute en Rust : quand tu utilises une notion nouvelle (ownership, emprunts,
  lifetimes, traits, Result et ?, Arc/AtomicBool, FFI), explique-la en 2 ou 3 phrases
  en français dans ton résumé, avec le fichier et la ligne où elle apparaît.
- Code, identifiants et commentaires en anglais ; échanges avec moi en français.
- Ne rajoute pas de dépendance hors de celles listées dans les spécifications sans me demander.
- Termine chaque session par : ce qui est fait, comment le tester, ce qui reste.
