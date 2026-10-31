# CleanSlate — consignes pour Claude Code

Dernière mise à jour : 30 oct. 2026

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
  (engine, error, model, fs, registry, native, system, inventory, scan, matcher,
  safety, quarantine, history, uninstall, logging, shortcut, tasks, services, store, msi) ;
  les accès réels au système (Planificateur, services, paquets du Store, Windows Installer)
  passent par des traits injectables qui refusent tout sur un moteur de test
- core/cleanslate-ffi : cdylib, [lib] name = "cleanslate" -> cleanslate.dll,
  API C extern "C", JSON UTF-8, ByteBuffer libéré par cs_buffer_free
- core/cleanslate-cli : binaire de debug
- app/CleanSlate.App : WinUI 3 non packagée (WindowsPackageType=None), .NET 8,
  app.manifest requireAdministrator, exécutable CleanSlate.App.exe ; vues, MainWindow,
  implémentations Windows des services (EngineService) ; installeur Inno Setup en J6
- app/CleanSlate.Presentation : net8.0 sans WinUI ; ViewModels (CommunityToolkit.Mvvm)
  et interfaces de services (IEngineService, INavigationService, IDialogService,
  ISettingsService) ; référencé par CleanSlate.App et CleanSlate.Tests
- app/CleanSlate.Interop : net8.0 ; NativeMethods.g.cs (généré par csbindgen, versionné
  dans Git, ne jamais l'éditer) + EngineClient, seul code C# qui touche aux pointeurs
- app/CleanSlate.Tests : xUnit v3 + NSubstitute

## Commandes
Toutes les commandes se lancent depuis la racine du dépôt.
- Compiler tout : pwsh scripts/build.ps1 -Configuration Debug
  (-Configuration s'applique à cargo et à dotnet : Debug = profil dev, Release = --release)
- Tests Rust : cargo test --manifest-path core/Cargo.toml --workspace
- Lint Rust : cargo fmt --manifest-path core/Cargo.toml --all --check ;
  cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets -- -D warnings
- Tests C# : dotnet test app/CleanSlate.sln -p:Platform=x64
- Couverture Rust : cargo llvm-cov --manifest-path core/Cargo.toml --workspace
  (outil de dev, objectif ≥ 80 %, ENF-12)
- CLI : sur cette machine, Claude ne lance que
  cargo run --manifest-path core/Cargo.toml -p cleanslate-cli -- scan --dry-run --json
  (toute autre commande de la CLI est bloquée par le hook .claude/hooks/guard-cleanslate-cli.ps1, D53).
  Les commandes de test destructives n'existent que dans un binaire compilé avec la feature
  `sandbox-tests` (par scripts/sandbox/prepare.ps1 pour le Sandbox, et par
  scripts/tests/check-release-cli.tests.ps1 dans core/target/feature-check, jamais exécuté ni livré) ;
  jamais dans build.ps1, le binaire publié ni l'installeur (contrôle : scripts/check-release-cli.ps1,
  codes de sortie 0 propre, 1 commandes trouvées, 2 erreur)
Avant de dire qu'une tâche est finie : fmt, clippy et tous les tests passent.

## Règles de sécurité (non négociables)
1. Ne jamais exécuter d'opération destructive sur le vrai système de cette machine.
   Pas de suppression, déplacement ou écriture de registre hors des zones de test,
   et jamais cleanslate-cli sans --dry-run ni l'application hors mode simulation,
   sauf si je le demande explicitement dans le message en cours.
   Incident du 30 oct. 2026 (D53), à ne jamais reproduire : Claude a lancé
   « cleanslate-cli sandbox quarantine » sur cette machine pour « vérifier la garde » ; l'outil
   l'a bloqué avant exécution. Une garde se teste par une fonction pure, jamais en lançant
   l'action réelle ; et aucune commande de test destructive ne s'exécute hors de Windows Sandbox.
2. Tests : uniquement des dossiers temporaires (tempfile) et la clé
   HKCU\Software\CleanSlateTest\<uuid>, créée puis supprimée par le test (TestRegistryKey).
   Le moteur reçoit son disque et son registre par injection : Engine::new(config, fs, registry)
   avec les traits FileSystem et Registry (FakeFileSystem, WinRegistry::sandboxed).
3. Toute action destructive appelle safety::validate juste avant d'agir.
4. Ne jamais affaiblir la liste noire de safety.rs. Toute modification de ce fichier
   doit être ajoutée avec des tests et signalée explicitement dans ton résumé.
   Seules dérogations, décidées par le porteur : les exceptions étroites D20 (menu Démarrer),
   D47 (tâches, services) et D48 (données de paquets du Store), chacune dans une fonction
   dédiée de safety.rs ; les règles générales (check_path, validate_key) restent inchangées.
5. dry_run vaut true par défaut dans tous les types et toutes les API.
6. Ne jamais suivre de lien symbolique ni de point de jonction (reparse point).
7. Jamais de suppression directe : tout passe par quarantine avec manifeste écrit
   avant le déplacement.
8. UninstallString : découpage exécutable + arguments, jamais via cmd /c ni un shell.

## Conventions Rust
- Édition 2021, toolchain stable, cible x86_64-pc-windows-msvc.
- Pas de unwrap() ni expect() hors tests (lints clippy du workspace) ; erreurs typées
  avec thiserror (CoreError).
- Chaque fonction FFI est enveloppée dans std::panic::catch_unwind.
- Chaque bloc unsafe est court et précédé d'un commentaire // SAFETY: ...
- Chemins en PathBuf / OsString jusqu'à la sérialisation.
- Journalisation avec tracing, jamais println! dans le core ; fichiers dans
  %LOCALAPPDATA%\CleanSlate\logs, rotation quotidienne (tracing-appender),
  7 derniers fichiers conservés (ENF-10).
- Fonctions publiques documentées (///), tests unitaires dans le même fichier.

## Conventions C#
- C# 12, nullable activé, MVVM strict : aucune logique métier dans les vues ;
  ViewModels et interfaces de services dans CleanSlate.Presentation, jamais dans CleanSlate.App.
- Aucune assembly .NET ne doit s'appeler CleanSlate.dll : elle écraserait la DLL native
  cleanslate.dll, Windows ne distinguant pas la casse (d'où CleanSlate.App.exe).
- Aucune couleur en dur : ressources de App.xaml (jetons de docs/design.md).
- Textes d'interface en français dans Strings/fr-FR/Resources.resw.
- Appels au moteur via IEngineService, en async, avec CancellationToken ; les ViewModels
  n'appellent jamais EngineClient directement.
- Tests de ViewModels avec NSubstitute (IEngineService simulé).

## Session du livre (même dossier, même branche main)
Une autre session de Claude Code rédige le livre ici, sur main, uniquement dans book/ et
docs/prompts/livre.md.
- Ne jamais modifier book/ ni docs/prompts/livre.md.
- Lire book/constats-pour-main.md au début de chaque session et avant chaque commit ;
  une faille ou un bug signalé se traite comme une correction normale, avec un test.
- Commits : uniquement ses propres fichiers, nommés un par un (git add <fichier>…).
  Jamais git add -A, git add ., git commit -a, ni git stash, reset, checkout ou rebase,
  qui toucheraient au livre en cours. Ignorer book/ dans git status.
- Si Git signale index.lock (l'autre session commite), attendre puis réessayer ;
  ne jamais supprimer le fichier de verrou.

## Façon de travailler
- Au début de chaque session, lis docs/feuille-de-route.md et annonce la prochaine action.
  Claude pilote le projet selon les « Règles de pilotage » de ce fichier : à la fin de chaque
  réponse, la prochaine action (qui, commande exacte, dossier, durée, réponse attendue) ;
  mettre à jour « Où en est-on ? » dès qu'une étape avance.
- Commence chaque jalon par un plan court (fichiers, étapes, tests) et attends mon accord.
- Avance par petites étapes qui compilent ; un commit par étape (Conventional Commits,
  ex. "feat(scan): parcours des racines AppData (EF-05)").
- Je débute en Rust : quand tu utilises une notion nouvelle (ownership, emprunts,
  lifetimes, traits, Result et ?, Arc/AtomicBool, FFI), explique-la en 2 ou 3 phrases
  en français dans ton résumé, avec le fichier et la ligne où elle apparaît.
- Code, identifiants et commentaires en anglais ; échanges avec moi en français.
- Ne rajoute pas de dépendance hors de celles listées dans les spécifications (crates § 3.1,
  paquets .NET § 8.3) sans me demander.
- Termine chaque session par : ce qui est fait, comment le tester, ce qui reste.
