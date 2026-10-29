# Prompts Claude Code — CleanSlate

Dernière mise à jour : 29 oct. 2026

## 1. Mode d'emploi

Claude Code travaille mieux par petites étapes vérifiables : un jalon par session, toujours avec les documents du projet sous les yeux.

1. **Installer l'outillage** sous Windows (pas dans WSL : WinUI 3 et le linker MSVC doivent compiler côté Windows) : Rust stable via rustup avec `rustfmt`, `clippy` et `cargo-llvm-cov` (composant `llvm-tools-preview`), le SDK .NET 8, PowerShell 7, Visual Studio 2022 (ou ses Build Tools) avec les charges « Développement Desktop C++ », « Développement .NET Desktop » et « Développement d'applications WinUI », Git, puis Claude Code selon la [documentation officielle](https://docs.claude.com/en/docs/claude-code/overview). Le [README](../README.md) donne les commandes d'installation.
2. **Cloner le dépôt** : `git clone <url-du-dépôt> cleanslate`, puis `cd cleanslate`.
3. **Vérifier l'installation** depuis la racine du dépôt : `pwsh scripts/build.ps1 -Configuration Debug` doit réussir, ainsi que les tests (section « Tester » du README).
4. **Relire `CLAUDE.md`** à la racine (section 2 ci-dessous). Claude Code le relit à chaque session : c'est là que vivent les règles permanentes.
5. **Lancer Claude Code** à la racine du dépôt et coller le prompt du prochain jalon (section 4). Le jalon J1 est déjà réalisé dans le dépôt ; son prompt (section 3) est conservé comme référence.
6. **À chaque jalon** : relire le plan proposé avant de le valider, laisser Claude Code compiler et tester, lire le résumé, faire un commit, puis ouvrir une nouvelle session avec le prompt du jalon suivant.

Règle d'or : ne jamais lancer `cleanslate-cli` ni l'application sans le mode simulation sur un PC principal avant la fin du jalon J4. Les vrais essais de suppression se font dans Windows Sandbox ou une machine virtuelle.

## 2. Fichier CLAUDE.md

Le fichier complet est à la racine du dépôt (`CLAUDE.md`). Il fixe le projet, les documents de référence, l'architecture, les commandes, les 8 règles de sécurité non négociables, les conventions Rust et C#, et la façon de travailler (plan d'abord, petits commits, explications Rust en français).

## 3. Prompt initial (jalon J1, réalisé)

Prompt de la première session, conservé comme référence : il fait lire les documents, puis construit uniquement le socle.

```text
Tu vas m'aider à développer CleanSlate, un utilitaire Windows en Rust + C# / WinUI 3.

Étape 0 — Contexte
Lis CLAUDE.md, puis en entier docs/cahier-des-charges.md, docs/specifications-techniques.md
et docs/design.md. Regarde aussi les maquettes HTML de docs/maquettes/.
Résume-moi en 10 lignes maximum ce que tu as compris de l'architecture et des règles
de sécurité, et liste les points qui te semblent ambigus ou contradictoires.

Étape 1 — Plan du jalon J1 (« Socle ») uniquement
Propose un plan avant d'écrire du code. J1 doit livrer :
1. Le monorepo décrit en section 2 des spécifications : workspace Cargo dans core/
   (cleanslate-core, cleanslate-ffi, cleanslate-cli) et solution app/CleanSlate.sln
   (CleanSlate.App WinUI 3 non packagée, CleanSlate.Interop, CleanSlate.Tests), cible x64.
2. Dans cleanslate-core : le module model avec les types de la section 5 des
   spécifications (serde, snake_case), le type CoreError, les traits FileSystem
   et Registry avec une implémentation réelle et une implémentation de test.
3. Dans cleanslate-ffi : cs_version, cs_engine_new, cs_engine_free, cs_buffer_free,
   le type ByteBuffer, l'enveloppe JSON { ok, data | error }, catch_unwind partout,
   et build.rs qui génère app/CleanSlate.Interop/NativeMethods.g.cs avec csbindgen.
4. Dans CleanSlate.Interop : EngineClient avec VersionAsync(), qui gère la mémoire
   correctement et transforme ok:false en EngineException.
5. Dans CleanSlate.App : fenêtre avec NavigationView (Accueil, Applications,
   Restes orphelins, Historique, Réglages), pages vides, jetons de couleur de
   docs/design.md dans App.xaml, injection de dépendances, application non packagée
   (WindowsPackageType=None) avec app.manifest requireAdministrator.
   L'accueil affiche « Moteur CleanSlate 0.1.0 » obtenu via la DLL.
6. scripts/build.ps1 (cargo puis dotnet, -Configuration appliquée aux deux, copie de
   cleanslate.dll) et .github/workflows/ci.yml (fmt, clippy -D warnings, cargo test,
   couverture cargo-llvm-cov ≥ 80 %, build et test .NET).
7. Tests : aller-retour JSON des types du modèle, cs_version via FFI depuis xUnit,
   panique simulée convertie en erreur INTERNAL_PANIC.
8. README.md : prérequis, compilation, lancement ; LICENSE (MIT).

Contraintes
- Respecte strictement CLAUDE.md, en particulier les règles de sécurité.
- Aucun scan, aucune lecture du registre réel, aucune suppression à ce jalon.
- Utilise les dernières versions stables des crates et du Windows App SDK ;
  vérifie-les plutôt que de les deviner.
- Si un outil manque sur ma machine (cargo, dotnet, pwsh, MSVC), dis-le-moi et arrête-toi.

Étape 2 — Réalisation
Après mon accord sur le plan, réalise J1 par petites étapes avec un commit chacune.
Termine quand scripts/build.ps1 réussit, que tous les tests passent et que
l'application affiche la version du moteur. Fais alors ton résumé avec les
explications Rust prévues dans CLAUDE.md. Ne commence pas J2.
```

