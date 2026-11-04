# Feuille de route de CleanSlate

## Où en est-on ?

- **Étape actuelle** : A5, usage réel. **Fait le 4 nov. 2026, à relire par le porteur** : Multicolore « street » (D76) ; barre de progression de la quarantaine (D77) ; critères de confiance et leur explication (D78) ; Gestionnaire, zones sensibles et mode expert (D79). Captures : `artifacts\captures\2026-11-04\multicolore-v2\` et `artifacts\captures\2026-11-04\gestionnaire\`.
- **Prochaine action** : le porteur installe l'installeur du 4 nov. 2026 (D85, SHA-256 `193BF357…6DDE39`) et essaie la page Stockage : la liste sous la carte (ouvrir des dossiers, Program Files (x86) et ses applications, un dossier comme VirtualBox, cocher puis supprimer en mode « Sûr » un fichier sans importance) ; choisit le marquage du menu (autocollant, pinceau ou éclaboussure) et dit ce qu'il veut pour le mode sombre (A street sur fond sombre, B retirer « Système », C garder).
- **Qui la fait** : le porteur.
- **Réponse attendue** : « tout OK » ou les remarques ; autocollant / pinceau / éclaboussure ; A / B / C ; « Nunito oui / non » ; garder ou non « Supprimer définitivement ».
- **Reste du Gestionnaire** : fait le 4 nov. 2026 (annulation au redémarrage, actions dans l'Historique, quarantaine sur un autre disque, espace non alloué, scénario Sandbox) ; limites dans `docs/securite/audit.md` § 4.
- **File des tâches** : vide (tâches 2 et 3 faites : D77, D78).

Dernière mise à jour : 4 nov. 2026.
## Règles de pilotage

- Claude pilote : à la fin de **chaque** réponse, il donne la prochaine action, qui la fait, la commande exacte et le dossier d'où la lancer, la durée et ce qu'il faut lui répondre.
- Claude ne lance ni Windows Sandbox ni CleanSlate (droits administrateur) : il donne la commande, le porteur l'exécute et répond « fait ».
- Après un scénario Sandbox, Claude lit lui-même le dernier dossier de `artifacts\sandbox\results\` : `rapport.md`, `journal.txt`, `diffs\`, `cleanslate-logs\`, `dumps\` et `evenements.txt`.
- Vérifications à la main : Claude donne une liste courte à cocher et la réponse attendue (« tout OK » ou ce qui ne va pas).
- Si un test échoue, Claude écrit d'abord un test qui reproduit le cas, puis corrige et dit quoi relancer, jusqu'à ce que tout passe.
- Claude décide seul de ce qui est technique et le consigne dans `docs/decisions.md`. Il ne pose une question que pour quatre cas : une règle de sécurité à affaiblir, une dépendance hors des spécifications, une dépense, ou une information qu'il ne peut pas connaître. Il propose alors une réponse par défaut.
- Les règles de sécurité de `CLAUDE.md` restent absolues.
- « suite » : Claude fait l'action suivante de cette feuille de route.
- Livre : rédigé par une autre session de Claude Code, dans ce même dossier, sur `main`, uniquement dans `book/` et `docs/prompts/livre.md`. Les règles qui s'appliquent ici :
  - ne jamais modifier `book/` ni `docs/prompts/livre.md` ;
  - lire directement `book/constats-pour-main.md`, au début de chaque session et avant chaque commit, et traiter une faille ou un bug qui y est signalé comme une correction normale, avec un test ;
  - règles de commit : voir `CLAUDE.md` (« Session du livre »).
## Étape A — Finaliser pour l'usage personnel

- [x] Mise en place du pilotage : feuille de route, consigne dans `CLAUDE.md`, `.claude/settings.json` commité (`ed923e8`).
- [x] **A1. Porteur** (atteint le 30 oct. 2026, run `apps-20261030-234036`) : scénario Apps dans le Sandbox. Critères :
  - ≥ 90 % des restes retrouvés ;
  - zéro élément fautif précoché ;
  - restauration à l'identique ;
  - aucun « échec du script ».

  La ligne « Information (D60) » sur GIMP n'est pas un critère.
- [x] **A2. Porteur** (atteint le 31 oct. 2026, run `complements-20261031-022958`, § 7.5) : scénario Complements, puis vérifications à la main, mode simulation désactivé :
  - VLC désinstallé avec des pourcentages ;
  - GIMP déjà lancé, puis désinstallé en mode silencieux : répondre Non, et vérifier que `%APPDATA%\GIMP` est proposé comme reste ;
  - export des journaux sans le nom d'utilisateur en clair ;
  - fenêtre à la largeur minimale.
- [x] **A3. Claude** (31 oct. 2026 : D59 à D68 ; dernier scénario Installer non relancé, à la demande du porteur) : corrections éventuelles, puis les scénarios nécessaires sont relancés ; faux positifs de l'analyse du vrai PC réduits et vérifiés par `scan --dry-run` (lecture seule).
- [x] **A4. Porteur** (31 oct. 2026) : point de restauration Windows, puis installation de CleanSlate sur son PC (Claude donne les commandes).
- [ ] **A5. Porteur** : utilisation progressive pendant une à deux semaines, dans cet ordre :
  1. relecture des listes, sans rien valider ;
  2. petite quarantaine ;
  3. test de restauration ;
  4. élargissement.

  Il n'y a plus de mode simulation (D66) : relire sans valider en tient lieu. Pas de services, de tâches, de mode silencieux ni de suppression immédiate la première semaine. Pour chaque problème, le porteur écrit « Problème : … » et joint le zip de journaux exportés. Claude analyse le zip, reproduit le cas par un test sans aucune opération destructive sur la machine, corrige, et dit comment restaurer si besoin.
- [ ] **A6.** « usage perso validé » → étape B.

## Étape B — Partager avec un ami (sur son propre PC)

- [ ] **B1. Claude** : audit de sécurité.
  - Outils, autorisés dans le profil du porteur : `cargo-audit`, `cargo-deny`, `cargo-fuzz` et la toolchain nightly.
  - Modèle de menaces : un malware sans privilèges dans la même session, un utilisateur local malveillant, la chaîne d'approvisionnement, un faux installeur.
  - Points à examiner au minimum :
    - élévation de privilèges via HKCU et AppData ;
    - altération de la quarantaine, des manifestes, des `.reg`, des XML de tâches, des configurations de services et de la base SQLite ;
    - courses TOCTOU, jonctions et liens symboliques ;
    - injection via UninstallString et MSI ;
    - détournement de DLL et droits du dossier d'installation ;
    - faux positifs et exceptions de la liste noire ;
    - tâches et services ;
    - `unsafe` et FFI ;
    - déni de service ;
    - vie privée des journaux ;
    - chaîne d'approvisionnement ;
    - interface trompeuse.
  - Méthodes : `cargo audit`, `cargo deny`, `dotnet list package --vulnerable`, les ACL des dossiers, la relecture des blocs `unsafe`, et du fuzzing ou des tests de propriétés.
  - Résultat : `docs/securite/audit.md`, avec une fiche par faille et un tableau récapitulatif. Pour chaque faille réelle, un test qui la reproduit, puis un commit `fix(security): …`. Toute correction qui change l'architecture ou une règle de sécurité est présentée au porteur avant.
- [ ] **B2. Claude** :
  - ~~écran d'avertissement au premier lancement, avec une case « J'ai compris » avant de pouvoir désactiver la simulation~~ : fait en avance avec le premier lancement (D63) ;
  - dossiers connus via `SHGetKnownFolderPath` partout (Windows dans une autre langue, dossiers redirigés vers OneDrive) ;
  - versions cohérentes de la DLL, de la CLI, de l'exe et de l'installeur (`winresource` est autorisé comme dépendance de compilation) ;
  - `CHANGELOG.md` ;
  - `docs/guide-utilisateur.md`, court et sans jargon, exporté aussi en PDF ;
  - fichier SHA256 de l'installeur.
- [ ] **B3. Porteur** : les scénarios Complements, Apps et Installer, avec les corrections de Claude si besoin.
- [ ] **B4. Porteur** : installer la nouvelle version sur son PC et vérifier.
- [ ] **B5. Porteur** : envoyer à l'ami l'installeur, le SHA256 et le guide PDF. Ses problèmes reviennent par ses journaux exportés et sont traités comme en A5.

## Étape C — Publier sur GitHub

- [ ] **C1. Claude** : demander le nom d'utilisateur GitHub et rappeler d'activer la double authentification. Dépôt : `<compte>/cleanslate`.
- [ ] **C2. Claude** :
  - README complet : logo, motivation, fonctionnalités, captures si possible, installation (avertissement SmartScreen, vérification SHA-256), utilisation prudente, signaler un problème, compilation, tests, licence ;
  - `SECURITY.md` (signalement privé de GitHub, aucune adresse e-mail), `CONTRIBUTING.md`, `PRIVACY.md` et `CODE_SIGNING_POLICY.md` conforme à SignPath Foundation ;
  - modèles d'issues ;
  - workflow de release déclenché par un tag `v*` : build, tests, contrôle de la CLI, installeur, `SHA256SUMS.txt`, notes tirées de `CHANGELOG.md`, release en brouillon, étape SignPath désactivée ;
  - manifeste winget dans `packaging/winget/` ;
  - vérification qu'aucun fichier privé ni nom personnel n'est versionné ; la liste est présentée avant de toucher à l'historique.
- [ ] **C3. Porteur** : les scénarios indiqués par Claude si le code ou l'installeur a changé.
- [ ] **C4. Porteur**, avec chaque commande donnée par Claude au bon moment :
  1. créer le dépôt privé et pousser ;
  2. vérifier la CI ;
  3. rendre le dépôt public ;
  4. activer le signalement privé de vulnérabilités ;
  5. créer le tag `v1.0.0` et publier la release ;
  6. faire la demande à SignPath.

## Étape D — Livre

- [ ] **D. Livre** : rédigé par une autre session dans book/, sur main. À la fin, traiter book/constats-pour-main.md.
