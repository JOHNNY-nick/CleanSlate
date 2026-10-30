# v1.1 — EF-07 complet : tâches planifiées et services morts — bilan

Dernière mise à jour : 30 oct. 2026

## Ce qui est fait

| Exigence | Réalisation | Fichiers | Tests |
| --- | --- | --- | --- |
| EF-07 | Tâches planifiées dont tous les programmes ont disparu : lecture des définitions XML (UTF-16) de `%WINDIR%\System32\Tasks` ; `\Microsoft\…`, programmes sous `%WINDIR%`, commandes relatives ou à variable inconnue écartés | `system.rs` (`dead_tasks`) | `dead_tasks_are_found_and_system_ones_ignored`, `unreadable_task_folder_gives_nothing` |
| EF-07 | Services dont l'exécutable (`ImagePath`) a disparu ; pilotes (types 1 et 2) et programmes sous `%WINDIR%` écartés | `system.rs` (`dead_services`) | `dead_services_skip_drivers_and_windows` |
| EF-07, EF-09, EF-10 | Éléments du rapport : regroupés avec l'application déduite du dossier du programme, au plus « Moyenne » (« Faible » si le dossier appartient à une application installée), motif « Exécutable introuvable : … » et la façon de les supprimer à la main ; option `include_tasks_and_services` | `scan.rs` (`dead_entry_leftover`, `guess_app`), `model.rs` (`ScanOptions`) | `dead_tasks_and_services_are_shown_never_prechecked` |
| EF-07, ENF-02 | Détection seule (D47) : case à cocher désactivée ; la quarantaine refuse déjà ces types (`UNSUPPORTED`) | `LeftoverGroupViewModel.cs` (`IsSelectable`), `OrphansPage.xaml`, `UninstallWizardPage.xaml` | `DeadTasksAndServices_CanNeverBeChecked`, `protected_and_linked_items_are_refused_and_left_in_place` (existant : un `Service` est refusé) |

**`safety.rs` n'a pas été modifié** : la liste noire est intacte. Les zones lues (`System32\Tasks`, clé `Services`) le sont en lecture seule, comme la lecture des services existante.

Chiffres : 170 tests du moteur + 14 FFI, 68 tests C# ; `cargo fmt` et `cargo clippy -D warnings` propres ; build Release de l'application sans avertissement.

Essai réel en lecture seule (`cleanslate-cli scan --dry-run`, sans droits administrateur) : 1 service mort trouvé, en « Faible » parce que son dossier appartient à une application encore listée ; les tâches ne sont lisibles qu'en administrateur (mode normal de l'application), le dossier illisible est ignoré sans erreur.

## Comment tester

- `cargo test --manifest-path core/Cargo.toml --workspace` ; `dotnet test app/CleanSlate.sln -p:Platform=x64`.
- Application lancée en administrateur → Restes orphelins → Analyser : les tâches et services morts apparaissent avec le type « Tâche planifiée » ou « Service », une case grisée et le motif « Exécutable introuvable ».
- Dans Windows Sandbox : créer une tâche et un service vers un programme absent, par exemple `schtasks /create /tn Test\Mort /tr C:\Absent\x.exe /sc daily` et `sc.exe create Mort binPath= C:\Absent\svc.exe`, puis analyser.

## Écarts et points d'attention

- **Pas de mise en quarantaine** des tâches et services (décision du porteur) : l'utilisateur les supprime dans le Planificateur de tâches ou avec `sc.exe delete`. La piste technique reste dans le [plan](V11-EF07-plan.md).
- **Raccourcis du menu Démarrer** : toujours non proposés (D20 validée).
- **Restes d'une application désinstallée** (assistant) : les tâches et services ne sont cherchés que par l'analyse des restes orphelins.

## Notions Rust du jalon

- **Motif de tranche** — `system.rs:217` : `[0xFF, 0xFE, rest @ ..]` vérifie que les deux premiers octets valent `FF FE` (la marque UTF-16) et lie le reste de la tranche à `rest`, sans indice ni risque de dépassement. `match` essaie les motifs dans l'ordre ; le dernier, `_`, prend tout le reste.
- **`as_chunks::<2>()`** — `system.rs:219` : découpe une tranche d'octets en tableaux de taille fixe `[u8; 2]` (le `::<2>` donne la taille, connue à la compilation) et rend à part l'éventuel octet seul à la fin. Chaque paire devient un `u16` par `from_le_bytes`, sans indexation qui pourrait paniquer.
- **`while let`** — `system.rs:203` : répète la boucle tant que `find` renvoie `Some(i)` ; dès qu'il renvoie `None`, la boucle s'arrête. C'est la forme boucle de `if let`.
- **`bool::then`** — `system.rs:135` : `(!fs.exists(&program)).then(|| DeadEntry { … })` donne `Some(…)` si la condition est vraie, sinon `None`. La fermeture n'est exécutée que dans le premier cas.
- **`Path::ancestors`** — `scan.rs:500` : itère sur le chemin puis tous ses parents (`C:\a\b\x.exe`, `C:\a\b`, `C:\a`, `C:\`) ; `skip(1)` saute le fichier lui-même. On remonte ainsi jusqu'au premier nom de dossier qui n'est pas générique (`bin`, `x64`…).
- **`Iterator::chain`** — `scan.rs:199` : met deux itérateurs bout à bout (les tâches puis les services) pour les traiter dans une seule boucle, sans créer de vecteur intermédiaire.
