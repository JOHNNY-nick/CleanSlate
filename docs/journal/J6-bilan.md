# J6 — Finition : bilan

Dernière mise à jour : 30 oct. 2026

## Ce qui est fait

| Exigence | Réalisation | Fichiers | Tests |
| --- | --- | --- | --- |
| ENF-10 | Journaux `tracing` dans `%LOCALAPPDATA%\CleanSlate\logs\cleanslate.AAAA-MM-JJ.log`, rotation quotidienne, 7 fichiers ; installés une seule fois, sur le vrai système uniquement (jamais par un moteur de test) | `logging.rs`, `engine.rs::with_system` | `daily_files_in_the_given_folder` |
| EF-20 | Réglages : exclusions (dossiers ou clés, transmises au moteur), durée de quarantaine, thème, dossier des journaux, « À propos » avec l'icône | `ISettingsService`, `SettingsService.cs` (`settings.json`, D40), `SettingsViewModel.cs`, `SettingsPage.xaml`, `EngineService.cs` | `Settings_ExclusionsAndTheme`, `Settings_ClampDaysAndWriteThrough`, `user_exclusions_protect_the_folder_and_its_content` |
| EF-20, ENF-04 | Accueil : applications visibles, restes détectés et date de la dernière analyse (D44), espace libéré, 3 dernières sessions, bandeau simulation, « Analyser les restes » ; avertissement et « Redémarrer en administrateur » sans droits | `HomeViewModel.cs`, `HomePage.xaml`, `App.xaml.cs` (`IsElevated`, `RestartElevated`) | `Load_CountsVisibleApps_KeepsThreeSessions_AndSumsFreedSpace`, `AnalyzeLeftovers_OpensOrphansAndStartsTheScan`, `NotElevated_ShowsTheElevationWarning` |
| ENF-08 | Thème système, clair ou sombre ; accent sarcelle (`SystemAccentColor*`) ; dictionnaire `HighContrast` ; progression annoncée au Narrateur au plus toutes les 2 s (`LiveRegionChanged`) | `App.xaml`, `MainWindow.xaml.cs` (`ApplyTheme`), `OrphansViewModel.cs`, `OrphansPage.xaml.cs` | captures en sombre ; Narrateur et Accessibility Insights à passer |
| ENF-07, § 10.4 | Installeur Inno Setup : publication autonome (.NET 8 et Windows App SDK embarqués), `SetupIconFile`, `PrivilegesRequired=admin`, assistant en français, données gardées à la désinstallation (D43) | `installer/cleanslate.iss`, `scripts/package.ps1`, `CleanSlate.App.csproj` (`PublishXamlResources`, D41) | publication lancée et capturée ; installation : Sandbox § 5 |

Aussi : réévaluation de Windows App SDK 2.x (D42 : on reste en 1.8), scénario des 5 applications ([tests-acceptation.md § 4](../tests-acceptation.md)), [revue finale](revue-finale.md), README final avec captures (aucune liste de logiciels installés, D17).

Corrections en cours de jalon :

- **Blocage au démarrage** : `MainWindow` → `HomePage` → `HomeViewModel` → `NavigationService` → `MainWindow` formait un cycle d'injection ; le conteneur attendait indéfiniment. `NavigationService` et `DialogService` résolvent maintenant la fenêtre au moment de l'utiliser.
- **Publication** : `dotnet publish` oubliait les `.xbf` et `CleanSlate.App.pri` d'une application WinUI non packagée ; l'application plantait dans `Microsoft.UI.Xaml.dll` (0xC000027B). La cible `PublishXamlResources` les copie (D41).

Chiffres : 165 tests du moteur + 14 FFI, 62 tests C# ; couverture du moteur **92,8 %** ; `cargo fmt` et `cargo clippy -D warnings` propres ; build Release réussi ; installeur `artifacts\installer\CleanSlate-0.1.0-setup.exe` (58,8 Mo). La zone de test `HKCU\Software\CleanSlateTest` est vide après les tests.

**Aucune opération destructive sur la machine du porteur** : toutes les vérifications de l'application ont été faites en mode simulation, et l'installeur n'a pas été exécuté ici (il installe dans `Program Files`) ; procédure dans Windows Sandbox.

## Comment tester

- Automatique : `cargo test --manifest-path core/Cargo.toml --workspace` ; `dotnet test app/CleanSlate.sln -c Release -p:Platform=x64`.
- Installeur : `pwsh scripts/package.ps1`, puis [tests-acceptation.md § 5](../tests-acceptation.md) dans Windows Sandbox.
- Application : Réglages → changer le thème (effet immédiat) ; ajouter une exclusion `C:\Jeux\Sauvegardes` puis relancer l'analyse ; Accueil → « Analyser les restes » ouvre les Restes orphelins et lance l'analyse.
- Contraste élevé : Paramètres Windows → Accessibilité → Thèmes de contraste, puis rouvrir CleanSlate.
- Narrateur (Ctrl + Win + Entrée) : lancer une analyse, la progression est lue au plus toutes les 2 s.

## Écarts avec les specs et points d'attention

- **Réglages dans `settings.json`** et non dans les tables SQLite `settings` et `exclusions` (D40).
- **Dernière analyse** gardée en mémoire seulement (D44).
- **ENF-08** : Narrateur et Accessibility Insights non passés de bout en bout ; à faire.
- **Windows App SDK** : reste en 1.8 (D42).
- **Installeur non signé** : avertissement SmartScreen.
- **Inno Setup** installé dans le profil utilisateur (`winget … --scope user`), sans droits administrateur.

## Notions Rust du jalon

- **`OnceLock`** — `logging.rs:15` et `logging.rs:32` : une valeur `static` initialisée au plus une fois, même si plusieurs threads appellent `init` en même temps. `get_or_init` exécute la fermeture la première fois seulement ; les appels suivants ne font rien. Un abonné `tracing` global ne peut être installé qu'une fois par processus.
- **Ignorer une erreur volontairement** — `logging.rs:34` : `let _ = ….try_init();` jette le `Result` de façon explicite. Si un autre abonné est déjà installé, on continue sans journal fichier : les journaux ne doivent jamais arrêter le moteur. Sans `let _`, le compilateur signalerait un `Result` non utilisé.
- **`if let Ok(…)`** — `logging.rs:33` : traite seulement le cas de succès d'un `Result`, sans `match` complet ; si le dossier des journaux ne peut pas être créé, on passe.
- **`map_err` puis `?`** — `logging.rs:19` : `map_err` transforme l'erreur d'entrée-sortie en `CoreError` (avec le chemin concerné), puis `?` renvoie cette erreur à l'appelant si elle survient. C'est la forme courte de « en cas d'erreur, convertis-la et sors de la fonction ».
- **Patron « builder »** — `logging.rs:20` à `logging.rs:25` : `RollingFileAppender::builder()` puis une chaîne d'appels (`rotation`, `filename_prefix`, `max_log_files`) qui renvoient chacun le constructeur modifié, et enfin `build` qui produit l'objet ou une erreur. Chaque réglage reste lisible et optionnel.
