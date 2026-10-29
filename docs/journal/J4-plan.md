# J4 — Quarantaine, restauration et historique : plan

Dernière mise à jour : 29 oct. 2026

Exigences : EF-16 à EF-19, EF-20 (historique), ENF-02, ENF-04, ENF-09. Premières opérations destructives : **testées uniquement dans des dossiers temporaires (sous `core/target`, car `%TEMP%` contient le nom protégé « Temp ») et sous `HKCU\Software\CleanSlateTest\<uuid>`**. Aucune vraie mise en quarantaine sur la machine du porteur ; procédure Windows Sandbox dans `docs/tests-acceptation.md`.

| # | Commit | Contenu | Tests |
| --- | --- | --- | --- |
| 1 | `feat(registry)` | Écriture et suppression d'arbres de clés dans le trait `Registry` (instantané JSON + export `.reg` UTF-16 LE) | Zone de test HKCU |
| 2 | `feat(quarantine)` | `quarantine.rs` : SHA-256 BCrypt en flux, SDDL d'origine, manifeste écrit avant chaque déplacement (`pending`) et après, `MoveFileExW` même volume sinon copie vérifiée, refus des dossiers contenant un reparse point, verrouillé → `MOVEFILE_DELAY_UNTIL_REBOOT`, plafond 20 Go / 50 000 fichiers (saisie « QUARANTAINE »), restauration totale / partielle (refus si la destination existe), purge par âge limitée au dossier de quarantaine, reprise après interruption | Cycle quarantaine → restauration à l'identique (contenu, dates, clés), reprise, plafond, destination existante, purge |
| 3 | `feat(history)` | `history.rs` : SQLite (`rusqlite` bundled), tables `sessions`, `items`, `exclusions`, `settings` | Base temporaire |
| 4 | `feat(engine)` + `feat(ffi)` | Dernier rapport gardé en mémoire ; `cs_clean` (simulation si `EngineConfig.dry_run` **ou** `CleanPlan.dry_run`), `cs_restore`, `cs_list_sessions`, `cs_purge_quarantine` ; purge automatique au démarrage | Rust + FFI |
| 5 | `feat(interop)` + `feat(presentation)` + `feat(app)` | `CleanAsync`, `RestoreAsync`, `ListSessionsAsync`, `PurgeAsync` ; `ISettingsService` (simulation activée par défaut), `IDialogService` ; dialogue de confirmation, page Historique, réglage « Mode simulation » ; bouton « Mettre en quarantaine » actif | NSubstitute ; capture en simulation |
| 6 | `docs` | Specs, procédure Windows Sandbox, bilan | — |

## Avancement

- Étapes 1 à 3 faites : registre en écriture (instantané, .reg), `quarantine.rs` (8 tests : cycle à l'identique, partiel, destination existante, éléments protégés et liens, plafond, reprise, purge, simulation), `history.rs`.
- Reste : étape 4 (Engine + FFI), 5 (Interop, Presentation, App), 6 (docs, procédure Sandbox, bilan).
