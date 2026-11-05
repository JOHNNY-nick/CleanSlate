# Cahier des charges — CleanSlate

Dernière mise à jour : 29 oct. 2026

## 1. Présentation du projet

CleanSlate est un utilitaire Windows qui retrouve les fichiers laissés par des applications désinstallées et désinstalle complètement une application, restes compris. Le nom est un nom de code, modifiable.

### 1.1 Contexte

Le désinstalleur lancé par Paramètres > Applications supprime les fichiers principaux d'un programme mais laisse souvent des restes :

- dossiers de données dans `%APPDATA%`, `%LOCALAPPDATA%`, `%PROGRAMDATA%` ;
- dossiers orphelins dans `Program Files` et `Program Files (x86)` ;
- clés de registre sous `HKCU\Software` et `HKLM\SOFTWARE` ;
- raccourcis, tâches planifiées, services, fichiers temporaires.

Les outils existants (Revo Uninstaller, BCUninstaller) couvrent ce besoin en partie ; la recherche de restes d'applications déjà désinstallées, sans en connaître le nom, est souvent payante ou limitée à Program Files.

### 1.2 Objectifs

1. **Tout supprimer en une seule désinstallation** : l'application et tous les fichiers qu'elle laisse derrière elle (désinstalleur officiel, puis recherche et suppression de ses restes).
2. **Retrouver les restes d'applications déjà désinstallées** et les supprimer, sans avoir besoin de connaître leur nom (dossiers et clés qui n'appartiennent plus à aucune application installée).
3. **Ne jamais casser le système** : tout élément supprimé est sauvegardé et restaurable ; aucun fichier système ou partagé n'est proposé à la suppression par défaut.
4. **Objectif personnel** : servir de projet d'apprentissage de Rust, avec un moteur Rust propre et testé.

## 2. Périmètre

La version 1.0 cible un poste Windows personnel, un utilisateur averti mais non expert, et les applications Win32 classiques.

| Inclus en v1.0 | Exclu de la v1.0 |
| --- | --- |
| Applications Win32 (MSI, EXE, installeurs Inno Setup, NSIS, InstallShield) et applications Microsoft Store de l'utilisateur courant (EF-04) | Applications du Store d'autres utilisateurs ou pour tous les utilisateurs ; applications intégrées à Windows (signées `System`) |
| Inventaire des applications installées (registre Uninstall) | Pilotes, mises à jour Windows, composants système |
| Scan des restes orphelins : dossiers, registre, raccourcis (Bureaux et menu Démarrer), tâches planifiées, services, données de paquets du Store | Nettoyage généraliste du registre (« registry cleaner ») |
| Désinstallation complète : désinstalleur officiel puis restes | Surveillance en temps réel des installations |
| Quarantaine et restauration des éléments supprimés | Gestion de plusieurs postes ou déploiement en entreprise |
| Interface en français, prête pour l'anglais | Version portable sans installation (à envisager en v1.1) |

**Utilisateurs cibles** : le porteur du projet d'abord, puis des particuliers à l'aise avec Windows qui veulent garder un PC propre.

**Environnement** : Windows 10 22H2 et Windows 11 (23H2 et suivants), architecture x64 ; ARM64 en option. Droits administrateur requis pour la désinstallation et pour les zones `HKLM` et `Program Files`.

## 3. Exigences fonctionnelles

Trente-cinq exigences : vingt et une en cinq modules (EF-01 à EF-21), puis quatorze pour le Gestionnaire (EF-22 à EF-35) ; la priorité suit la méthode MoSCoW (M = indispensable, S = souhaitable, C = optionnel).

### 3.1 Inventaire des applications installées

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-01 | Lister les applications installées depuis les trois clés Uninstall (HKLM 64 bits, HKLM WOW6432Node, HKCU) avec nom, éditeur, version, date d'installation, taille, dossier d'installation et commande de désinstallation. | M |
| EF-02 | Masquer par défaut les composants système (`SystemComponent = 1`), les mises à jour et les entrées sans nom ; un filtre permet de les afficher. | M |
| EF-03 | Rechercher par nom ou éditeur, trier par nom, taille ou date. | M |
| EF-04 | Lister les applications Microsoft Store de l'utilisateur courant (hors frameworks, ressources et applications système) avec un badge « Store » et un filtre « Win32 / Store / Toutes » ; les désinstaller pour l'utilisateur courant seulement, puis proposer leurs restes (`%LOCALAPPDATA%\Packages\<famille>` et ses clés). | S |

