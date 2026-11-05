# Tests dans Windows Sandbox

Les actions réelles de CleanSlate (quarantaine, désinstallation) ne se testent **que** dans Windows Sandbox : une machine jetable, effacée à sa fermeture (CLAUDE.md, règle 1 ; D53). Ce dossier contient tout ce qu'il faut pour le faire, et ce que chaque fichier fait.

## Fichiers

| Fichier | Où il tourne | Rôle |
| --- | --- | --- |
| `prepare.ps1` | hôte (pwsh 7) | Vérifie la compilation Release, télécharge et vérifie les installeurs, compile la CLI de test (Complements, Apps et Gestionnaire), écrit le jeton et le `.wsb`, lance le Sandbox avec `-Launch`. N'installe rien et ne lance rien de CleanSlate sur l'hôte. |
| `manifeste.json` | hôte et Sandbox | Tout ce qui est téléchargé ou installé : nom, version, éditeur, URL officielle, SHA-256, commande d'installation silencieuse ; ce qui est construit dans le Sandbox ; ce que l'hôte partage. |
| `common.ps1` | Sandbox (PowerShell 5.1) | Garde, journal, étapes horodatées, inventaires avant et après chaque étape avec un fichier de différences, vérification des SHA-256, `rapport.md` et `rapport.html`. |
| `inside-complements.ps1` | Sandbox | Scénario **Complements** (tests-acceptation.md § 6). |
| `inside-apps.ps1` | Sandbox | Scénario **5 applications** (§ 4, cahier § 6.3). |
| `inside-installer.ps1` | Sandbox | Scénario **Installer** (§ 5). |
| `inside-gestionnaire.ps1` | Sandbox | Scénario **Gestionnaire** (D79, gestionnaire-plan.md § 4). |