## 4. Prompts des jalons J2 à J6

Un prompt par nouvelle session, après le commit du jalon précédent ; chacun rappelle le périmètre et la condition de fin.

### J2 — Inventaire des applications

```text
Jalon J2 — Inventaire (EF-01 à EF-03). Relis CLAUDE.md et les sections 3, 4, 5 et 8
des spécifications, puis propose un plan avant de coder.
- Module inventory : lecture en lecture seule des clés Uninstall HKLM 64 bits,
  HKLM WOW6432Node et HKCU via le trait Registry ; normalisation en InstalledApp ;
  id stable ; détection MSI ; filtrage SystemComponent, mises à jour, entrées sans nom.
- Tests sur une ruche factice (WinRegistry::sandboxed et TestRegistryKey, sous-clé
  HKCU\Software\CleanSlateTest\<uuid>) avec des cas réels : MSI, Inno Setup, NSIS,
  entrée 32 bits, InstallLocation vide.
- FFI cs_list_apps + EngineClient.ListAppsAsync.
- AppsViewModel dans CleanSlate.Presentation, testé avec NSubstitute.
- Page Applications conforme à la maquette : liste virtualisée, recherche,
  tri, case « composants système », ligne sélectionnée, barre d'action (bouton
  « Désinstaller complètement » visible mais désactivé jusqu'à J5).
- cleanslate-cli apps --json.
Condition de fin : inventaire < 2 s (ENF-05) mesuré sur ma machine, tests verts,
couverture du moteur ≥ 80 %, résumé avec explications Rust. Ne commence pas J3.
```

### J3 — Moteur de détection (simulation seulement)

```text
Jalon J3 — Détection des restes orphelins (EF-05 à EF-11), EN SIMULATION UNIQUEMENT :
aucun déplacement ni suppression à ce jalon. Relis les sections 6 et 9 des
spécifications et propose un plan.
- Module safety d'abord : liste noire complète de la section 9.1, y compris chaque
  racine de scan elle-même (EF-05, dont %LOCALAPPDATA%\Programs, et EF-06 : seuls
  leurs enfants sont candidats), canonisation des chemins, refus des reparse points,
  exclusions utilisateur ; un test par entrée de la liste noire et par racine de scan
  + jonction piégée vers C:\Windows + nom contenant "..".
- Module scan : racines de EF-05/EF-06, profondeur 1 et 2, rayon + walkdir,
  AtomicBool d'annulation, erreurs collectées ; progression : jamais plus d'un
  événement par 250 ms, et au moins un toutes les 250 ms tant que le travail avance.
- EF-07 (S) : raccourcis du menu Démarrer et du Bureau dont la cible n'existe plus,
  tâches planifiées et services dont l'exécutable a disparu (en lecture seule).
- Module matcher : normalisation, Jaro-Winkler (strsim), règles de rattachement
  de la section 6.2, score de la section 6.3 avec reasons lisibles, regroupement.
- Fixtures : arborescence factice reproduisant 5 applications désinstallées et
  5 installées (dont 2 du même éditeur) ; test : zéro élément d'une app installée
  dans les résultats, restes des désinstallées retrouvés.
- Benchmark criterion sur 500 000 fichiers factices (ENF-05).
- FFI cs_scan_orphans + cs_cancel ; EngineClient.ScanOrphansAsync avec IProgress.
- Page Restes orphelins conforme à la maquette, bouton d'action en mode
  « Simuler » seulement.
Condition de fin : tests verts, benchmark < 60 s, et un scan de ma machine en
lecture seule montré via la CLI avec --dry-run. Ne commence pas J4.
```

### J4 — Quarantaine et restauration