### 3.2 Recherche des restes orphelins

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-05 | Lancer un scan complet sans saisir de nom : dossiers de `Program Files`, `Program Files (x86)`, `%APPDATA%`, `%LOCALAPPDATA%`, `%LOCALAPPDATA%\Programs`, `%PROGRAMDATA%` ; `Documents` seulement pour les restes d'une application désinstallée, jamais dans l'analyse générale, qui proposait des dossiers personnels (D68). | M |
| EF-06 | Scanner le registre : `HKCU\Software`, `HKLM\SOFTWARE`, `HKLM\SOFTWARE\WOW6432Node`. | M |
| EF-07 | Scanner les raccourcis du menu Démarrer et du Bureau dont la cible locale n'existe plus, les tâches planifiées et services dont l'exécutable a disparu ; les mettre en quarantaine de façon restaurable (raccourci déplacé, tâche exportée en XML, service sauvegardé en JSON et `.reg`). | S |
| EF-08 | Considérer un élément comme orphelin s'il ne correspond à aucune application installée, en comparant noms, éditeurs, chemins d'installation et exécutables. | M |
| EF-09 | Regrouper les restes par application déduite (ex. : « Probablement : Audacity »), avec taille totale et date de dernière modification. | M |
| EF-10 | Attribuer à chaque élément un niveau de confiance (Élevée, Moyenne, Faible) ; seuls les éléments à confiance élevée sont précochés. | M |
| EF-11 | Afficher la progression du scan et permettre son annulation. | M |

