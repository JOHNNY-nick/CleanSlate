# J2 — Inventaire des applications : bilan

Dernière mise à jour : 29 oct. 2026

## Ce qui est fait

| Exigence | Réalisation | Fichiers | Tests |
| --- | --- | --- | --- |
| EF-01 | Lecture seule des trois clés Uninstall (HKLM 64 bits, `WOW6432Node` par chemin explicite, HKCU) ; normalisation en `InstalledApp` ; id stable FNV-1a | `core/cleanslate-core/src/inventory.rs`, `engine.rs`, `cleanslate-ffi/src/lib.rs` (`cs_list_apps`), `EngineClient.ListAppsAsync` | `inventory::tests::*` (ruche factice : Inno Setup, MSI, NSIS, 32 bits, HKCU, `InstallLocation` vide), `InstalledAppJsonTests` |
| EF-02 | Composants système et mises à jour marqués par `hidden_reason`, filtrés par le ViewModel ; entrées sans nom écartées | `inventory.rs`, `AppsViewModel.cs` | `hidden_entries_are_flagged_not_dropped`, `ShowSystemComponents_RevealsHiddenEntries` |
| EF-03 | Recherche nom/éditeur (sans casse ni accents), tri nom / taille / date | `AppsViewModel.cs`, `AppsPage.xaml` | `AppsViewModelTests` |
| ENF-05 | Inventaire réel : **292 applications en 25 à 68 ms** (Release, machine du porteur, `cleanslate-cli apps`) | `cleanslate-cli/src/main.rs` | mesure manuelle |

Autres livrables : logo et icônes (commit `feat(branding)`), `QuarantinedItem` complet dans le modèle (D11), CLI `apps [--json] [--all]`, formats fr-FR (`Formats.cs`), boutons d'accent en sarcelle.

Chiffres de fin de jalon : 49 tests Rust, 31 tests C#, couverture du moteur **91,8 %** des lignes (`inventory.rs` 98,9 %), fmt et clippy sans avertissement, build Release réussi.

## Comment tester

Depuis la racine du dépôt :

```powershell
pwsh scripts/build.ps1 -Configuration Release
cargo run --manifest-path core/Cargo.toml --release -p cleanslate-cli -- apps          # liste, durée sur stderr
cargo run --manifest-path core/Cargo.toml --release -p cleanslate-cli -- apps --json --all
dotnet test app/CleanSlate.sln -c Release -p:Platform=x64
```

Puis lancer `CleanSlate.App.exe` (Release) et ouvrir « Applications » : recherche, tri, case « Afficher les composants système », sélection d'une ligne (barre d'action en bas, boutons désactivés).

## Écarts avec les specs et points d'attention

- **MSI** : une sous-clé GUID ne suffit pas à conclure au MSI ; il faut aussi `msiexec` dans `UninstallString` ou `WindowsInstaller = 1` (D16). Évite qu'en J5 un `msiexec /x` soit lancé sur un installeur EXE.
- **Type d'installeur** : déduction au mieux (`_is1`, `unins000.exe` → Inno Setup ; `uninst.exe` / `uninstall.exe` → NSIS), affichée seulement.
- **Monogrammes** : deux premiers caractères alphanumériques (« No » pour Notepad++ là où la maquette montre « N+ ») — décoratif.
- **Tests** : aucun test ne lit le vrai registre (règle 2) ; la mesure ENF-05 est faite à la main via la CLI.
- **Build Debug** : une instance de `CleanSlate.App.exe` lancée en administrateur (PID 17976) verrouille `bin\x64\Debug` et ne peut pas être arrêtée sans élévation ; les vérifications se font en Release.
- **`.gitignore`** : la ligne `.reference/` ajoutée hors de cette session a été incluse dans le commit `feat(inventory)`.
- **Captures** : la page affiche la vraie liste des logiciels de la machine ; aucune capture de ce type n'est publiée dans le dépôt (D17).

## Notions Rust du jalon

- **Fermetures (closures)** — `inventory.rs:87` : `let get = |name: &str| { … }` crée une petite fonction anonyme qui *capture* la variable `values` par emprunt. `text` et `number` s'appuient dessus, ce qui évite de répéter la recherche de valeur.
- **`?` sur une `Option`** — `inventory.rs:102` : `text("DisplayName")?` renvoie immédiatement `None` si l'entrée n'a pas de nom, comme `?` renvoie une erreur sur un `Result`. C'est ainsi que les entrées sans nom sont écartées.
- **`bool::then`** — `inventory.rs:111` : `(condition).then(|| valeur)` donne `Some(valeur)` si la condition est vraie, sinon `None` ; plus lisible qu'un `if … { Some(…) } else { None }`.
- **`fold` et arithmétique enveloppante** — `inventory.rs:161` : `fold` parcourt les octets en accumulant le hachage ; `wrapping_mul` multiplie modulo 2⁶⁴ au lieu de paniquer en cas de dépassement (en Debug, un dépassement normal provoque une panique).
- **`let … else`** — `inventory.rs:176` : déstructure une `Option` et sort de la fonction (`return false`) si elle est vide, sans imbriquer de `match`.
- **Motifs « ou » dans un `match`** — `inventory.rs:225` : `Some("uninst.exe" | "uninstall.exe")` filtre plusieurs valeurs de chaîne dans une seule branche.
- **Objet trait `&dyn Registry`** — `inventory.rs:55` : la fonction accepte n'importe quel registre (réel ou de test) ; l'appel de méthode est résolu à l'exécution.
- **`Iterator` dans `extend`** — `inventory.rs:69` : `apps.extend(parse_entry(…))` fonctionne parce qu'une `Option` est aussi un itérateur de 0 ou 1 élément.
- **Pointeur brut vers référence** — `cleanslate-ffi/src/lib.rs:188-190` : `engine.as_ref()` transforme un `*const EngineHandle` en `Option<&EngineHandle>` (`None` si nul) ; c'est `unsafe` car Rust ne peut pas vérifier que le pointeur est encore valide, d'où le commentaire `// SAFETY:`.
