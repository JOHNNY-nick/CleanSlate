# J3 — Détection des restes orphelins (simulation) : plan

Dernière mise à jour : 29 oct. 2026

Exigences : EF-05 à EF-11, ENF-01, ENF-03, ENF-04, ENF-05 (scan < 60 s), ENF-06, ENF-09. **Aucun déplacement ni suppression** : le scan est en lecture seule, le bouton d'action ne fait que simuler.

| # | Commit | Contenu | Tests |
| --- | --- | --- | --- |
| 1 | `feat(safety)` | `safety.rs` : `SafetyRules` (racines connues injectables), `validate_path` et `validate_key`. Refus : chemin relatif, composant `.` ou `..`, liste noire § 9.1 (chemins, racines de scan, noms de sous-dossiers, registre), exclusions utilisateur, reparse point sur le chemin ou un ancêtre, chemin canonisé différent du chemin lexical (`std::fs::canonicalize` = `GetFinalPathNameByHandleW`). `FileSystem::canonicalize` ajouté au trait | Un test par entrée de la liste noire et par racine de scan (générés par macro), jonction piégée vers `C:\Windows` dans un dossier temporaire, nom contenant `..`, exclusions |
| 2 | `feat(matcher)` | `matcher.rs` : normalisation (minuscules, accents, versions, architecture, suffixes juridiques, ponctuation), Jaro-Winkler (`strsim`), rattachement § 6.2, score § 6.3 avec `reasons` en français, regroupement par application déduite | Unitaires par règle et par signal |
| 3 | `feat(scan)` | `scan.rs` : racines EF-05 / EF-06 (`ScanRoots::from_system` via variables d'environnement et `SHGetKnownFolderPath`), candidats de profondeur 1 et 2 (sous un dossier éditeur d'une application installée), taille et date via le trait `FileSystem` sans traverser les reparse points, parallélisme `rayon`, annulation `AtomicBool`, progression limitée (D03), erreurs collectées (ENF-09). Raccourcis morts (EF-07) : lecture minimale du format `.lnk`. `Engine::scan_orphans` | Fixtures : 5 applications désinstallées et 5 installées (dont 2 du même éditeur), zéro élément d'une application installée, restes retrouvés ; annulation ; progression |
| 4 | `bench` | Criterion : scan de 500 000 fichiers factices (ENF-05) | `cargo bench` |
| 5 | `feat(ffi)` | `cs_scan_orphans(engine, opts, len, cb, ctx)`, `cs_cancel(engine)`, erreur `BUSY` | Rust |
| 6 | `feat(cli)` | `cleanslate-cli scan --dry-run [--json]` (refusé sans `--dry-run`) | — |
| 7 | `feat(interop)` | Records `ScanReport`…, `ScanOrphansAsync(ScanOptions, IProgress<ScanProgress>, CancellationToken)` avec rappel `[UnmanagedCallersOnly]` et `GCHandle` | xUnit |
| 8 | `feat(presentation)` + `feat(app)` | `OrphansViewModel` (scan, annulation, groupes, cases précochées si « Élevée », filtre de confiance, total sélectionné, « Simuler ») ; `OrphansPage` conforme à la maquette | NSubstitute ; capture |
| 9 | `docs` | Specs, bilan, décisions | — |

Condition de fin : tests verts, benchmark < 60 s, scan en lecture seule de la machine du porteur via `cleanslate-cli scan --dry-run`, couverture ≥ 80 %, build Release.

## Avancement

- Étapes 1 à 6 faites (safety, matcher, scan, benchmark 554 ms, FFI `cs_scan_orphans` / `cs_cancel`, CLI `scan --dry-run`).
- Scan réel en lecture seule de la machine du porteur : 7,6 s, 429 éléments, 3 erreurs d'accès (non élevé).
- Reste : étape 7 (Interop), 8 (Presentation + page), 9 (docs, bilan).
