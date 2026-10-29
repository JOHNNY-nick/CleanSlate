# J6 — Finition : plan

Dernière mise à jour : 29 oct. 2026

| # | Commit | Contenu |
| --- | --- | --- |
| 1 | `feat(logs)` | Journaux `tracing` dans `%LOCALAPPDATA%\CleanSlate\logs`, rotation quotidienne, 7 fichiers (ENF-10) ; initialisés une fois par `cs_engine_new` sur le vrai système. Nécessite `tracing-subscriber` (non listé : D39) |
| 2 | `feat(settings)` | Réglages complets : exclusions (EF-20, passées au moteur), thème (système / clair / sombre), dossier des journaux, « À propos » avec l'icône |
| 3 | `feat(home)` | Accueil : nombre d'applications, dernière analyse, espace récupérable, sessions récentes, bandeau simulation, « Analyser les restes » ; droits administrateur absents → message et « Redémarrer en administrateur » |
| 4 | `feat(theme)` | Accent sarcelle de la `NavigationView` et des contrôles, ressources `HighContrast`, annonce de la progression au Narrateur au plus toutes les 2 s |
| 5 | `build(installer)` | Publication autonome (.NET et Windows App SDK embarqués), script Inno Setup (`SetupIconFile`, `PrivilegesRequired=admin`), `scripts/package.ps1` |
| 6 | `docs` | Réévaluation Windows App SDK 2.x, revue finale, scénario des 5 applications, README final avec captures sans données personnelles (D17), bilan |
