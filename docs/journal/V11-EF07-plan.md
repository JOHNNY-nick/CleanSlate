# v1.1 — EF-07 complet : tâches planifiées et services morts — plan

Dernière mise à jour : 30 oct. 2026

But : terminer EF-07 (cahier § 4) en détectant les tâches planifiées et les services Windows dont l'exécutable a disparu. Les raccourcis restent comme aujourd'hui (D20 validée : Bureaux seulement).

## Point délicat

Les définitions de tâches (`C:\Windows\System32\Tasks`) et les services (`HKLM\SYSTEM\CurrentControlSet\Services`) sont dans des zones protégées par la liste noire de `safety.rs`. La liste noire **n'est pas modifiée** :

- la lecture de ces zones est déjà permise (le scan lit les services, `system.rs`) ;
- une éventuelle suppression ne passerait pas par un chemin ou une clé : elle utiliserait les API dédiées (Planificateur de tâches, `DeleteService`), avec des règles propres à chaque type.

## Étape 1 — Détection (lecture seule)

| Fichier | Travail |
| --- | --- |
| `system.rs` | `scheduled_tasks(fs)` : parcours de `System32\Tasks` par le trait `FileSystem`, XML UTF-16, extraction du chemin de la tâche et des `<Command>` ; `\Microsoft\…` ignoré. `services(registry)` : nom, `ImagePath`, `Start`, `Type` ; pilotes, binaires sous `%windir%` et `svchost` ignorés |
| `system.rs` | Résolution d'une commande : variables d'environnement, guillemets, arguments ; « disparu » = exécutable absent |
| `scan.rs` | Candidats `ScheduledTask` (« Tâche \Nom ») et `Service` (« Service Nom »), rattachés à l'application déduite du dossier de l'exécutable ; confiance plafonnée à « Moyenne » : jamais précochés |
| `OrphansPage.xaml`, `.resw` | Libellés « Tâche planifiée », « Service », motif « Exécutable introuvable : … » |

Tests : arbre de tâches factice (`FakeFileSystem`), services dans la ruche de test (`TestRegistryKey`) ; tâches Microsoft, pilotes, exécutable présent et commande entre guillemets avec arguments.

## Étape 2 — Mise en quarantaine : écartée (décision du porteur, 30 oct. 2026)

Le porteur a choisi la détection seule : tâches et services morts affichés, jamais cochables ; le moteur refuse de les mettre en quarantaine. Le détail ci-dessous reste comme piste pour une version ultérieure.

| Fichier | Travail |
| --- | --- |
| `quarantine.rs`, `native.rs` | Tâche : copie de sa définition XML dans la session (manifeste écrit avant), puis suppression par le Planificateur (COM `ITaskFolder::DeleteTask`) ; restauration par `RegisterTask` depuis le XML. Service : export de sa clé dans la session, puis `DeleteService` ; restauration par réimport et `CreateService` |
| `safety.rs` (règles **ajoutées**, rien de retiré) | Refus : tâche sous `\Microsoft\`, service pilote, démarrage Boot/System, binaire sous `%windir%`, service en cours d'exécution |
| `Cargo.toml` | Fonctions supplémentaires de la crate `windows` déjà utilisée : `Win32_System_TaskScheduler`, `Win32_System_Services`, `Win32_System_Variant` (pas de nouvelle crate) |

Tests : règles de refus en unitaire ; suppression et recréation réelles uniquement dans Windows Sandbox (tâche et service factices créés par `inside-apps.ps1`).

## Étape 3 — Finition

Tests des ViewModels, `revue-finale.md` (EF-07 ✅), décision D47, bilan `V11-EF07-bilan.md`. Un commit par étape.
