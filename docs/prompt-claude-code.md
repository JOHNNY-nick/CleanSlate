# Prompt Claude Code — CleanSlate

29 oct. 2026

## 1. Mode d'emploi

Claude Code travaille mieux par petites étapes vérifiables : un jalon par session, toujours avec les documents du projet sous les yeux.

1. **Installe l'outillage** sur ton PC Windows : Rust (rustup), Visual Studio 2022 avec les charges « Développement .NET Desktop » et « Développement Desktop C++ », .NET 8 SDK, Git, puis Claude Code selon la [documentation officielle](https://docs.claude.com/en/docs/claude-code/overview).
2. **Décompresse ce dossier** : il sert de base au dépôt (`CLAUDE.md` à la racine, documents et maquettes dans `docs/`). Renomme-le `cleanslate` si tu veux, puis `git init`. Travaille sous Windows directement (pas dans WSL) : WinUI 3 et le linker MSVC doivent compiler côté Windows.
3. **Vérifie le `CLAUDE.md`** à la racine (section 2 ci-dessous). Claude Code le relit à chaque session : c'est là que vivent les règles permanentes.
4. **Lance Claude Code** dans le dossier et colle le prompt initial (section 3). Il réalise uniquement le jalon J1.
5. **À chaque jalon** : relis le plan qu'il propose avant de valider, laisse-le compiler et tester, lis le résumé, fais un commit, puis ouvre une nouvelle session avec le prompt du jalon suivant (section 4).

Règle d'or : ne lance jamais `cleanslate-cli` ni l'application sans le mode simulation sur ton PC principal avant le jalon J4 terminé. Pour les vrais essais de suppression, utilise Windows Sandbox ou une machine virtuelle.

## 2. Fichier CLAUDE.md

Le fichier complet est à la racine de ce dossier (`CLAUDE.md`). Il fixe le projet, les documents de référence, l'architecture, les commandes, les 8 règles de sécurité non négociables, les conventions Rust et C#, et la façon de travailler (plan d'abord, petits commits, explications Rust en français).

## 3. Prompt initial (jalon J1)

À coller dans la première session ; il fait lire les documents, puis construit uniquement le socle.

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
   (CleanSlate.App WinUI 3 packagé, CleanSlate.Interop, CleanSlate.Tests), cible x64.
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
   docs/design.md dans App.xaml, injection de dépendances, manifeste requireAdministrator.
   L'accueil affiche « Moteur CleanSlate 0.1.0 » obtenu via la DLL.
6. scripts/build.ps1 (cargo puis dotnet, copie de cleanslate.dll) et
   .github/workflows/ci.yml (fmt, clippy -D warnings, cargo test, build et test .NET).
7. Tests : aller-retour JSON des types du modèle, cs_version via FFI depuis xUnit,
   panique simulée convertie en erreur INTERNAL_PANIC.
8. README.md : prérequis, compilation, lancement.

Contraintes
- Respecte strictement CLAUDE.md, en particulier les règles de sécurité.
- Aucun scan, aucune lecture du registre réel, aucune suppression à ce jalon.
- Utilise les dernières versions stables des crates et du Windows App SDK ;
  vérifie-les plutôt que de les deviner.
- Si un outil manque sur ma machine (cargo, dotnet, MSVC), dis-le-moi et arrête-toi.

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
Jalon J2 — Inventaire (EF-01 à EF-04). Relis CLAUDE.md et les sections 3, 4, 5 et 8
des spécifications, puis propose un plan avant de coder.
- Module inventory : lecture en lecture seule des clés Uninstall HKLM 64 bits,
  HKLM WOW6432Node et HKCU via le trait Registry ; normalisation en InstalledApp ;
  id stable ; détection MSI ; filtrage SystemComponent, mises à jour, entrées sans nom.
- Tests sur une ruche factice (sous-clé HKCU\Software\CleanSlateTest\<uuid>) avec
  des cas réels : MSI, Inno Setup, NSIS, entrée 32 bits, InstallLocation vide.
