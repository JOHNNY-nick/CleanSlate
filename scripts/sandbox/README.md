# Tests dans Windows Sandbox

Les actions réelles de CleanSlate (quarantaine, désinstallation) ne se testent **que** dans Windows Sandbox : une machine jetable, effacée à sa fermeture (CLAUDE.md, règle 1 ; D53). Ce dossier contient tout ce qu'il faut pour le faire, et ce que chaque fichier fait.

## Fichiers

| Fichier | Où il tourne | Rôle |
| --- | --- | --- |
| `prepare.ps1` | hôte (pwsh 7) | Vérifie la compilation Release, télécharge et vérifie les installeurs, compile la CLI de test (Complements seulement), écrit le jeton et le `.wsb`, lance le Sandbox avec `-Launch`. N'installe rien et ne lance rien de CleanSlate sur l'hôte. |
| `manifeste.json` | hôte et Sandbox | Tout ce qui est téléchargé ou installé : nom, version, éditeur, URL officielle, SHA-256, commande d'installation silencieuse ; ce qui est construit dans le Sandbox ; ce que l'hôte partage. |
| `common.ps1` | Sandbox (PowerShell 5.1) | Garde, journal, étapes horodatées, inventaires avant et après chaque étape avec un fichier de différences, vérification des SHA-256, `rapport.md` et `rapport.html`. |
| `inside-complements.ps1` | Sandbox | Scénario **Complements** (tests-acceptation.md § 6). |
| `inside-apps.ps1` | Sandbox | Scénario **5 applications** (§ 4, cahier § 6.3). |
| `inside-installer.ps1` | Sandbox | Scénario **Installer** (§ 5). |

