# Revue finale — CleanSlate v1.0

Dernière mise à jour : 30 oct. 2026

Chaque exigence du [cahier des charges](../cahier-des-charges.md) avec les fichiers qui la réalisent et les tests qui la vérifient. Les tests Rust sont dans le même fichier que le code (`core/cleanslate-core/src/`), les tests C# dans `app/CleanSlate.Tests/`. « Sandbox » renvoie aux procédures manuelles de [tests-acceptation.md](../tests-acceptation.md).

État au 30 oct. 2026 : 170 tests du moteur, 14 tests FFI, 68 tests C# ; `cargo fmt` et `cargo clippy -D warnings` propres ; couverture du moteur **92,9 %** des lignes (ENF-12 : ≥ 80 %) ; build Release et installeur produits.

Légende : ✅ fait et testé · ⚠️ partiel ou à vérifier · ⏭️ reporté.

## Exigences fonctionnelles

| ID | Prio. | État | Fichiers | Tests |
| --- | --- | --- | --- | --- |
| EF-01 Inventaire des 3 clés Uninstall | M | ✅ | `inventory.rs`, `AppsViewModel.cs`, `AppsPage.xaml` | `list_apps_reads_the_three_views`, `inno_setup_entry`, `msi_entry`, `nsis_entry_with_unquoted_path`, `install_date_formats` ; `Load_SortsByNameAndHidesSystemComponents` |
| EF-02 Masquer composants système, mises à jour, sans nom | M | ✅ | `inventory.rs` (`hidden_reason`), `AppsViewModel.cs` | `hidden_entries_are_flagged_not_dropped`, `nameless_entries_are_dropped`, `kb_detection_needs_a_real_reference` ; `ShowSystemComponents_RevealsHiddenEntries` |
| EF-03 Recherche et tri | M | ✅ | `AppsViewModel.cs` | `Search_MatchesNameOrPublisher_IgnoringCaseAndAccents`, `Sort_BySizeAndDate_PutsUnknownLast` |
| EF-04 Applications Microsoft Store | C | ⏭️ | — | Reportée en v1.1 (cahier § 6.5) |
| EF-05 Scan des dossiers sans nom saisi | M | ✅ | `scan.rs` (`ScanRoots`), `OrphansViewModel.cs` | `fixture_finds_leftovers_and_never_installed_apps` ; Sandbox § 4 |
| EF-06 Scan du registre | M | ✅ | `scan.rs`, `registry.rs` | `fixture_finds_leftovers_and_never_installed_apps` (ruche de test), `registry_value_sizes` |
| EF-07 Raccourcis morts, tâches, services | S | ✅ | `scan.rs` (`.lnk` des Bureaux, `dead_entry_leftover`), `system.rs` (`dead_tasks`, `dead_services`) | `shortcut_parsing`, `dead_tasks_are_found_and_system_ones_ignored`, `dead_services_skip_drivers_and_windows`, `dead_tasks_and_services_are_shown_never_prechecked` ; `DeadTasksAndServices_CanNeverBeChecked`. Raccourcis du menu Démarrer non proposés (D20 validée) ; tâches et services détectés seulement, jamais mis en quarantaine (v1.1, D47) |
| EF-08 Orphelin = ne correspond à aucune application installée | M | ✅ | `matcher.rs` | `attach_by_install_location_both_ways`, `attach_by_uninstaller_folder_and_executables`, `attach_by_similar_name_or_publisher`, `present_programs_attach_their_data` (D22) |
| EF-09 Regroupement par application déduite | M | ✅ | `matcher.rs` (`group`), `OrphansPage.xaml` | `grouping_by_normalized_name_largest_first` |
| EF-10 Confiance, seuls les « Élevée » précochés | M | ✅ | `matcher.rs`, `OrphansViewModel.cs` | `old_unrelated_folder_is_high`, `recent_or_generic_or_portable_is_lower`, `items_under_an_installed_publisher_are_at_most_medium` (D23), `personal_folders_are_never_high` ; `Scan_PrechecksOnlyHighConfidence` |
| EF-11 Progression et annulation | M | ✅ | `scan.rs` (`Reporter`), `cs_cancel`, `EngineService.cs` | `reporter_throttles_to_250_ms`, `cancelled_scan_returns_partial_flagged_report`, `scan_orphans_with_progress` ; `CancelledScan_KeepsPartialResults` |
| EF-12 Désinstalleur officiel, attente de fin | M | ✅ | `uninstall.rs`, `engine.rs::uninstall`, `UninstallWizardViewModel.cs` | `split_real_uninstall_strings`, `shells_and_empty_strings_are_refused`, `job_runner_waits_for_a_real_process`, `uninstall_simulated_then_real_then_leftovers` ; Sandbox § 3 |
| EF-13 Restes de l'application après désinstallation | M | ✅ | `scan.rs::scan_app_leftovers` | `leftovers_of_a_just_uninstalled_app`, `missing_uninstaller_leftovers_are_searched_but_never_high` (D45) ; `CancelledUninstaller_SkipsLeftovers`, `NoLeftover_SkipsStepThree`, `MissingUninstaller_SearchesLeftoversAnyway_NothingPrechecked` |
| EF-14 Mode silencieux | S | ✅ | `uninstall.rs::build_command` | `build_command_for_msi_quiet_and_missing` ; `ClassicUninstall_StopsAfterTheUninstaller_AndQuietIsSent` |
| EF-15 Point de restauration | S | ✅ | `uninstall.rs::SystemRestorePoints` | `uninstall_simulated_then_real_then_leftovers` (échec simulé, non bloquant) ; vrai point de restauration : Sandbox § 3 seulement |
| EF-16 Récapitulatif et confirmation explicite | M | ✅ | `DialogService.cs`, `OrphansViewModel.cs` | `Quarantine_AlwaysAsksForConfirmation_AndStopsWhenCancelled`, `Quarantine_AboveCeiling_AsksForTheWord`, `ceiling_requires_the_confirmation_word` |
| EF-17 Quarantaine, jamais de suppression directe | M | ✅ | `quarantine.rs`, `native.rs` | `quarantine_then_restore_identically`, `interrupted_session_is_recovered`, `snapshot_delete_and_restore_identically` |
| EF-18 Restauration totale ou par élément | M | ✅ | `quarantine.rs::restore`, `HistoryPage.xaml` | `partial_restore_and_existing_destination` ; `History_LoadsRestoresCheckedItemsAndPurges` |
| EF-19 Purge après 30 jours (réglable) ou manuelle | S | ✅ | `quarantine.rs::purge`, `App.xaml.cs` (purge au démarrage), `SettingsViewModel.cs` | `purge_removes_only_old_sessions_inside_the_quarantine` ; `Settings_ClampDaysAndWriteThrough` |
| EF-20 Historique et exclusions | M | ✅ | `history.rs` (SQLite), `safety.rs` (exclusions), `SettingsService.cs` (`settings.json`, D40), `HomeViewModel.cs` | `sessions_round_trip_most_recent_first`, `user_exclusions_protect_the_folder_and_its_content` ; `Settings_ExclusionsAndTheme`, `Load_CountsVisibleApps_KeepsThreeSessions_AndSumsFreedSpace` |

