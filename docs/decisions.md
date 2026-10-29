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
