# J2 — Inventaire des applications : plan

Dernière mise à jour : 29 oct. 2026

Exigences : EF-01, EF-02, EF-03, ENF-05 (inventaire < 2 s). Plan validé par le porteur le 29 oct. 2026, avec les réponses D11 à D15 de [decisions.md](../decisions.md).

| # | Commit | Contenu | Tests |
| --- | --- | --- | --- |
| 1 | `feat(inventory)` | `model.rs` : `HiddenReason`, `InstallerKind`, champs `hidden_reason` et `installer` ; `QuarantinedItem` complet (D11). `inventory.rs` : lecture seule de `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall` (vue 64 bits), `HKLM\SOFTWARE\WOW6432Node\…\Uninstall` (chemin explicite, aussi valable dans la ruche de test) et `HKCU\…\Uninstall`. Normalisation : `YYYYMMDD` → date, guillemets retirés, `EstimatedSize`. MSI = `WindowsInstaller = 1` ou sous-clé GUID. Masquage : `SystemComponent = 1` → `SystemComponent` ; `ParentKeyName`, `ReleaseType` (mise à jour, correctif…) ou nom « (KBnnnnnn) » → `Update`. Entrées sans nom écartées. Identifiant : FNV-1a 64 bits (vue + sous-clé en minuscules), sans dépendance. Sous-clé illisible : journalisée et ignorée (ENF-09). `Engine::list_apps` | Ruche factice : MSI, Inno `_is1`, NSIS, 32 bits, `InstallLocation` vide, composant système, mise à jour, sans nom, date invalide, stabilité de l'id, HKCU absente |
| 2 | `feat(ffi)` | `cs_list_apps(engine)` ; poignée nulle → `INVALID_INPUT` | Aller-retour JSON |
| 3 | `feat(cli)` | `cleanslate-cli apps [--json] [--all]`, durée sur la sortie d'erreur (ENF-05) | — |
| 4 | `feat(interop)` | Records `InstalledApp`, enums en snake_case, `DateOnly` ; `EngineClient.ListAppsAsync` | xUnit sur JSON d'exemple (pas de lecture du vrai registre en test) |
| 5 | `feat(presentation)` | `IEngineService.ListAppsAsync` ; `AppsViewModel` (recherche nom/éditeur sans casse ni accents, tri nom/taille/date, filtre système, sélection, initiales, commandes désinstaller désactivées) ; `Formats` fr-FR | NSubstitute |
| 6 | `feat(app)` | `AppsPage` conforme à la maquette, textes `.resw`, `EngineService.ListAppsAsync` | Capture |
| 7 | `docs` | Specs, README, bilan `J2-bilan.md` | — |

Condition de fin : `cleanslate-cli apps --json` en Release sur la machine du porteur en moins de 2 s ; fmt, clippy, tous les tests ; couverture du moteur ≥ 80 % ; build Release.