## Exigences non fonctionnelles

| ID | État | Fichiers | Vérification |
| --- | --- | --- | --- |
| ENF-01 Liste noire non désactivable | ✅ | `safety.rs` | `system_folders_are_absolute_and_protected`, `hives_and_roots_are_never_deleted`, `protected_and_linked_items_are_refused_and_left_in_place` |
| ENF-02 Confirmation et quarantaine obligatoires | ✅ | `quarantine.rs`, `DialogService.cs` | `Quarantine_AlwaysAsksForConfirmation_AndStopsWhenCancelled`, `quarantine_then_restore_identically` |
| ENF-03 Jamais de lien symbolique ni de jonction | ✅ | `fs.rs`, `safety.rs::validate_path`, `quarantine.rs` (D29) | `trapped_junction_to_windows_is_refused` (vraie jonction), `reparse_point_on_the_way_is_refused`, `fake_refuses_to_walk_reparse_points` |
| ENF-04 Mode simulation | ✅ | partout : `dry_run = true` par défaut, double verrou (D27) | `config_defaults_to_dry_run`, `clean_plan_defaults_to_dry_run`, `simulation_touches_nothing`, `engine_new_defaults_to_dry_run` ; `EngineConfig_DefaultsToDryRun`, `Simulate_ReportsWithoutTouchingAnything` |
| ENF-05 Inventaire < 2 s ; scan < 60 s pour 500 000 fichiers | ✅ | `inventory.rs`, `scan.rs` | Mesuré : inventaire réel 25 à 68 ms (292 entrées), scan réel 7,6 s ; banc `benches/scan.rs` (`enf05/scan_500k_files`) : 554 ms |
| ENF-06 UI réactive, ≤ 1 événement / 250 ms | ✅ | `scan.rs` (`Reporter`), `EngineService.cs` (`Task.Run`) | `reporter_throttles_to_250_ms` ; annonces au Narrateur limitées à une toutes les 2 s |
| ENF-07 Désinstallation complète en ≤ 5 clics | ✅ | `UninstallWizardViewModel.cs` | `FullUninstall_InFiveClicks` ; Sandbox § 3 étape 5 |
| ENF-08 Thèmes, clavier, Narrateur | ⚠️ | `App.xaml` (clair, sombre, `HighContrast`, accent sarcelle), `SettingsPage.xaml` (thème), `AutomationProperties` dans les vues, `OrphansPage.xaml.cs` (`LiveRegionChanged`) | Vérifié à l'écran en sombre ; **Accessibility Insights et Narrateur non passés** : à faire par le porteur |
| ENF-09 Une erreur n'arrête pas la session | ✅ | `scan.rs` (`Errors`), `quarantine.rs` | `protected_and_linked_items_are_refused_and_left_in_place` (session « Partielle ») |
| ENF-10 Journal local, rotation quotidienne, 7 fichiers | ✅ | `logging.rs`, `engine.rs::with_system` | `daily_files_in_the_given_folder` |
| ENF-11 Aucun envoi sur Internet | ✅ | `core/Cargo.toml`, `*.csproj` | Revue des dépendances : aucune crate ni paquet réseau ; aucune télémétrie |
| ENF-12 Couverture ≥ 80 %, clippy, MVVM | ✅ | `.github/workflows/ci.yml` | `cargo llvm-cov` : 92,9 % ; clippy `-D warnings` ; ViewModels dans `CleanSlate.Presentation`, vues sans logique métier |