Produits par `prepare.ps1`, sous `artifacts\sandbox\` (ignoré par Git) :

| Chemin | Contenu |
| --- | --- |
| `installers\<id>\<fichier>` | installeurs du manifeste, téléchargés depuis leur URL officielle ; un SHA-256 différent arrête `prepare.ps1` |
| `tools\cleanslate-cli.exe` | la CLI publiée (contrôlée par `check-release-cli.ps1`), pour l'analyse en lecture seule |
| `tools\cleanslate-cli-sandbox.exe` | la CLI compilée avec la feature `sandbox-tests` (scénarios Complements, Apps et Gestionnaire ; supprimée pour Installer) |
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

- **Durée** : 2 à 3 min de préparation (compilation de la CLI de test, téléchargement de VLC et GIMP la première fois), puis 10 à 15 min dans le Sandbox (VLC et GIMP sont installés deux fois), plus les vérifications à la main.
- **Déroulé automatique** : copie de CleanSlate sur le Bureau ; pour le raccourci mort du menu Démarrer (D20), la tâche planifiée et le service orphelins (D47) et le reste de paquet du Store (D48) : construction, mise en quarantaine, restauration, comparaison de l'état ; paquet MSIX de test construit et signé dans le Sandbox, listé puis désinstallé avec progression (EF-04, EF-21) ; VLC (MSI) et GIMP (Inno Setup) installés, **désinstallés par CleanSlate avec vérification automatique des événements de progression** (MSI : pourcentage réel jusqu'à 100 % et actions de Windows Installer ; `.exe` : processus du désinstalleur et aucun pourcentage inventé ; ligne récapitulative « uninstall finished » dans le journal de CleanSlate ; détail dans `progression-<application>.txt`), puis réinstallés pour les vérifications à la main (D57).
- **À vérifier** : `rapport.html` (il s'ouvre seul) : chaque cas « retiré » et « restauré à l'identique » est « atteint » ; dans `diffs\`, l'étape de restauration annule exactement l'étape de quarantaine. Puis, à la main, dans `CleanSlate` sur le Bureau : d'abord les trois écrans du premier lancement (D63 ; plus de mode simulation dans l'application, D66) ; désinstaller VLC et GIMP par « Désinstaller complètement » ; une désinstallation peut durer une seconde : le « Déroulé de la désinstallation » de l'étape 4 garde chaque étape, pourcentage et processus avec son heure, à comparer avec `progression-*.txt` ; filtrer la page Applications sur « Store ».

### 2. 5 applications (§ 4)

```powershell
pwsh scripts/sandbox/prepare.ps1 -Scenario Apps -Launch
```

- **Durée** : téléchargement des 6 installeurs la première fois (environ 600 Mo), puis 20 à 30 min dans le Sandbox (une étape inventoriée par installation, premier lancement, désinstallation, puis quarantaine et restauration de chaque reste retrouvé).
- **Déroulé automatique** : vérification des 6 SHA-256 ; installation silencieuse de VLC, 7-Zip, Notepad++, Audacity, GIMP et Firefox (témoin, gardé) ; premier lancement de chacun (programme trouvé dans son dossier, jusqu'à deux niveaux, sinon par son raccourci du menu Démarrer : D58) ; relevé de référence ; désinstallation classique des 5 ; analyse `scan --dry-run` ; **critère 4 automatique** : chaque reste réel retrouvé est mis en quarantaine par `cleanslate-cli-sandbox`, restauré et comparé à son état d'avant ; rapport.
- **À vérifier** : dans `rapport.html` : T / R ≥ 90 %, zéro précoché fautif, analyse sous 60 s ; relire la liste des précochés ; le diff de l'étape d'analyse ne montre que les journaux de CleanSlate (`%LOCALAPPDATA%\CleanSlate\logs`). Le critère 4 est mesuré automatiquement ; le refaire dans l'interface (tests-acceptation.md § 4.2, étapes 4 à 7) est facultatif. Si une application n'a pas pu être lancée, le critère 2 est en « échec du script » : la mesure est incomplète.

### 3. Installer (§ 5)

```powershell
pwsh scripts/package.ps1
pwsh scripts/sandbox/prepare.ps1 -Scenario Installer -Launch
```

- **Durée** : 3 à 5 min de compilation de l'installeur, puis 5 à 10 min dans le Sandbox, presque tout à la main.
- **Déroulé** : vérification du SHA-256 de l'installeur, copié dans Téléchargements comme un fichier téléchargé ; l'assistant s'ouvre, vous installez (assistant annulé : relancez l'installeur depuis Téléchargements, le script attend 20 min) ; contrôles automatiques (dossier, raccourci, entrée de désinstallation) ; le script attend pendant que vous faites le premier lancement (trois écrans d'accueil, D63), les vérifications du § 5, puis la désinstallation depuis Paramètres → Applications ; **tapez « fini »** dans la console (une simple touche Entrée ne suffit pas) ; contrôles de la désinstallation.
- **À vérifier** : UAC, assistant en français, icône, licence MIT, À propos ; dans `rapport.html` : installé, raccourci, entrée, puis dossier supprimé, réglages de l'utilisateur et dossier de quarantaine choisi conservés (D43).

### 4. Gestionnaire (D79)

```powershell
pwsh scripts/sandbox/prepare.ps1 -Scenario Gestionnaire -Launch
```

- **Durée** : 1 à 2 min de préparation (compilation de la CLI de test), puis 5 à 15 min dans le Sandbox, sans rien à faire (le nettoyage du magasin des composants par DISM est le plus long).
- **Déroulé automatique** : chaque cas écrit une requête du Gestionnaire (`requete-NN-<op>.json`, `dry_run` à false), l'envoie au vrai moteur par la commande `sandbox manager <fichier>` de la CLI de test, puis vérifie le disque : caches vidés (dossier gardé) ; Corbeille (« Sûr », taille par lecteur, vidage) ; « Immédiat » ; suppression en masse des `.log` trouvés par l'analyse ; fichier ouvert refusé, supprimé au redémarrage, listé puis annulé ; doublons (la copie la plus ancienne gardée) ; déplacement vers un second lecteur `X:` avec SHA-256 comparé : un disque virtuel VHDX de 256 Mo attaché par `diskpart` (le Sandbox n'a que C:), à défaut un lecteur `subst` ; « Sûr » depuis `X:` vers la quarantaine de CleanSlate sur C: (sur un lecteur `subst`, simple alias de C:, le moteur doit refuser le chemin : règle 6) ; zone sensible `C:\Windows\Temp` : `EXPERT_REQUIRED`, puis `CONFIRMATION_REQUIRED`, puis Corbeille avec « CONFIRMER » et restauration à l'identique ; nettoyage système (cache de Windows Update, Optimisation de la distribution, hibernation, points de restauration, magasin des composants) par les outils officiels ; une mesure enregistrée puis relue ; les disques physiques lus par `DeviceIoControl` (sans fenêtre PowerShell), comparés à `Get-Disk` (D82).
- **À vérifier** : `rapport.html` (il s'ouvre seul) : chaque cas « ok ». Un cas du nettoyage système en « ko » affiche le message de l'outil de Windows (certains services manquent dans le Sandbox) : à relire avant de conclure. Facultatif, à la main : lancer CleanSlate depuis le Bureau, ouvrir le Gestionnaire et parcourir les 11 onglets.

## Ce qu'il ne faut pas faire pendant un test

- Ne pas cliquer dans les applications que le script ouvre (il les ferme lui-même après 15 à 90 s) ni fermer la console PowerShell du Sandbox.
- Ne pas relancer `prepare.ps1` pendant qu'un Sandbox tourne : le jeton changerait (il refuse de toute façon).
- Ne pas activer le réseau dans le `.wsb`, ni modifier `run\token.txt` ou le manifeste à la main.
- Ne pas parcourir `C:\CleanSlate\repo` dans l'Explorateur du Sandbox : le dépôt partagé compte des dizaines de milliers de fichiers (`core\target`), et c'est en l'ouvrant que le Sandbox a planté le 30 oct. 2026 (D56). Pour lire les traces, ouvrir `C:\CleanSlate\results` : elles y sont déjà, et aussi sur le PC.
- Ne jamais lancer `inside-*.ps1` ni `cleanslate-cli-sandbox.exe` sur le PC : ils refusent (codes 1 et 3), mais une garde ne se teste pas en lançant l'action réelle (D53).
- Ne rien copier du Sandbox vers le PC autrement que par `C:\CleanSlate\results`.

## Résultats

`artifacts\sandbox\results\<scénario>-<date>\` :

- `rapport.html` et `rapport.md` : vérifications (« atteint », « NON atteint » = résultat de CleanSlate, « échec du script » = le test n'a pas pu s'exécuter, aucune conclusion sur CleanSlate, « à faire à la main »), mesures, étapes (début, durée, nombre de lignes ajoutées et retirées de l'inventaire, lien vers le diff) ;
- `journal.txt` : chaque étape horodatée, avec toute la sortie de ses commandes ; écrit **ligne par ligne** sur le disque du PC, rien n'est perdu si le Sandbox se ferme brutalement ; une erreur du script y figure avec sa position et sa pile ;
- `transcription.txt` : transcription PowerShell (`Start-Transcript`), complète seulement si le scénario va au bout ;
- `cleanslate-logs\` : journaux de CleanSlate (moteur `cleanslate.<date>.log` et erreurs de l'interface `crash.<date>.log`, avec la pile d'appels complète), écrits directement ici grâce à la valeur `LogDirectory` de `HKLM\SOFTWARE\CleanSlate` (posée par le script, D93), y compris pour CleanSlate lancé depuis le Bureau ;
- `dumps\` : vidages de plantage de `CleanSlate.App.exe`, `cleanslate-cli.exe` et `cleanslate-cli-sandbox.exe` (Windows Error Reporting, clé `LocalDumps`) ;
- `evenements.txt` : erreurs du journal Application (.NET Runtime, Application Error, Windows Error Reporting, Application Hang) depuis le début, recopiées après chaque étape puis toutes les 10 s pendant les vérifications à la main ;
- `inventaires\NN-avant.json`, `NN-apres.json` : applications (clés Uninstall et paquets Appx), AppData, ProgramData, Program Files, menu Démarrer, `HKCU\Software`, `HKLM\SOFTWARE` (deux niveaux de dossiers ou de clés), tâches planifiées, services ;
- `diffs\NN-<étape>.txt` : lignes ajoutées (`+`) et retirées (`-`) par catégorie pendant l'étape ;
- les sorties de la CLI (`scan.json`, `apps.json`, `cli-progression.txt`…) et, pour 5 applications, `reference.txt` et `restes-reels.txt`.

## En cas de plantage

Après la fin du scénario, la console du Sandbox reste ouverte et **surveille** : toutes les 10 s, elle recopie les événements d'erreur et signale tout nouveau vidage ou `crash.*.log`. Les traces sont déjà sur le PC, dans `artifacts\sandbox\results\<scénario>-<date>\`, même si le Sandbox se ferme ensuite.

**CleanSlate se ferme ou affiche « CleanSlate a rencontré une erreur inattendue »** : lire `cleanslate-logs\crash.<date>.log` (exception complète), puis `evenements.txt` et `dumps\` (un `.dmp` s'ouvre avec Visual Studio ou WinDbg). Une erreur de l'interface est désormais journalisée et affichée ; CleanSlate continue quand c'est possible (D56).

**Le Sandbox entier se ferme d'un coup** : ce n'est ni CleanSlate ni les scripts. Aucun script n'arrête la session, et la console reste ouverte (`-NoExit`). Le 30 oct. 2026, c'était un **écran bleu du noyau du Sandbox**. Pour le savoir après coup, sur le PC :

```powershell
# Plantages du Sandbox : événement « ContainerCrashDump » ; P1 = code d'arrêt (59 = 0x3B), P2 = code d'exception
Get-WinEvent -FilterHashtable @{ LogName = 'Application'; ProviderName = 'Windows Error Reporting'; StartTime = (Get-Date).AddHours(-4) } |
    Where-Object Message -match 'ContainerCrashDump' | Format-List TimeCreated, Message
