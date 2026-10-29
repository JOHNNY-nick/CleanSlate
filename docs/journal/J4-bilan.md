# J4 — Quarantaine, restauration et historique : bilan

Dernière mise à jour : 29 oct. 2026

## Ce qui est fait

| Exigence | Réalisation | Fichiers | Tests |
| --- | --- | --- | --- |
| EF-16, ENF-02 | Dialogue de confirmation obligatoire : récapitulatif par type, total, rappel de la durée, mention de la simulation, bouton rouge, « Annuler » par défaut ; saisie de « QUARANTAINE » au-delà de 20 Go / 50 000 fichiers | `DialogService.cs`, `OrphansViewModel`, `quarantine.rs::check_ceiling` | `Quarantine_AlwaysAsksForConfirmation…`, `Quarantine_AboveCeiling_AsksForTheWord`, `ceiling_requires_the_confirmation_word` |
| EF-17 | Manifeste écrit avant chaque déplacement puis mis à jour ; `validate_path` / `validate_key` juste avant d'agir ; SHA-256 BCrypt ; SDDL ; `MoveFileExW` ou copie vérifiée ; clés exportées en JSON et `.reg` UTF-16 LE puis supprimées ; fichier verrouillé → redémarrage | `quarantine.rs`, `native.rs`, `registry.rs` | `quarantine_then_restore_identically`, `protected_and_linked_items_are_refused_and_left_in_place`, `snapshot_delete_and_restore_identically` |
| EF-18 | Restauration totale ou partielle, refus si la destination existe | `quarantine.rs::restore`, `HistoryViewModel` | `partial_restore_and_existing_destination`, `History_LoadsRestoresCheckedItemsAndPurges` |
| EF-19 | Purge par âge (limitée au dossier de quarantaine), au démarrage hors simulation et sur demande confirmée ; durée réglable 1 à 365 jours | `quarantine.rs::purge`, `App.xaml.cs`, `SettingsViewModel` | `purge_removes_only_old_sessions_inside_the_quarantine`, `Settings_ClampDaysAndWriteThrough` |
| EF-20 (historique) | SQLite : `sessions`, `items`, `exclusions`, `settings` ; page Historique | `history.rs`, `HistoryPage` | `sessions_round_trip_most_recent_first`, `exclusions_and_settings` |
| ENF-04 | Double verrou : action réelle seulement si le moteur **et** la requête ne sont pas en simulation (D27) ; mode simulation actif par défaut | `engine.rs`, `SettingsService` | `scan_clean_list_restore_cycle_respects_dry_run`, `clean_is_simulated_by_default…` |
| ENF-09 | Un échec par élément n'arrête pas la session ; session « Partielle » | `quarantine.rs` | `protected_and_linked…` |
| Reprise | Session interrompue reprise sans perte | `quarantine.rs::recover` | `interrupted_session_is_recovered` |

Chiffres : 163 tests core + 13 FFI, 52 tests C#, couverture du moteur **92,7 %** (`quarantine.rs` 89,5 %), fmt et clippy propres, build Release réussi.

**Aucune vraie mise en quarantaine sur la machine du porteur** : tous les tests destructifs tournent dans `core/target/…` et sous `HKCU\Software\CleanSlateTest\<uuid>` (vérifié vide après les tests). L'application a été lancée en mode simulation (par défaut) : dialogue de confirmation vérifié, rien confirmé.

## Comment tester

- Automatique : `cargo test --manifest-path core/Cargo.toml --workspace` et `dotnet test app/CleanSlate.sln -c Release -p:Platform=x64`.
- Réel : procédure Windows Sandbox, [tests-acceptation.md § 2](../tests-acceptation.md).

## Écarts avec les specs et points d'attention

- **ACL** (D31) : enregistrée mais pas réappliquée ; une copie inter-volumes prend l'ACL de la destination.
- **Restauration des clés** (D30) : depuis l'instantané JSON ; le `.reg` est fourni pour une restauration manuelle.
- **Tâches planifiées et services** : non pris en charge (non détectés en J3, D20) → refusés `UNSUPPORTED`.
- **Signal « historique » (+30)** du score : l'historique existe, mais le signal n'est pas encore branché sur le score (il ne concerne que les applications désinstallées par CleanSlate, J5).
- **Exclusions** : table prête et passée au moteur via `EngineConfig.exclusions` ; leur édition dans les Réglages arrive en J6.
- **Changement du mode simulation** (D32) : le moteur est recréé, l'analyse doit être relancée.

## Notions Rust du jalon

- **RAII sur une ressource Windows** — `native.rs:108` : `impl Drop for Sha256` détruit la poignée CNG quand la valeur sort de portée, même en cas d'erreur `?` au milieu du calcul ; impossible d'oublier `BCryptDestroyHash`.
- **Garde de `Mutex` renvoyée** — `engine.rs:234` : `history()` renvoie un `MutexGuard` ; tant que l'appelant le garde, personne d'autre n'accède à la base ; le verrou se libère automatiquement à la fin de l'expression.
- **Paramètre `impl FnOnce`** — `engine.rs:248` : `with_quarantine` prend une fermeture appelée une seule fois avec la quarantaine construite sur place ; la quarantaine emprunte des données locales (`root`) qui ne peuvent pas sortir de la fonction, d'où ce style « je te prête, tu rends ».
- **Alias de type** — `quarantine.rs:92` : `type FileEntry = (String, PathBuf, u64);` nomme un tuple utilisé partout et répond à l'alerte clippy `type_complexity`.
- **Filtrage sur des constantes** — `quarantine.rs:189` : `Err(native::ERROR_NOT_SAME_DEVICE) =>` compare le code d'erreur à une constante dans un motif de `match`.
- **Écriture atomique** — `quarantine.rs:254` : écrire dans un fichier temporaire puis le renommer garantit qu'un plantage ne laisse jamais un manifeste à moitié écrit.
- **`Option::is_none_or`** — `quarantine.rs:524` : « aucune liste, ou la liste contient l'élément » en une expression ; c'est ainsi que `item_ids: None` signifie « tout restaurer ».
- **Énumération étiquetée serde** — `registry.rs:124` : `#[serde(tag = "type", content = "data")]` écrit `{"type":"dword","data":7}`, lisible dans l'instantané JSON.
