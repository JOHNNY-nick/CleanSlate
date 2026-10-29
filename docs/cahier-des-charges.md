# Cahier des charges — CleanSlate

29 oct. 2026

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

1. **Détecter automatiquement les restes orphelins** : repérer les dossiers et clés qui n'appartiennent plus à aucune application installée, sans que l'utilisateur fournisse de nom.
2. **Désinstaller complètement** : lancer le désinstalleur officiel d'une application installée, puis trouver et supprimer tous ses restes.
3. **Ne jamais casser le système** : tout élément supprimé est sauvegardé et restaurable ; aucun fichier système ou partagé n'est proposé à la suppression par défaut.
4. **Objectif personnel** : servir de projet d'apprentissage de Rust, avec un moteur Rust propre et testé.

## 2. Périmètre

La version 1.0 cible un poste Windows personnel, un utilisateur averti mais non expert, et les applications Win32 classiques.

| Inclus en v1.0 | Exclu de la v1.0 |
| --- | --- |
| Applications Win32 (MSI, EXE, installeurs Inno Setup, NSIS, InstallShield) | Applications Microsoft Store (AppX/MSIX) : listées en lecture seule seulement |
| Inventaire des applications installées (registre Uninstall) | Pilotes, mises à jour Windows, composants système |
| Scan des restes orphelins : dossiers, registre, raccourcis, tâches planifiées | Nettoyage généraliste du registre (« registry cleaner ») |
| Désinstallation complète : désinstalleur officiel puis restes | Surveillance en temps réel des installations |
| Quarantaine et restauration des éléments supprimés | Gestion de plusieurs postes ou déploiement en entreprise |
| Interface en français, prête pour l'anglais | Version portable sans installation (à envisager en v1.1) |

**Utilisateurs cibles** : le porteur du projet d'abord, puis des particuliers à l'aise avec Windows qui veulent garder un PC propre.

**Environnement** : Windows 10 22H2 et Windows 11 (23H2 et suivants), architecture x64 ; ARM64 en option. Droits administrateur requis pour la désinstallation et pour les zones `HKLM` et `Program Files`.

## 3. Exigences fonctionnelles

Vingt exigences réparties en cinq modules ; la priorité suit la méthode MoSCoW (M = indispensable, S = souhaitable, C = optionnel).

### 3.1 Inventaire des applications installées

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-01 | Lister les applications installées depuis les trois clés Uninstall (HKLM 64 bits, HKLM WOW6432Node, HKCU) avec nom, éditeur, version, date d'installation, taille, dossier d'installation et commande de désinstallation. | M |
| EF-02 | Masquer par défaut les composants système (`SystemComponent = 1`), les mises à jour et les entrées sans nom ; un filtre permet de les afficher. | M |
| EF-03 | Rechercher par nom ou éditeur, trier par nom, taille ou date. | M |
| EF-04 | Lister les applications Microsoft Store en lecture seule, avec un lien vers Paramètres pour les désinstaller. | C |

### 3.2 Recherche des restes orphelins

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-05 | Lancer un scan complet sans saisir de nom : dossiers de `Program Files`, `Program Files (x86)`, `%APPDATA%`, `%LOCALAPPDATA%`, `%LOCALAPPDATA%\Programs`, `%PROGRAMDATA%`, `Documents`. | M |
| EF-06 | Scanner le registre : `HKCU\Software`, `HKLM\SOFTWARE`, `HKLM\SOFTWARE\WOW6432Node`. | M |
| EF-07 | Scanner les raccourcis du menu Démarrer et du Bureau dont la cible n'existe plus, les tâches planifiées et services dont l'exécutable a disparu. | S |
| EF-08 | Considérer un élément comme orphelin s'il ne correspond à aucune application installée, en comparant noms, éditeurs, chemins d'installation et exécutables. | M |
| EF-09 | Regrouper les restes par application déduite (ex. : « Probablement : Audacity »), avec taille totale et date de dernière modification. | M |
| EF-10 | Attribuer à chaque élément un niveau de confiance (Élevée, Moyenne, Faible) ; seuls les éléments à confiance élevée sont précochés. | M |
| EF-11 | Afficher la progression du scan et permettre son annulation. | M |

### 3.3 Désinstallation complète

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-12 | Sélectionner une application installée et lancer son désinstalleur officiel (`UninstallString`, ou `msiexec /x` pour un MSI), en attendant la fin du processus. | M |
| EF-13 | Après la désinstallation, scanner automatiquement les restes de cette application et les afficher pour validation. | M |
| EF-14 | Proposer un mode silencieux (`QuietUninstallString`, `/quiet`) quand il existe. | S |
| EF-15 | Créer un point de restauration Windows avant la désinstallation, si la protection système est active. | S |