# Machine virtuelle et conteneur (souvent vides) ; filtre des dossiers partagés
Get-WinEvent -LogName 'Microsoft-Windows-Hyper-V-Compute-Operational', 'Microsoft-Windows-Hyper-V-Worker-Admin', 'Microsoft-Windows-Containers-BindFlt/Operational' -MaxEvents 30 -ErrorAction SilentlyContinue |
    Format-Table TimeCreated, Id, Message -Wrap
```

Le vidage du noyau du Sandbox est dans le rapport WER (`C:\ProgramData\Microsoft\Windows\WER\ReportArchive\Critical_59_*` ou `ReportQueue\Critical_59_*`), lisible seulement en administrateur. Pour l'analyser (PowerShell **administrateur**, débogueur du Windows SDK) :

```powershell
$dmp = Get-ChildItem 'C:\ProgramData\Microsoft\Windows\WER\Report*\Critical_59_*' -Recurse -Filter *.dmp | Sort-Object LastWriteTime | Select-Object -Last 1
& "${env:ProgramFiles(x86)}\Windows Kits\10\Debuggers\x64\kd.exe" -z $dmp.FullName -c '!analyze -v; q'
```

La ligne `MODULE_NAME` / `IMAGE_NAME` de `!analyze` désigne le pilote fautif : `bindflt.sys` ou `wcifs.sys` (dossiers partagés), un pilote graphique (vGPU), etc.

Réglages du `.wsb` qui limitent le risque (D56) : vGPU désactivé (rendu logiciel), 8 Go de mémoire, pas de micro, caméra ni imprimante. Un manque de mémoire ralentit ou fait échouer des programmes, il ne provoque pas d'écran bleu 0x3B.