Produits par `prepare.ps1`, sous `artifacts\sandbox\` (ignoré par Git) :

| Chemin | Contenu |
| --- | --- |
| `installers\<id>\<fichier>` | installeurs du manifeste, téléchargés depuis leur URL officielle ; un SHA-256 différent arrête `prepare.ps1` |
| `tools\cleanslate-cli.exe` | la CLI publiée (contrôlée par `check-release-cli.ps1`), pour l'analyse en lecture seule |
| `tools\cleanslate-cli-sandbox.exe` | la CLI compilée avec la feature `sandbox-tests` (scénario Complements seulement ; supprimée pour les autres) |
| `run\token.txt` | jeton aléatoire de 32 octets, nouveau à chaque préparation |
| `run\prepare.json` | date, commit, SHA-256 de la CLI, de l'application, de l'installeur et des installeurs partagés |
| `CleanSlate-<Scénario>.wsb` | configuration du Sandbox |
| `results\<scénario>-<date>\` | **seul dossier écrit par le Sandbox** : `rapport.html`, `rapport.md`, `journal.txt`, `inventaires\`, `diffs\`, sorties de la CLI |

## Ce que voit le Sandbox

Le `.wsb` partage :

- le dépôt entier, **en lecture seule**, à `C:\CleanSlate\repo` (scripts, application compilée, installeurs, outils, jeton) ;
- `artifacts\sandbox\results`, **seul dossier en écriture**, à `C:\CleanSlate\results` ;
- pour Complements seulement, le dossier `bin\…\x64` du Windows SDK (`makeappx`, `signtool`), en lecture seule, à `C:\CleanSlate\sdk`.

Le réseau est **coupé** (`<Networking>Disable</Networking>`) : tout ce qui est installé vient des installeurs vérifiés. La commande d'ouverture de session pose `CLEANSLATE_SANDBOX_RUN=<jeton>` puis lance `inside-<scénario>.ps1` ; la console reste ouverte (`-NoExit`).

## La garde (D53)

Les commandes de test (`sandbox quarantine | restore | uninstall`) n'existent que dans `cleanslate-cli-sandbox.exe`. Elles s'exécutent seulement si les **trois** conditions sont vraies, chacune d'une source différente :

1. le compte est `WDAGUtilityAccount`, le compte fixe de Windows Sandbox ;
2. la variable `CLEANSLATE_SANDBOX_RUN` vaut 64 chiffres hexadécimaux : seul le `.wsb` la pose ;
3. le même jeton est dans `C:\CleanSlate\repo\artifacts\sandbox\run\token.txt` : ce chemin n'existe que dans le Sandbox, par le partage.

Sinon la CLI sort avec le code 3 sans rien toucher ; les scripts `inside-*.ps1` vérifient les mêmes conditions (`Assert-Sandbox`) et sortent avec le code 1.

**Limites.** La garde arrête une erreur, pas une personne décidée : un administrateur local peut créer un compte `WDAGUtilityAccount`, poser la variable et le fichier. Elle n'est pas une frontière de sécurité ; la vraie protection est que la version publiée ne contient pas ces commandes (feature `sandbox-tests`, contrôle `scripts/check-release-cli.ps1` dans `build.ps1 -Configuration Release` et la CI). Un `.wsb` d'une préparation antérieure ne passe plus la garde : le jeton a changé.

## Ordre et scénarios

Chaque scénario se lance dans un Sandbox **neuf** : fermer le précédent avant de lancer le suivant (un seul Sandbox à la fois ; `prepare.ps1` refuse s'il en trouve un ouvert). Préalable, une fois : Windows Sandbox activé (tests-acceptation.md § 1) et

```powershell
pwsh scripts/build.ps1 -Configuration Release
```

### 1. Complements (§ 6)

```powershell
pwsh scripts/sandbox/prepare.ps1 -Scenario Complements -Launch
```

- **Durée** : 2 à 3 min de préparation (compilation de la CLI de test, téléchargement de VLC et GIMP la première fois), puis 5 à 10 min dans le Sandbox, plus les vérifications à la main.
- **Déroulé automatique** : copie de CleanSlate sur le Bureau ; pour le raccourci mort du menu Démarrer (D20), la tâche planifiée et le service orphelins (D47) et le reste de paquet du Store (D48) : construction, mise en quarantaine, restauration, comparaison de l'état ; paquet MSIX de test construit et signé dans le Sandbox, listé puis désinstallé avec progression (EF-04, EF-21) ; installation de VLC et GIMP.
- **À vérifier** : `rapport.html` (il s'ouvre seul) : chaque cas « retiré » et « restauré à l'identique » est « atteint » ; dans `diffs\`, l'étape de restauration annule exactement l'étape de quarantaine. Puis, à la main, dans `CleanSlate` sur le Bureau, mode simulation désactivé : désinstaller VLC (pourcentage et action à l'étape 2), désinstaller GIMP (liste des processus), filtrer la page Applications sur « Store ».

### 2. 5 applications (§ 4)

```powershell
pwsh scripts/sandbox/prepare.ps1 -Scenario Apps -Launch
```

- **Durée** : téléchargement des 6 installeurs la première fois (environ 600 Mo), puis 20 à 30 min dans le Sandbox (une étape inventoriée par installation, premier lancement et désinstallation), plus la restauration à la main.
- **Déroulé automatique** : vérification des 6 SHA-256 ; installation silencieuse de VLC, 7-Zip, Notepad++, Audacity, GIMP et Firefox (témoin, gardé) ; premier lancement de chacun ; relevé de référence ; désinstallation classique des 5 ; analyse `scan --dry-run` ; rapport.
- **À vérifier** : dans `rapport.html` : T / R ≥ 90 %, zéro précoché fautif, analyse sous 60 s ; relire la liste des précochés ; le diff de l'étape d'analyse ne montre que les journaux de CleanSlate (`%LOCALAPPDATA%\CleanSlate\logs`). Puis le critère 4 à la main (tests-acceptation.md § 4.2, étapes 4 à 7).

### 3. Installer (§ 5)

```powershell
pwsh scripts/package.ps1
pwsh scripts/sandbox/prepare.ps1 -Scenario Installer -Launch
```

- **Durée** : 3 à 5 min de compilation de l'installeur, puis 5 à 10 min dans le Sandbox, presque tout à la main.
- **Déroulé** : vérification du SHA-256 de l'installeur ; l'assistant s'ouvre, vous installez ; contrôles automatiques (dossier, raccourci, entrée de désinstallation) ; le script attend pendant que vous faites les vérifications du § 5, une quarantaine, puis la désinstallation depuis Paramètres → Applications ; appuyez sur Entrée dans la console ; contrôles de la désinstallation.
- **À vérifier** : UAC, assistant en français, icône, licence MIT, À propos ; dans `rapport.html` : installé, raccourci, entrée, puis dossier supprimé et `%LOCALAPPDATA%\CleanSlate` conservé (D43).

## Ce qu'il ne faut pas faire pendant un test

- Ne pas cliquer dans les applications que le script ouvre (il les ferme lui-même après 15 à 90 s) ni fermer la console PowerShell du Sandbox.
- Ne pas relancer `prepare.ps1` pendant qu'un Sandbox tourne : le jeton changerait (il refuse de toute façon).
- Ne pas activer le réseau dans le `.wsb`, ni modifier `run\token.txt` ou le manifeste à la main.
- Ne jamais lancer `inside-*.ps1` ni `cleanslate-cli-sandbox.exe` sur le PC : ils refusent (codes 1 et 3), mais une garde ne se teste pas en lançant l'action réelle (D53).
- Ne rien copier du Sandbox vers le PC autrement que par `C:\CleanSlate\results`.

## Résultats

`artifacts\sandbox\results\<scénario>-<date>\` :

- `rapport.html` et `rapport.md` : vérifications, mesures, étapes (début, durée, nombre de lignes ajoutées et retirées de l'inventaire, lien vers le diff) ;
- `journal.txt` : transcription complète de la session (`Start-Transcript`), chaque étape horodatée ;
- `inventaires\NN-avant.json`, `NN-apres.json` : applications (clés Uninstall et paquets Appx), AppData, ProgramData, Program Files, menu Démarrer, `HKCU\Software`, `HKLM\SOFTWARE` (deux niveaux de dossiers ou de clés), tâches planifiées, services ;
- `diffs\NN-<étape>.txt` : lignes ajoutées (`+`) et retirées (`-`) par catégorie pendant l'étape ;
- les sorties de la CLI (`scan.json`, `apps.json`, `cli-progression.txt`…) et, pour 5 applications, `reference.txt` et `restes-reels.txt`.