- FFI cs_list_apps + EngineClient.ListAppsAsync.
- Page Applications conforme à la maquette : liste virtualisée, recherche,
  tri, case « composants système », ligne sélectionnée, barre d'action (bouton
  « Désinstaller complètement » visible mais désactivé jusqu'à J5).
- cleanslate-cli apps --json.
Condition de fin : inventaire < 2 s (ENF-05) mesuré sur ma machine, tests verts,
résumé avec explications Rust. Ne commence pas J3.
```

### J3 — Moteur de détection (simulation seulement)

```text
Jalon J3 — Détection des restes orphelins (EF-05 à EF-11), EN SIMULATION UNIQUEMENT :
aucun déplacement ni suppression à ce jalon. Relis les sections 6 et 9 des
spécifications et propose un plan.
- Module safety d'abord : liste noire complète de la section 9.1, canonisation des
  chemins, refus des reparse points, exclusions utilisateur ; un test par entrée de
  la liste noire + jonction piégée vers C:\Windows + nom contenant "..".
- Module scan : racines de EF-05/EF-06, profondeur 1 et 2, rayon + walkdir,
  AtomicBool d'annulation, progression toutes les 250 ms, erreurs collectées.
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
Jalon J4 — Quarantaine (EF-16 à EF-19). Premières opérations destructives :
sois très prudent. Relis la section 7.2, 7.3 et 9 des spécifications, plan d'abord.
- Module quarantine : manifeste écrit avant chaque déplacement, MoveFileExW
  même volume sinon copie vérifiée SHA-256, export .reg UTF-16 LE des clés,
  fichiers verrouillés -> MOVEFILE_DELAY_UNTIL_REBOOT, plafond 20 Go / 50 000 fichiers.
- Restauration totale et partielle, refus si la destination existe ; purge par âge.
- Module history : SQLite (rusqlite bundled), tables sessions, items, exclusions, settings.
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
  complète (exclusions, durée de quarantaine, thème, journaux).
- Thème sombre et Contraste élevé via ThemeDictionaries ; vérification
  clavier et Narrateur selon la section 6 du design.
- Tous les états et textes de la section 5 du design.
- Journaux tracing avec rotation 10 Mo (ENF-10).
- Installeur (MSIX ou Inno Setup) et README final.
- Revue finale : relis toutes les exigences M du cahier des charges et produis
  un tableau exigence -> fichier(s) -> test(s), en signalant ce qui manque.
```

## 5. Prompts utiles en cours de route

Ces prompts courts servent à apprendre Rust en chemin et à garder le moteur sûr ; utilise-les entre deux jalons.

| Besoin | Prompt |
| --- | --- |
| Comprendre un module | « Explique-moi core/cleanslate-core/src/scan.rs comme à un débutant en Rust : ownership, emprunts, Result et ?, puis pose-moi 3 questions pour vérifier que j'ai compris. » |
| S'exercer | « Donne-moi un petit exercice Rust lié à ce projet (ex. une fonction de normalisation de nom) avec ses tests ; laisse-moi l'écrire puis corrige-moi sans donner la solution d'emblée. » |
| Revue de sécurité | « Fais une revue de sécurité de safety.rs, quarantine.rs et uninstall.rs contre la section 9 des spécifications et CLAUDE.md. Liste chaque chemin de code destructif et le contrôle qui le protège. Ne modifie rien. » |
| Revue FFI | « Vérifie la frontière FFI : chaque ByteBuffer est-il libéré exactement une fois ? Chaque fonction a-t-elle catch_unwind ? Le callback de progression peut-il être appelé après la fin ? » |
| Faux positif constaté | « Le scan propose <chemin> alors que <application> est installée. Ajoute d'abord un test qui reproduit le cas, puis corrige matcher.rs sans baisser la détection des autres fixtures. » |
| Reprendre après une pause | « Lis CLAUDE.md, git log des 20 derniers commits et le jalon en cours dans docs/cahier-des-charges.md ; dis-moi où on en est et ce qui reste. » |