## États de l'interface (design § 5)

| Situation | Action | Fichiers | Tests |
| --- | --- | --- | --- |
| Aucun reste trouvé | « Retour à l'accueil » | `OrphansPage.xaml`, `OrphansViewModel.cs` (`GoHome`) | `StateActions_NavigateAndListIgnoredItems` |
| Analyse annulée | « Relancer » | `OrphansPage.xaml` (`ScanCommand`) | `StateActions_NavigateAndListIgnoredItems`, `CancelledScan_KeepsPartialResults` |
| Fichier verrouillé | « Voir les fichiers » (Historique) | `OrphansViewModel.cs` (`OpenHistory`) | `StateActions_NavigateAndListIgnoredItems` |
| Accès refusé | « Détails » : liste des éléments ignorés | `OrphansViewModel.cs` (`ItemErrors`, `ToggleItemErrors`) | `StateActions_NavigateAndListIgnoredItems` |
| Mode simulation | « Désactiver la simulation » (Réglages) | `OrphansViewModel.cs` (`OpenSettings`), `NavigationService.cs`, `MainWindow.xaml.cs` (`FooterMenuItems`) | `StateActions_NavigateAndListIgnoredItems` |
| Historique vide | « Analyser les restes » | `HistoryViewModel.cs` (`AnalyzeLeftovers`), `HistoryPage.xaml` | `EmptyHistory_OffersToAnalyzeLeftovers` |
| Lancé sans droits admin | « Redémarrer en administrateur » | `HomeViewModel.cs`, `App.xaml.cs` | `NotElevated_ShowsTheElevationWarning` |
| Désinstalleur introuvable | « Rechercher ses restes quand même » (D45) | `UninstallWizardViewModel.cs` (`SearchLeftoversAnyway`), `engine.rs::scan_app_leftovers` | `MissingUninstaller_*`, `OtherErrors_CannotSearchAnyway`, `missing_uninstaller_leftovers_are_searched_but_never_high` |
| Confirmation renforcée | mot « QUARANTAINE » | `DialogService.cs` | `Quarantine_AboveCeiling_AsksForTheWord` |

## Points ouverts

1. **EF-07** (D20 et D47, validées par le porteur le 30 oct. 2026) : liste noire inchangée, les `.lnk` morts du menu Démarrer ne sont pas proposés ; tâches planifiées et services détectés (v1.1), à supprimer à la main.
2. **ACL d'origine** (D31, validé par le porteur le 30 oct. 2026) : enregistrée dans le manifeste, pas réappliquée après une copie entre deux volumes.
3. **Signal « nom d'une application désinstallée par CleanSlate »** (+30, specs § 6.3) : reporté (D38).
4. **Progression de `cs_uninstall`** : rappel réservé ; barre indéterminée dans l'assistant.
5. **ENF-08** : passer Accessibility Insights et le Narrateur sur les 5 écrans.
6. **Critères du cahier § 6.3** (90 % des restes, zéro faux précochage, restauration à l'identique) : à mesurer dans Windows Sandbox ([tests-acceptation.md § 4](../tests-acceptation.md)). Ils ne peuvent pas être mesurés sur la machine de développement sans opération destructive. Les critères 2 et 3 sont mesurés automatiquement par `scripts/sandbox/` (D46) dès que Windows Sandbox est activé ; le critère 4 reste à la main.
7. **Signature de code** : l'installeur n'est pas signé ; SmartScreen avertira à l'installation.
8. **Windows App SDK 2.x** : migration étudiée en v1.1 (D42).
