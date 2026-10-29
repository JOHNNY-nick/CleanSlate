# J5 — Désinstallation complète : plan

Dernière mise à jour : 29 oct. 2026

Exigences : EF-12 à EF-15, ENF-07 (5 clics). **Aucune vraie désinstallation sur la machine du porteur** : tests avec un faux processus (et `hostname.exe`, inoffensif, pour vérifier l'attente du Job Object) ; procédure Windows Sandbox avec 7-Zip, VLC et Notepad++ dans `docs/tests-acceptation.md`.

| # | Commit | Contenu | Tests |
| --- | --- | --- | --- |
| 1 | `feat(uninstall)` ✔ | Découpage sûr (15 formes), `msiexec /x`, refus des shells, Job Object, délai 30 min, point de restauration | Unitaires + `hostname.exe` |
| 2 | `feat(scan)` | `scan_app_leftovers` (§ 6.4) : ancien dossier d'installation, nom exact ou similarité ≥ 0,85, clé au `ProductCode`, raccourcis vers l'ancien dossier (Élevée) ; dossier éditeur sans autre application (Moyenne) ; jamais un élément rattaché à une autre application installée ; refus si l'application est toujours installée | Fixture GIMP |
| 3 | `feat(engine)` + `feat(ffi)` | `Engine::uninstall` (empreinte gardée en mémoire, simulation par défaut, vérification de la disparition de l'entrée), `scan_app_leftovers` ; `cs_uninstall`, `cs_scan_app_leftovers` | Faux processus qui supprime l'entrée de la ruche de test |
| 4 | `feat(interop)` + `feat(presentation)` + `feat(app)` | `UninstallAsync`, `ScanAppLeftoversAsync` ; `INavigationService` ; assistant en 4 étapes ; boutons de la page Applications actifs (« Désinstallation classique » = EF-12 seul) | NSubstitute (5 clics) |
| 5 | `docs` | Procédure Sandbox, specs, bilan | — |