### 3.4 Suppression et sécurité

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-16 | Afficher un récapitulatif avant toute suppression : nombre d'éléments, taille, chemins ; confirmation explicite obligatoire. | M |
| EF-17 | Déplacer les fichiers et exporter les clés de registre vers une quarantaine avant suppression ; jamais de suppression définitive directe. | M |
| EF-18 | Restaurer une session de nettoyage, en totalité ou élément par élément, depuis l'historique. | M |
| EF-19 | Purger la quarantaine automatiquement après 30 jours (durée réglable) ou manuellement. | S |

### 3.5 Historique et réglages

| ID | Exigence | Priorité |
| --- | --- | --- |
| EF-20 | Tenir un historique des sessions (date, application, éléments, taille libérée) et permettre une liste d'exclusions personnalisée (dossiers ou clés à ne jamais proposer). | M |

## 4. Exigences non fonctionnelles

La sécurité prime sur tout le reste : un faux positif supprimé est bien plus grave qu'un reste oublié.

| ID | Domaine | Exigence | Mesure de vérification |
| --- | --- | --- | --- |
| ENF-01 | Sécurité | Liste noire non désactivable : `C:\Windows`, racines des lecteurs, dossiers utilisateur racines, `Microsoft`, `Windows`, `Common Files`, pilotes, clés `HKLM\SYSTEM`, `HKLM\SOFTWARE\Microsoft` et `\Classes`. | Tests unitaires sur chaque chemin protégé |
| ENF-02 | Sécurité | Aucune suppression sans confirmation explicite ; aucune suppression définitive sans passage en quarantaine. | Tests d'intégration |
| ENF-03 | Sécurité | Ne jamais suivre les liens symboliques ni les points de jonction pendant le scan et la suppression. | Test avec jonction piégée vers `C:\Windows` |
| ENF-04 | Sécurité | Mode « simulation » (dry run) qui produit la liste sans rien toucher. | Test : aucun fichier modifié |
| ENF-05 | Performance | Inventaire des applications en moins de 2 s ; scan complet des restes en moins de 60 s sur SSD avec 500 000 fichiers. | Benchmark Criterion + chronométrage UI |
| ENF-06 | Performance | Interface toujours réactive : tout travail long hors du thread UI, progression au moins toutes les 250 ms. | Revue de code + test manuel |
| ENF-07 | Ergonomie | Une désinstallation complète en 5 clics maximum depuis l'écran Applications, confirmation comprise. | Test utilisateur |
| ENF-08 | Ergonomie | Thèmes clair et sombre suivant Windows ; accessibilité clavier et lecteur d'écran (Narrateur). | Accessibility Insights |
| ENF-09 | Fiabilité | Une erreur sur un élément (fichier verrouillé, accès refusé) n'interrompt pas la session ; elle est listée dans le rapport final. | Tests d'intégration |
| ENF-10 | Traçabilité | Journal local de chaque opération (scan, déplacement, restauration) dans `%LOCALAPPDATA%\CleanSlate\logs`, rotation à 10 Mo. | Inspection des journaux |
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
- **Distribution** : installeur Inno Setup en v1.0 (MSIX écarté : incompatible avec `requireAdministrator`) ; signature de code à prévoir avant diffusion publique.
- **Licence** : MIT (fichier `LICENSE` à la racine du dépôt).

## 6. Livrables, jalons et acceptation

Le projet avance en six jalons séquentiels ; chacun se termine par une version qui compile, passe les tests et se démontre.

### 6.1 Jalons

1. **J1 — Socle** : monorepo, workspace Cargo, solution WinUI 3, DLL appelée depuis C# (fonction `version()`), CI verte.
2. **J2 — Inventaire** : EF-01 à EF-03 ; écran « Applications » fonctionnel.
3. **J3 — Moteur de détection** : EF-05 à EF-11 en mode simulation uniquement ; écran « Restes orphelins ».
4. **J4 — Quarantaine** : EF-16 à EF-19 ; premières suppressions réelles, toujours restaurables.
5. **J5 — Désinstallation complète** : EF-12 à EF-15 ; parcours complet de bout en bout.
6. **J6 — Finition** : historique, exclusions, réglages, thèmes, accessibilité, installeur.

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
