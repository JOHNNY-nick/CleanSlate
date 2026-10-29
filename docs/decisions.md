# Journal des décisions — CleanSlate

Dernière mise à jour : 29 oct. 2026

Chaque décision de fond prise pendant le développement, avec sa raison. « Porteur » : décision du porteur du projet ; « Claude » : décision prise en autonomie selon la règle « option la plus prudente et la plus simple », à relire.

| # | Date | Jalon | Question | Options | Choix | Raison | Décideur |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D01 | 29 oct. 2026 | J1 | Application packagée ou non ? | Packagée ; non packagée | Non packagée, `requireAdministrator`, Inno Setup | Un paquet ne peut pas demander l'élévation | Porteur |
| D02 | 29 oct. 2026 | J1 | Nom de l'assembly de l'application | `CleanSlate` ; `CleanSlate.App` | `CleanSlate.App` | `CleanSlate.dll` écraserait `cleanslate.dll` (casse ignorée par Windows) | Claude, validé |
| D03 | 29 oct. 2026 | Passe doc | Fréquence de progression (ENF-06) | « au moins » ; « au plus » | Jamais plus d'un événement par 250 ms, au moins un toutes les 250 ms tant que le travail avance | Lever la contradiction cahier / specs | Porteur |
| D04 | 29 oct. 2026 | Passe doc | Rotation des journaux (ENF-10) | 10 Mo (nouvelle crate) ; quotidienne | Quotidienne, 7 fichiers, `tracing-appender` | Pas de nouvelle dépendance | Porteur |
| D05 | 29 oct. 2026 | Passe doc | Mot de confirmation renforcée | « SUPPRIMER » ; « QUARANTAINE » | « QUARANTAINE » | Vocabulaire fixe du design | Porteur |
| D06 | 29 oct. 2026 | Passe doc | Racines de scan dans la liste noire | Non ; oui | Oui, chacune, un test par racine | `%LOCALAPPDATA%\Programs` pouvait être proposé en bloc | Porteur |
| D07 | 29 oct. 2026 | Passe doc | Erreur de `cs_engine_new` | `cs_engine_new_ex` ; paramètre `out_error` | `out_error` | API plus simple | Porteur |
| D08 | 29 oct. 2026 | Passe doc | Tester les ViewModels | Référencer l'exe WinUI ; projet séparé | `CleanSlate.Presentation` (net8.0) | Tests sans interface graphique | Porteur |
| D09 | 29 oct. 2026 | Passe doc | Cible de `CleanSlate.Interop` | `net8.0-windows…` ; `net8.0` | `net8.0` | Doit être référencé par Presentation (net8.0) | Claude |
| D10 | 29 oct. 2026 | Passe doc | EF-04, Windows App SDK, mocks | — | EF-04 en v1.1 ; WinAppSDK 1.8 jusqu'à J6 ; NSubstitute | — | Porteur |
| D11 | 29 oct. 2026 | J2 | `QuarantinedItem` | Type minimal ; type complet | Type complet (§ 5), SHA-256 via BCrypt (crate `windows`) | Pas de crate `sha2` | Porteur |
| D12 | 29 oct. 2026 | J2 | Entrées masquées et type d'installeur | Filtrer dans le moteur ; champ dans le modèle | `hidden_reason` et `installer` dans `InstalledApp`, filtre côté ViewModel | Bascule instantanée de la case à cocher | Porteur |
| D13 | 29 oct. 2026 | J2 | Format des dates | « 12/03/2025 » ; « 22 oct. 2026 » | Format du design partout | Cohérence | Porteur |
| D14 | 29 oct. 2026 | J2 | Bouton « Désinstallation classique » | Retirer ; garder | Visible désactivé jusqu'à J5, puis désinstalleur officiel seul | Maquette | Porteur |
| D15 | 29 oct. 2026 | J2 | `ILocalizationService` | Garder ; retirer | Retiré : `.resw` dans l'App, formats fr-FR dans Presentation | Simplicité | Porteur |
| D16 | 29 oct. 2026 | J2 | Quand une entrée est-elle MSI ? | Sous-clé GUID seule ; GUID + msiexec, ou `WindowsInstaller = 1` | GUID + msiexec, ou `WindowsInstaller = 1` | Ne jamais lancer `msiexec /x` sur un installeur EXE (J5) | Claude |
| D17 | 29 oct. 2026 | J2 | Captures d'écran publiées | Capture de la machine réelle ; captures sans données personnelles | Aucune capture montrant les logiciels réellement installés ; seulement des pages sans données personnelles ou des données de test | Le dépôt est public | Claude |
| D18 | 29 oct. 2026 | J3 | Reparse point candidat (specs § 9.2 : « l'élément lui-même peut être traité ») | Le proposer ; l'ignorer | Jamais proposé ni traversé ; `validate_path` refuse tout chemin passant par un reparse point | Plus prudent ; un lien seul ne libère pas d'espace | Claude |
| D19 | 29 oct. 2026 | J3 | Parcours récursif | `walkdir` ; trait `FileSystem` | Trait `FileSystem` (`walkdir` non ajouté) | Testable avec `FakeFileSystem`, une dépendance en moins | Claude |
| D20 | 29 oct. 2026 | J3 | EF-07 face à la liste noire (« Microsoft » protège `…\Microsoft\Windows\Start Menu`) | Exception pour les `.lnk` ; liste noire inchangée | Liste noire inchangée : seuls les raccourcis des Bureaux sont détectés ; tâches planifiées et services non détectés (EF-07 est S) | Une exception affaiblirait une règle de sécurité : à décider par le porteur | Claude, **à valider** |
| D21 | 29 oct. 2026 | J3 | Dossiers de `Documents` (racine EF-05) | Score normal ; toujours Faible | Toujours Faible, jamais précochés | Documents personnels sans application associée (« Thèse ») | Claude |
| D22 | 29 oct. 2026 | J3 | Programmes présents sans entrée Uninstall (installations par utilisateur, portables) | Ignorer ; ajouter leur nom à l'ensemble connu | Dossier de Program Files, Program Files (x86) ou `Local\Programs` contenant un `.exe` : son nom rattache les données | Faux positifs observés au premier scan réel | Claude |
| D23 | 29 oct. 2026 | J3 | Sous-dossiers d'un éditeur installé (`NVIDIA\NGX`, `JetBrains\Daemon`) | Score normal ; plafond | Au plus Moyenne | Composants souvent partagés par l'éditeur | Claude |
| D24 | 29 oct. 2026 | J3 | Clé `HKCU\Software\CleanSlateTest` | Candidat normal ; protégée | Ajoutée aux noms de clés protégés | Zone de test (règle 2) | Claude |
| D25 | 29 oct. 2026 | J3 | Candidat qui contient un dossier d'installation (`Program Files\Vendor` ⊃ `Vendor\Tool`) | Orphelin ; rattaché | Rattaché | Prudence : il contient une application installée | Claude |
| D26 | 29 oct. 2026 | J3 | Annulation côté C# | Exception ; rapport partiel | Rapport partiel `Cancelled = true`, sans exception | Specs § 4.2 (« résultat partiel ») | Claude |
| D27 | 29 oct. 2026 | J4 | Quand agir pour de vrai ? | Requête seule ; moteur seul ; les deux | Seulement si `EngineConfig.dry_run` **et** la requête (`CleanPlan`, `RestoreRequest`, `PurgeRequest`) valent `false` | Double verrou, simulation par défaut partout (règle 5) | Claude |
| D28 | 29 oct. 2026 | J4 | Dossier de données d'un moteur de test (`Engine::new`) | `%LOCALAPPDATA%` ; aucun | Aucun : historique en mémoire, quarantaine réelle refusée | Les tests n'écrivent jamais hors de leurs dossiers temporaires | Claude |
| D29 | 29 oct. 2026 | J4 | Dossier contenant un lien ou une jonction | Le déplacer ; le refuser | Refusé (`CONTAINS_LINK`), laissé en place | Une copie inter-volumes perdrait le lien ; prudence | Claude |
| D30 | 29 oct. 2026 | J4 | Restauration d'une clé | Rejouer le `.reg` avec `reg.exe` ; instantané JSON | Instantané JSON (types exacts) ; le `.reg` reste pour une restauration manuelle | Pas de lancement de processus, restauration testable | Claude |
| D31 | 29 oct. 2026 | J4 | ACL d'origine | Réappliquer ; enregistrer seulement | Enregistrée (SDDL) ; un déplacement sur le même volume la conserve ; une copie inter-volumes prend celle de la destination | Simplicité ; écart à revoir si besoin | Claude, **à valider** |
| D32 | 29 oct. 2026 | J4 | Changement du mode simulation | Moteur inchangé ; moteur recréé | Moteur recréé avec le nouveau `dry_run` ; il faut relancer l'analyse | `EngineConfig` est fixé à la création | Claude |
| D33 | 29 oct. 2026 | J4 | Tests sur disque | `%TEMP%` ; `core/target` | Dossiers temporaires sous `core/target` | `%TEMP%` contient le nom protégé « Temp » | Claude |
