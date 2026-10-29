# J3 — Détection des restes orphelins (simulation) : bilan

Dernière mise à jour : 29 oct. 2026

## Ce qui est fait

| Exigence | Réalisation | Fichiers | Tests |
| --- | --- | --- | --- |
| ENF-01 | Liste noire § 9.1 complète, racines de scan protégées (D06), exclusions, `validate_path` / `validate_key` | `safety.rs` | un test par entrée (macros), 70 cas |
| ENF-03 | Aucun reparse point traversé ni proposé ; chemin canonique comparé au chemin lexical | `safety.rs`, `scan.rs`, `fs.rs` | `trapped_junction_to_windows_is_refused` (vraie jonction vers `C:\Windows` dans un dossier temporaire) |
| EF-05, EF-06 | Racines dossiers (profondeur 1, et 2 sous un éditeur installé) et registre | `scan.rs`, `safety.rs` (`KnownFolders`) | `fixture_finds_leftovers_and_never_installed_apps` |
| EF-07 (S) | Raccourcis morts des Bureaux (lecture du format `.lnk`) ; menu Démarrer écarté par la liste noire, tâches et services non faits (D20) | `scan.rs` | `shortcut_parsing`, fixture |
| EF-08 | Rattachement § 6.2 (+ programmes présents D22, dossier contenant une installation D25), exécutables des services et processus | `matcher.rs`, `system.rs` | tests du matcher, `service_image_paths_are_parsed` |
| EF-09 | Regroupement par application déduite | `matcher.rs::group` | `grouping_by_normalized_name_largest_first` |
| EF-10 | Score § 6.3, raisons en français, plafonds D21/D23, précochage « Élevée » seulement | `matcher.rs`, `OrphansViewModel` | tests du score, `Scan_PrechecksOnlyHighConfidence` |
| EF-11 | Progression limitée à un événement par 250 ms (et au moins un tant que le travail avance), annulation avec résultat partiel | `scan.rs` (`Reporter`), `cs_cancel`, `ScanOrphansAsync` | `reporter_throttles_to_250_ms`, `cancelled_scan_returns_partial_flagged_report`, `CancelledScan_KeepsPartialResults` |
| ENF-04 | Le scan est en lecture seule ; « Simuler » n'agit pas ; CLI refusée sans `--dry-run` | `OrphansViewModel`, `cleanslate-cli` | `Simulate_ReportsWithoutTouchingAnything` |
| ENF-05 | Benchmark Criterion : **554 ms** pour 500 000 fichiers en mémoire ; scan réel de la machine : **7,6 s** | `benches/scan.rs` | `cargo bench` |
| ENF-09 | Erreurs d'accès collectées (500 au plus), jamais bloquantes | `scan.rs` (`Errors`) | scan réel : 3 éléments ignorés |

Chiffres : 137 tests core + 12 FFI, 42 tests C#, couverture du moteur **92,9 %**, fmt et clippy propres, build Release réussi.

Scan réel en lecture seule (non élevé) de la machine du porteur : 343 groupes, 429 éléments (97 Élevée, 155 Moyenne, 177 Faible), 59 Go en tout, 3 accès refusés. Le plus gros groupe (« Virtual Machines », 27 Go, dans Documents) reste Faible.

## Comment tester

```powershell
pwsh scripts/build.ps1 -Configuration Release
cargo run --manifest-path core/Cargo.toml --release -p cleanslate-cli -- scan --dry-run          # lecture seule
cargo bench --manifest-path core/Cargo.toml -p cleanslate-core --bench scan
```

Dans l'application : « Restes orphelins » → « Analyser les restes » ; observer la progression, annuler, filtrer par confiance, cocher / décocher, « Simuler ». « Mettre en quarantaine » reste désactivé jusqu'à J4.

## Écarts avec les specs et points d'attention

- **EF-07 partiel (D20)** : raccourcis du menu Démarrer non détectés tant que la liste noire protège `Microsoft` ; tâches planifiées et services non détectés. Question au porteur : autoriser une exception limitée aux `.lnk` morts ?
- **Historique (+30)** : signal reporté à J4, quand l'historique existe.
- **Fixtures** : générées en mémoire dans les tests (`scan::tests::fixture`) plutôt que dans `tests/fixtures/`.
- **Benchmark** : il mesure le moteur sur une arborescence en mémoire ; le disque réel ajoute ses lectures (7,6 s mesurées sur la machine du porteur).
- **Faux positifs** : le premier scan réel a montré des données d'applications présentes mais non inscrites ; corrigé par D22 et D23. Les applications portables lancées depuis un autre dossier (ex. Ghidra) restent proposées : la quarantaine reste le filet de sécurité.

## Notions Rust du jalon

- **`AtomicBool` partagé par référence** — `scan.rs:50` : le scan lit `cancel` pendant que `cs_cancel` l'écrit depuis un autre thread. Un atomique se modifie sans verrou ; `&AtomicBool` suffit (pas besoin d'`Arc`) parce que le moteur vit plus longtemps que le scan.
- **`compare_exchange`** — `engine.rs:156` : passe `busy` de `false` à `true` en une seule opération indivisible ; si un autre thread l'avait déjà mis à `true`, l'appel échoue et on renvoie `BUSY`.
- **Garde avec `Drop`** — `engine.rs:49` : `BusyGuard` remet `busy` à `false` quand il sort de portée, même si le scan panique ; c'est le motif RAII.
- **Objet fonction `dyn Fn + Sync`** — `scan.rs:56` : le rappel de progression est une fermeture que plusieurs threads peuvent appeler ; `Sync` le garantit au compilateur.
- **`Mutex`** — `scan.rs:125` : la liste des erreurs est partagée entre les threads de `rayon` ; `lock()` donne un accès exclusif.
- **Parallélisme `rayon`** — `scan.rs:183` : `par_iter()` remplace `iter()` et répartit l'analyse des candidats sur tous les cœurs ; les règles d'emprunt garantissent qu'aucune donnée n'est modifiée en même temps sans protection.
- **Durées de vie sur une structure** — `scan.rs:88` : `Reporter<'a>` garde une référence au rappel ; `'a` dit au compilateur que le `Reporter` ne doit pas vivre plus longtemps que ce rappel.
- **Macros déclaratives** — `safety.rs:404` : `macro_rules!` génère une fonction de test par entrée de la liste noire, ce qui répond à « un test par entrée » sans copier-coller.
- **`unsafe impl Send/Sync`** — `cleanslate-ffi/src/lib.rs:221` : on promet au compilateur qu'un pointeur brut peut passer d'un thread à l'autre, parce que Rust ne fait que le transmettre ; la méthode `get` (ligne 228) évite que la fermeture capture le champ brut (règle de capture précise de l'édition 2021).
- **Appels Win32 `unsafe`** — `system.rs:53` et `:82` : énumération des processus ; chaque bloc ferme ses poignées et documente ses hypothèses dans `// SAFETY:`.