### 3.3 Désinstallation complète

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-12 | Sélectionner une application installée et lancer son désinstalleur officiel (`UninstallString`, ou `msiexec /x` pour un MSI), en attendant la fin du processus. Une application importante (antivirus, pilote, composant dont d'autres programmes ont besoin, application Microsoft) est désinstallée après un encadré qui explique le risque ; un composant de Windows ou une mise à jour ne l'est jamais (D65, D67). | M |
| EF-13 | Après la désinstallation, scanner automatiquement les restes de cette application et les afficher pour validation. | M |
| EF-14 | Proposer un mode silencieux (`QuietUninstallString`, ou `/qn` pour un MSI) quand il existe. Pour Inno Setup : `/VERYSILENT /NORESTART` à la place des options déclarées, jamais `/SUPPRESSMSGBOXES` (une question du désinstalleur reste affichée et l'utilisateur y répond ; les données qu'il garderait sont ensuite proposées comme restes, restaurables) (D60). | S |
| EF-15 | Créer un point de restauration Windows avant la désinstallation, si la protection système est active. | S |
| EF-21 | Afficher la progression réelle de la désinstallation : pourcentage et action en cours pour un MSI et une application du Store ; pour un désinstalleur `.exe`, étapes horodatées, temps écoulé et processus actifs, sans pourcentage inventé. | S |

### 3.4 Suppression et sécurité

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-16 | Afficher un récapitulatif avant toute suppression : nombre d'éléments, taille, chemins ; confirmation explicite obligatoire. | M |
| EF-17 | Déplacer les fichiers et exporter les clés de registre vers une quarantaine avant suppression ; jamais de suppression définitive directe. Option « Supprimer définitivement, sans quarantaine », décochée par défaut : la session tout juste mise en quarantaine est purgée aussitôt, rien n'est restaurable (D66). | M |
| EF-18 | Restaurer une session de nettoyage, en totalité ou élément par élément, depuis l'historique. | M |
| EF-19 | Purger la quarantaine automatiquement après 30 jours (durée réglable) ou manuellement. Au premier lancement, l'utilisateur choisit ce délai (7, 14, 30 ou 90 jours, 30 recommandé), l'emplacement de la quarantaine et le dossier d'export des journaux, après un accueil qui explique CleanSlate et une acceptation ; tout se change ensuite dans Réglages (D62, D63). | S |

### 3.5 Historique et réglages

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-20 | Tenir un historique des sessions (date, application, éléments, taille libérée) et permettre une liste d'exclusions personnalisée (dossiers ou clés à ne jamais proposer). | M |

## 4. Exigences non fonctionnelles

La sécurité prime sur tout le reste : un faux positif supprimé est bien plus grave qu'un reste oublié.

| ID | Domaine | Exigence | Mesure de vérification |
| --- | --- | --- | --- |
| ENF-01 | Sécurité | Zones sensibles (D79, remplace la liste noire bloquante) : `C:\Windows`, Program Files, ProgramData, dossiers utilisateur racines, `Microsoft`, `Windows`, `Common Files`, pilotes, clés `HKLM\SYSTEM`, `SAM`, `SECURITY`, `HARDWARE`, `HKLM\SOFTWARE\Microsoft`, `\Classes`, `Policies`. Jamais proposées ni précochées automatiquement ; action manuelle seulement en mode expert avec « CONFIRMER ». Toujours refusés : chemins malformés, liens et jonctions, racines de lecteur. | Tests de zones sensibles sur chaque chemin |
| ENF-02 | Sécurité | Chaque action a une confirmation explicite et un mode « Sûr » (Corbeille ou quarantaine, restaurable, par défaut) ou « Immédiat » (définitif) ; simulation par défaut dans toutes les API (D79). | Tests d'intégration |
| ENF-03 | Sécurité | Ne jamais suivre les liens symboliques ni les points de jonction pendant le scan et la suppression. | Test avec jonction piégée vers `C:\Windows` |
| ENF-04 | Sécurité | Mode « simulation » (dry run) qui produit la liste sans rien toucher : par défaut dans le moteur, la FFI et la CLI ; l'application n'a plus de mode simulation, mais garde l'aperçu « Simuler » des restes (D66). | Test : aucun fichier modifié |
| ENF-05 | Performance | Inventaire des applications en moins de 2 s ; scan complet des restes en moins de 60 s sur SSD avec 500 000 fichiers. | Benchmark Criterion + chronométrage UI |
| ENF-06 | Performance | Interface toujours réactive : tout travail long hors du thread UI. Le moteur n'envoie jamais plus d'un événement de progression par 250 ms, et en envoie au moins un toutes les 250 ms tant que le travail avance. | Revue de code + test manuel |
| ENF-07 | Ergonomie | Une désinstallation complète en 5 clics maximum depuis l'écran Applications, confirmation comprise. | Test utilisateur |
| ENF-08 | Ergonomie | Thèmes clair et sombre suivant Windows ; accessibilité clavier et lecteur d'écran (Narrateur). | Accessibility Insights |
| ENF-09 | Fiabilité | Une erreur sur un élément (fichier verrouillé, accès refusé) n'interrompt pas la session ; elle est listée dans le rapport final. | Tests d'intégration |
| ENF-10 | Traçabilité | Journal local de chaque opération (scan, déplacement, restauration) dans `%LOCALAPPDATA%\CleanSlate\logs`, rotation quotidienne, conservation des 7 derniers fichiers. | Inspection des journaux |
| ENF-11 | Confidentialité | Aucune donnée envoyée sur Internet ; aucune télémétrie. | Revue des dépendances |
| ENF-12 | Maintenabilité | Couverture de tests du moteur Rust ≥ 80 % ; `cargo clippy` sans avertissement ; code C# en MVVM. | CI GitHub Actions, couverture mesurée avec `cargo-llvm-cov` |

## 5. Contraintes techniques

L'architecture est hybride : un moteur en Rust, une interface en C# / WinUI 3, reliés par une DLL native.

- **Moteur** : Rust édition 2021 (stable), compilé en `cdylib` (`cleanslate.dll`), avec une API C stable (`extern "C"`).
- **Interface** : C# 12, .NET 8, WinUI 3 (Windows App SDK) en application non packagée (`WindowsPackageType=None`), patron MVVM avec CommunityToolkit.Mvvm.
- **Liaison** : P/Invoke depuis C#, liaisons générées par `csbindgen` ; échanges de données structurées en JSON UTF-8.
- **Accès Windows côté Rust** : crates `windows` (API Win32 officielles de Microsoft) et `winreg` pour le registre.
- **Privilèges** : application lancée en administrateur (manifeste `requireAdministrator`).
- **Développement** : Claude Code comme assistant principal ; dépôt Git unique (monorepo) ; GitHub Actions pour la CI.
- **Distribution** : installeur Inno Setup en v1.0 (une application packagée ne peut pas demander `requireAdministrator`) ; signature de code à prévoir avant diffusion publique.
- **Licence** : MIT (fichier `LICENSE` à la racine du dépôt).
- **Versions retenues** (au J1) : Rust stable du moment (1.99.1), .NET 8, Windows App SDK 1.8.x, Microsoft.Extensions.Hosting 8.x, xUnit v3 ; détail dans les spécifications techniques (§ 2 et § 8.3).

## 6. Livrables, jalons et acceptation

Le projet avance en six jalons séquentiels ; chacun se termine par une version qui compile, passe les tests et se démontre.

### 6.1 Jalons

1. **J1 — Socle** (terminé) : monorepo, workspace Cargo, solution WinUI 3, `cleanslate.dll` appelée depuis C# (fonction `cs_version`), CI.
2. **J2 — Inventaire** : EF-01 à EF-03 ; écran « Applications » fonctionnel.
3. **J3 — Moteur de détection** : EF-05 à EF-11 en mode simulation uniquement ; écran « Restes orphelins ».
4. **J4 — Quarantaine** : EF-16 à EF-19 et historique des sessions (EF-20) ; premières suppressions réelles, toujours restaurables.
5. **J5 — Désinstallation complète** : EF-12 à EF-15 ; parcours complet de bout en bout.
6. **J6 — Finition** : accueil complet, exclusions (EF-20), réglages, thèmes, accessibilité, journaux, installeur Inno Setup.

Traçabilité des exigences vers les jalons :

| Jalon | Exigences fonctionnelles | Exigences non fonctionnelles |
| --- | --- | --- |
| J1 | — | ENF-03 (accès disque sans traverser les reparse points), ENF-12 (CI, couverture) |
| J2 | EF-01, EF-02, EF-03 | ENF-05 (inventaire < 2 s) |
| J3 | EF-05 à EF-11 | ENF-01, ENF-03, ENF-04, ENF-05 (scan < 60 s), ENF-06, ENF-09 |
| J4 | EF-16 à EF-19, EF-20 (historique) | ENF-02, ENF-04, ENF-09 |
| J5 | EF-12 à EF-15 | ENF-07 |
| J6 | EF-20 (exclusions dans les réglages) | ENF-08, ENF-10, ENF-11 (revue finale) |

Compléments de la v1.0 (30 oct. 2026) : EF-04 (remontée en S), EF-07 complète et EF-21, voir `docs/journal/v1.0-complements-plan.md`.

### 6.2 Livrables

- Code source du moteur Rust et de l'interface, avec tests.
- Exécutable et installeur Windows x64.
- Documentation : ce cahier des charges, les spécifications techniques, le dossier de design, le `README.md` et le `CLAUDE.md` du dépôt.

### 6.3 Critères d'acceptation

- [ ] Toutes les exigences de priorité M sont satisfaites.
- [ ] Sur une machine de test, après installation puis désinstallation classique de 5 applications connues (ex. : VLC, 7-Zip, Notepad++, Audacity, GIMP), le scan retrouve au moins 90 % de leurs restes.
- [ ] Zéro élément d'une application encore installée ou du système n'est précoché sur ce même jeu de test.
- [ ] Toute session de nettoyage est restaurable à l'identique (fichiers et registre).
- [ ] Les tests automatisés passent en CI ; aucune alerte `clippy`.

### 6.4 Risques

| Risque | Impact | Parade |
| --- | --- | --- |
| Faux positif : suppression de données d'une application installée | Critique | Liste noire, niveau de confiance, précochage limité à « Élevée », quarantaine systématique |
| Dossiers partagés par plusieurs logiciels d'un même éditeur | Élevé | Un dossier éditeur n'est orphelin que si aucune application de cet éditeur n'est installée |
| Désinstalleurs qui relancent un processus enfant et rendent la main trop tôt | Moyen | Suivre l'arbre de processus (Job Object) et attendre leur fin |
| Complexité de la frontière Rust / C# | Moyen | API C minimale en JSON, tests de la frontière dès J1 |
| Courbe d'apprentissage de Rust | Moyen | Jalons courts, moteur découpé en modules simples, tests unitaires fournis par Claude Code |
| Windows App SDK 1.8 dépassé par la ligne 2.x, déjà disponible | Faible | Rester en 1.8 jusqu'à J6, puis évaluer la migration vers la 2.x en J6 |

### 6.5 Évolutions envisagées

- **v1.1** : version portable sans installation.
- **J6** : évaluation de la ligne Windows App SDK 2.x.

## Gestionnaire du stockage (D79, 4 nov. 2026)

Détail et étapes : [`docs/journal/gestionnaire-plan.md`](journal/gestionnaire-plan.md).

| ID | Exigence |
| --- | --- |
| EF-22 | Zones sensibles : jamais proposées ni précochées automatiquement ; action manuelle seulement en mode expert avec « CONFIRMER ». |
| EF-23 | Modes « Sûr » (Corbeille ou quarantaine) et « Immédiat » (définitif) dans chaque confirmation ; « Sûr » par défaut ; aperçu : nombre, taille, zones sensibles, mode, gain libéré maintenant. |
| EF-24 | Quarantaine sur un autre lecteur, choisi dans Réglages, avec vérification de l'espace libre ; la confirmation dit « libéré maintenant » ou « libéré après purge ». |
| EF-25 | Caches régénérables (temporaires, navigateurs, miniatures, rapports d'erreurs et vidages, caches de paquets de développement) supprimés directement, jamais un fichier ouvert ; navigateur ouvert : proposition de le fermer. |
| EF-26 | Nettoyage système par les mécanismes officiels : DISM `StartComponentCleanup`, caches de Windows Update et de l'Optimisation de la distribution (arrêt et redémarrage propres des services), Windows.old, hibernation (`powercfg`), points de restauration (API système ; on garde le plus récent et ceux de moins de 30 jours, sauf mode expert). |
| EF-27 | Fichiers personnels : accès complet, jamais précochés ; Corbeille, déplacement vers un autre lecteur (espace vérifié, copie, comparaison SHA-256, puis suppression de l'original), suppression définitive, suppression sécurisée. Jamais les deux copies d'un doublon. |
| EF-28 | Fonctions de suppression : Corbeille, définitive, « supprimer et vider la Corbeille » ; vider les Corbeilles (taille par lecteur) ; purger la quarantaine (tout, session, élément) ; dossiers vides ; suppression au redémarrage (liste, annulation) ; suppression en masse par filtre avec aperçu ; suppression sécurisée (avertissement SSD, BitLocker recommandé). |
| EF-29 | Diagnostic : une phrase par lecteur, les 5 plus gros consommateurs expliqués, récupérable par catégorie (« sans risque » / « à vérifier »), « Libérer » par catégorie et « Tout libérer maintenant (sans risque) ». |
| EF-30 | Carte (la page Stockage), Explorateur trié par taille (taille, %, fichiers, date, recherche, sélection multiple, badge « zone sensible »), Catégories, Gros fichiers, Fichiers anciens, Doublons (taille puis SHA-256 via BCrypt, original proposé, actions sur les copies). |
| EF-31 | Système : hibernation, fichier d'échange, points de restauration, magasin de composants, Windows.old, caches de mise à jour, stockage réservé : taille, explication, conséquence, bouton qui l'exécute avec progression. |
| EF-32 | Suppressions en attente : redémarrage, Corbeilles, quarantaine, avec taille et bouton pour libérer. |
| EF-33 | Évolution : mesures enregistrées (SQLite) à chaque ouverture et analyse ; courbe de l'espace libre par lecteur, « ce qui a le plus grossi ». |
| EF-34 | Volumes : lecteurs et partitions (taille, utilisé, libre, système de fichiers, non alloué), affichage seul. |
| EF-35 | Partout : ouvrir dans l'Explorateur, propriétés, copier le chemin, désinstaller via l'assistant ; après chaque action, « X Go libérés maintenant » mesuré, entrée dans l'Historique, « Annuler » quand c'est possible ; export d'un rapport HTML et CSV sur le Bureau. |
| ENF-13 | Analyses en lecture seule, parallèles, avec progression et annulation, sans suivre liens ni jonctions, accès refusés ignorés proprement ; analyse des restes < 60 s ; explorateur, doublons et nettoyage système en tâches séparées. |
| ENF-14 | Opérations système par API ou commandes officielles, code de retour vérifié, journalisation complète, message clair en cas d'échec ou de redémarrage nécessaire. |
| ENF-15 | Aucune donnée ne quitte la machine ; textes dans les `.resw` ; thèmes Clair, Sombre, Multicolore ; clavier et Narrateur. |
