# J5 — Désinstallation complète : bilan

Dernière mise à jour : 29 oct. 2026

## Ce qui est fait

| Exigence | Réalisation | Fichiers | Tests |
| --- | --- | --- | --- |
| EF-12 | Désinstalleur officiel : `msiexec /x {ProductCode}` pour MSI, sinon `UninstallString` découpée sans shell (règle 8) ; Job Object ; attente de tout l'arbre (30 min au plus) ; vérification de la disparition de l'entrée | `uninstall.rs`, `engine.rs::uninstall` | 15 formes réelles (`split_real_uninstall_strings`), `shells_and_empty_strings_are_refused`, `job_runner_waits_for_a_real_process` (`hostname.exe`, inoffensif), `uninstall_simulated_then_real_then_leftovers` (faux désinstalleur) |
| EF-13 | Restes de l'application (§ 6.4) : ancien dossier, nom ≥ 0,85, clé au `ProductCode`, raccourcis vers l'ancien dossier, dossier éditeur sans autre application ; jamais un élément d'une autre application ; rien si toujours installée | `scan.rs::scan_app_leftovers` | `leftovers_of_a_just_uninstalled_app` |
| EF-14 | Mode silencieux (`QuietUninstallString`, `/qn`) proposé quand il existe | `build_command`, `UninstallWizardViewModel.CanQuiet` | `build_command_for_msi_quiet_and_missing`, `ClassicUninstall_StopsAfterTheUninstaller_AndQuietIsSent` |
| EF-15 | Point de restauration (`SRSetRestorePointW`) coché par défaut ; échec signalé, non bloquant | `uninstall.rs::SystemRestorePoints`, `engine.rs` | `uninstall_simulated_then_real_then_leftovers` (échec simulé) |
| ENF-07 | Parcours complet en 5 clics depuis Applications | `UninstallWizardViewModel`, `UninstallWizardPage` | `FullUninstall_InFiveClicks` |

Aussi : bouton « Désinstallation classique » (désinstalleur seul, D14), étape 3 sautée sans reste, cas « désinstalleur annulé » (D36), simulation par défaut (la commande est seulement affichée).

Chiffres : 164 tests core + 15 FFI, 58 tests C#, couverture du moteur **92,8 %** (`uninstall.rs` 91,6 %), fmt et clippy propres, build Release réussi.

**Aucune vraie désinstallation sur la machine du porteur.** Vérifié dans l'application, en simulation : assistant ouvert sur 7-Zip, « Lancer » → étape 4 avec la commande `"C:\Program Files\7-Zip\Uninstall.exe"` ; 7-Zip toujours installé ensuite.

## Comment tester

- Automatique : `cargo test --manifest-path core/Cargo.toml --workspace` ; `dotnet test app/CleanSlate.sln -c Release -p:Platform=x64`.
- Réel : [tests-acceptation.md § 3](../tests-acceptation.md) dans Windows Sandbox (7-Zip, VLC, Notepad++).

## Écarts avec les specs et points d'attention

- **Délai et annulation** (D35) : l'attente s'arrête, le désinstalleur continue.
- **Shells refusés** (D34) : une application dont l'`UninstallString` passe par `cmd /c` ne peut pas être désinstallée par CleanSlate (message « Le programme de désinstallation… »).
- **Entrée encore présente** : la specs § 7.1 proposait de retirer l'entrée Uninstall avec les restes ; elle est sous `SOFTWARE\Microsoft\…`, donc protégée par la liste noire : CleanSlate l'indique seulement (D36).
- **Signal historique (+30)** : reporté (D38).
- **Progression** de `cs_uninstall` : le rappel est réservé ; l'interface affiche une barre indéterminée.

## Notions Rust du jalon

- **Traits pour les effets de bord** — `uninstall.rs:242` : `ProcessRunner` (et `RestorePoints`) séparent la logique de l'action réelle ; le moteur reçoit une `Box<dyn ProcessRunner>` (`engine.rs:277`) et les tests lui donnent un faux (`engine.rs:649`) qui supprime l'entrée de la ruche de test au lieu de lancer un programme.
- **Indices de caractères** — `uninstall.rs:84` : `char_indices()` donne la position en octets de chaque caractère ; `text[index..]` découpe la chaîne sans couper un caractère UTF-8 en deux.
- **`Option::or_else`** — `uninstall.rs:148` : « le premier candidat qui existe, sinon le dernier » s'écrit `find(...).or_else(|| last())`, sans `if` imbriqués.
- **Module privé pour l'`unsafe`** — `uninstall.rs:296` : tout l'appel Win32 vit dans `mod win` ; le reste du module ne voit qu'une fonction sûre `run_in_job`.
- **Structures « packed »** — `uninstall.rs:452` : `RESTOREPOINTINFOW` est alignée sur 1 octet ; prendre une référence vers l'un de ses champs serait un comportement indéfini, le compilateur l'interdit (erreur E0793). On remplit donc le texte dans une variable locale, puis on la place dans la structure.