```text
Jalon J4 — Quarantaine (EF-16 à EF-19) et historique des sessions (EF-20).
Premières opérations destructives : sois très prudent. Relis les sections 5, 7.2,
7.3 et 9 des spécifications, plan d'abord.
- Module quarantine : manifeste écrit avant chaque déplacement, MoveFileExW
  même volume sinon copie vérifiée SHA-256, export .reg UTF-16 LE des clés,
  fichiers verrouillés -> MOVEFILE_DELAY_UNTIL_REBOOT, plafond 20 Go / 50 000 fichiers
  (au-delà, saisie du mot « QUARANTAINE »).
- Restauration totale et partielle, refus si la destination existe ; purge par âge.
- Module history : SQLite (rusqlite bundled), tables sessions, items, exclusions, settings.
- Finaliser QuarantinedItem (provisoire depuis J1) d'après le manifeste de la section 7.2.
- Tests d'intégration : cycle quarantaine -> restauration à l'identique (contenu,
  dates, clés) uniquement dans tempfile et HKCU\Software\CleanSlateTest ; test de
  reprise après interruption au milieu d'une session.
- FFI cs_clean, cs_restore, cs_list_sessions, cs_purge_quarantine.
- Dialogue de confirmation et page Historique conformes aux maquettes ;
  réglage « Mode simulation » (actif par défaut) dans Réglages.
Ne teste JAMAIS une vraie mise en quarantaine sur ma machine : donne-moi plutôt
la procédure pour l'essayer dans Windows Sandbox. Ne commence pas J5.
```

### J5 — Désinstallation complète

```text
Jalon J5 — Désinstallation complète (EF-12 à EF-15). Relis les sections 6.4 et 7.1
des spécifications et la section 4.2 du design, plan d'abord.
- Module uninstall : empreinte de l'application, point de restauration
  (SRSetRestorePointW), découpage sûr de UninstallString (tests sur 15 formes
  réelles : guillemets, arguments, MsiExec /I{GUID} à transformer en /X, chemins
  avec espaces), lancement dans un Job Object, attente de tout l'arbre, délai 30 min,
  vérification de la disparition de l'entrée Uninstall.
- cs_uninstall et cs_scan_app_leftovers ; EngineClient correspondant.
- Assistant en 4 étapes conforme à la maquette ; activer le bouton de la page
  Applications.
- Tests unitaires avec un faux processus ; aucune vraie désinstallation sur ma machine.
Condition de fin : procédure de test dans Windows Sandbox avec 7-Zip, VLC et
Notepad++ écrite dans docs/tests-acceptation.md. Ne commence pas J6.
```

### J6 — Finition

```text
Jalon J6 — Finition. Plan d'abord.
- Accueil complet (chiffres, sessions récentes, bandeau simulation), page Réglages
  complète (exclusions EF-20, durée de quarantaine, thème, journaux).
- Validation à l'écran des valeurs sombres (en place depuis J1) et ressources
  HighContrast dans les ThemeDictionaries ; vérification clavier et Narrateur selon
  la section 6 du design (ENF-08).
- Couleur d'accent de la NavigationView : encore le bleu Windows au J1, à remplacer
  par la sarcelle #0F766E (AccentBrush) en redéfinissant les ressources d'accent
  système dans App.xaml.
- Tous les états et textes de la section 5 du design.
- Journaux tracing dans %LOCALAPPDATA%\CleanSlate\logs, rotation quotidienne,
  7 derniers fichiers conservés (ENF-10).
- Réévaluer la migration de Windows App SDK 1.8 vers la ligne 2.x (risques,
  changements cassants, compatibilité .NET 8) et me proposer une décision.
- Installeur Inno Setup (application non packagée, requireAdministrator, section 10.4
  des spécifications) et README final.
- Revue finale : relis toutes les exigences M du cahier des charges et produis
  un tableau exigence -> fichier(s) -> test(s), en signalant ce qui manque.
```

## 5. Prompts utiles en cours de route

Ces prompts courts servent à apprendre Rust en chemin et à garder le moteur sûr, entre deux jalons.

| Besoin | Prompt |
| --- | --- |
| Comprendre un module | « Explique-moi core/cleanslate-core/src/scan.rs comme à un débutant en Rust : ownership, emprunts, Result et ?, puis pose-moi 3 questions pour vérifier que j'ai compris. » |
| S'exercer | « Donne-moi un petit exercice Rust lié à ce projet (ex. une fonction de normalisation de nom) avec ses tests ; laisse-moi l'écrire puis corrige-moi sans donner la solution d'emblée. » |
| Revue de sécurité | « Fais une revue de sécurité de safety.rs, quarantine.rs et uninstall.rs contre la section 9 des spécifications et CLAUDE.md. Liste chaque chemin de code destructif et le contrôle qui le protège. Ne modifie rien. » |
| Revue FFI | « Vérifie la frontière FFI : chaque ByteBuffer est-il libéré exactement une fois ? Chaque fonction a-t-elle catch_unwind ? Le callback de progression peut-il être appelé après la fin ? » |
| Faux positif constaté | « Le scan propose <chemin> alors que <application> est installée. Ajoute d'abord un test qui reproduit le cas, puis corrige matcher.rs sans baisser la détection des autres fixtures. » |
| Reprendre après une pause | « Lis CLAUDE.md, git log des 20 derniers commits et le jalon en cours dans docs/cahier-des-charges.md ; dis-moi où on en est et ce qui reste. » |
