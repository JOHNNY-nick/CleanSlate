# Tests d'acceptation — CleanSlate

Dernière mise à jour : 30 oct. 2026

Les opérations destructives (mise en quarantaine, désinstallation) ne se testent **jamais** sur un PC principal : uniquement dans Windows Sandbox (jetable : tout disparaît à la fermeture) ou dans une machine virtuelle.

## 0. Lancement automatique (D46)

Une fois Windows Sandbox activé (§ 1, étape 2) et CleanSlate compilé en Release :

```powershell
pwsh scripts/sandbox/prepare.ps1 -Scenario Complements -Launch  # § 6 : compléments v1.0
pwsh scripts/sandbox/prepare.ps1 -Scenario Apps -Launch         # § 4 : scénario des 5 applications
pwsh scripts/sandbox/prepare.ps1 -Scenario Installer -Launch    # § 5 : installeur (après scripts/package.ps1)
```

Le détail (fichiers, ordre, durée, ce qu'il faut vérifier et ne pas faire, garde et ses limites) est dans [scripts/sandbox/README.md](../scripts/sandbox/README.md). En bref :

- `prepare.ps1` télécharge les installeurs listés dans `scripts/sandbox/manifeste.json` depuis leur URL officielle et vérifie leur SHA-256 (rien n'est installé sur le PC), écrit un jeton aléatoire et `artifacts\sandbox\CleanSlate-<Scénario>.wsb`. Le dépôt est partagé en lecture seule, seul `artifacts\sandbox\results` est en écriture ; le réseau du Sandbox est coupé.
- Dans le Sandbox, `inside-<scénario>.ps1` démarre seul, revérifie les SHA-256 avant d'installer quoi que ce soit, horodate chaque étape, inventorie le système avant et après chacune (fichier de différences) et écrit `results\<scénario>-<date>\rapport.html` et `rapport.md`, avec la transcription complète (`journal.txt`).
- Pour § 4 : T / R (critère 2) et précochés fautifs (critère 3) sont mesurés ; le critère 4 (restauration à l'identique, § 4.2 étapes 4 à 7) se fait ensuite à la main avec la copie de CleanSlate posée sur le Bureau du Sandbox.
- Les scripts `inside-*.ps1` et les commandes de test de la CLI refusent de tourner hors du Sandbox préparé (compte `WDAGUtilityAccount`, variable `CLEANSLATE_SANDBOX_RUN` et même jeton dans le fichier partagé, D53).

## 1. Préparer Windows Sandbox

1. Windows 10/11 Pro ou Entreprise, virtualisation activée dans le BIOS.
2. Une seule fois, dans un PowerShell **administrateur** :
   ```powershell
   Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All
   ```
   puis redémarrer.
3. Compiler CleanSlate en Release depuis la racine du dépôt :
   ```powershell
   pwsh scripts/build.ps1 -Configuration Release
   ```
4. Créer `CleanSlate.wsb` (n'importe où) avec le contenu suivant, en adaptant le chemin du dépôt. Le dossier de l'application est partagé **en lecture seule** :
   ```xml
   <Configuration>
     <MappedFolders>
       <MappedFolder>
         <HostFolder>C:\chemin\vers\cleanslate\app\CleanSlate.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64</HostFolder>
         <SandboxFolder>C:\CleanSlate</SandboxFolder>
         <ReadOnly>true</ReadOnly>
       </MappedFolder>
     </MappedFolders>
     <Networking>Enable</Networking>
   </Configuration>
   ```
5. Double-cliquer sur `CleanSlate.wsb`. Dans la Sandbox, copier `C:\CleanSlate` vers `C:\Users\WDAGUtilityAccount\Desktop\CleanSlate` (le dossier partagé est en lecture seule), puis lancer `CleanSlate.App.exe` depuis la copie. Le compte de la Sandbox est administrateur.

## 2. Quarantaine et restauration (J4)

Préparer des restes factices dans la Sandbox (PowerShell de la Sandbox) :

```powershell
$r = "$env:APPDATA\OldTool"; New-Item -ItemType Directory -Force $r | Out-Null
Set-Content "$r\settings.ini" "[x]`nlang=fr"
(Get-Item "$r\settings.ini").LastWriteTime = (Get-Date).AddDays(-200)
New-Item -Force "HKCU:\Software\OldTool" | Out-Null
Set-ItemProperty "HKCU:\Software\OldTool" -Name Lang -Value fr
Get-FileHash "$r\settings.ini"   # noter l'empreinte
```

| # | Étape | Résultat attendu |
| --- | --- | --- |
| 1 | Réglages : vérifier que « Mode simulation » est **activé** | Activé par défaut |
| 2 | Restes orphelins → Analyser ; cocher `OldTool` (dossier et clé) → Mettre en quarantaine → bouton « Simuler » | Message « Simulation : 2 élément(s) auraient été mis en quarantaine » ; `OldTool` toujours présent (dossier et clé) |
| 3 | Réglages : désactiver le mode simulation ; relancer l'analyse | Nouvelle analyse |
| 4 | Cocher `OldTool` (dossier et clé) → Mettre en quarantaine → confirmer | « 2 élément(s) mis en quarantaine » ; `%APPDATA%\OldTool` et `HKCU\Software\OldTool` absents ; `%LOCALAPPDATA%\CleanSlate\quarantine\<session>\` contient `manifest.json`, `files\…` et `registry\<id>.reg` (UTF-16 LE) |
| 5 | Historique : la session apparaît (date, 2 éléments, date de purge à +30 jours) | Session « Terminée » |
| 6 | Cocher seulement le dossier → Restaurer | Dossier revenu, clé toujours en quarantaine ; `Get-FileHash` identique, date de modification identique |
| 7 | Restaurer (sans sélection) | Clé revenue avec ses valeurs ; session « Restaurée » |
| 8 | Refaire 4, recréer `%APPDATA%\OldTool` à la main, puis Restaurer | Refus « un élément existe déjà à cet emplacement » ; le dossier recréé n'est pas touché |
| 9 | Fichier verrouillé : ouvrir `settings.ini` avec un programme qui le verrouille, mettre en quarantaine le **fichier** | Signalé « au prochain redémarrage » (statut « Au prochain redémarrage ») |
| 10 | Élément protégé : essayer de mettre en quarantaine un dossier sous `%APPDATA%\Microsoft` (via un scan qui ne le propose pas, il n'apparaît jamais) | Jamais proposé ; refusé par le moteur s'il est demandé |
| 11 | Interruption : pendant une grosse mise en quarantaine, fermer CleanSlate depuis le Gestionnaire des tâches, relancer, ouvrir l'Historique | Session reprise : l'élément en cours est soit en quarantaine, soit laissé en place (« Non déplacé ») ; aucun élément perdu |
| 12 | Historique → Purger les sessions anciennes, Réglages : durée = 1 jour, changer la date du système à +2 jours, relancer | Sessions de plus d'un jour purgées au démarrage ; dossiers `files` et `registry` supprimés, `manifest.json` conservé (statut « Purgée ») |

## 3. Désinstallation complète (J5)

Dans la Sandbox (connexion réseau activée par `CleanSlate.wsb`), installer trois applications réelles, les lancer une fois pour qu'elles créent leurs données, puis les fermer :

```powershell
winget install --id 7zip.7zip -e --source winget --accept-package-agreements --accept-source-agreements
winget install --id VideoLAN.VLC -e --source winget --accept-package-agreements --accept-source-agreements
winget install --id Notepad++.Notepad++ -e --source winget --accept-package-agreements --accept-source-agreements
```

(Si `winget` n'est pas disponible dans la Sandbox, télécharger les installeurs depuis les sites officiels.) Lancer 7-Zip File Manager, VLC et Notepad++ une fois chacun, puis les fermer.

| # | Étape | Résultat attendu |
| --- | --- | --- |
| 1 | Mode simulation **activé** : Applications → sélectionner VLC → Désinstaller complètement → Lancer | Étape 4 : « Simulation : la commande ci-dessous aurait été lancée » ; VLC toujours installé |
| 2 | Réglages : désactiver la simulation | — |
| 3 | Applications → VLC → Désinstaller complètement (clic 2) → Lancer la désinstallation (clic 3) | Point de restauration tenté (protection système souvent désactivée en Sandbox : message non bloquant) ; le désinstalleur de VLC s'ouvre ; « En attente du désinstalleur… » jusqu'à sa fin |
| 4 | Terminer le désinstalleur | Étape 3 : restes trouvés (`%APPDATA%\vlc`, éventuellement `Program Files\VideoLAN`, `HKCU\Software\VideoLAN`…), confiance « Élevée » précochée |
| 5 | Mettre les restes en quarantaine (clic 4) → Confirmer (clic 5) | Étape 4 : restes mis en quarantaine ; **5 clics** depuis la page Applications (ENF-07) ; session visible dans l'Historique, origine « Désinstallation complète » |
| 6 | 7-Zip : cocher « Désinstallation silencieuse » si proposé → Lancer | Aucune fenêtre du désinstalleur (EF-14) ; restes proposés |
| 7 | Notepad++ : Lancer, puis **annuler** dans la fenêtre du désinstalleur | Étape 4 : « Le désinstalleur a été annulé ou a échoué : l'application est toujours installée » ; aucun reste proposé |
| 8 | Notepad++ : « Désinstallation classique » | Le désinstalleur officiel seul ; pas d'étape des restes ; l'application disparaît de la liste |
| 9 | Restes orphelins → Analyser | Les restes de Notepad++ laissés par la désinstallation classique sont proposés |
| 10 | Historique → restaurer la session VLC | Dossiers et clés de VLC revenus à l'identique |
| 11 | Désinstalleur qui ne rend pas la main : vérifier que « Ne plus attendre » arrête l'attente sans fermer la fenêtre du désinstalleur | Le désinstalleur reste ouvert ; l'assistant passe à l'étape 4 |

## 4. Scénario d'acceptation des 5 applications (cahier § 6.3)

But : vérifier les trois critères mesurables du cahier § 6.3 sur un jeu de 5 applications connues, dans une Sandbox **neuve** (relancer `CleanSlate.wsb`).

### 4.1 Préparation

1. Installer les 5 applications :
   ```powershell
   $ids = 'VideoLAN.VLC', '7zip.7zip', 'Notepad++.Notepad++', 'Audacity.Audacity', 'GIMP.GIMP.3'
   foreach ($id in $ids) { winget install --id $id -e --source winget --accept-package-agreements --accept-source-agreements }
   ```
   (Si l'identifiant de GIMP a changé : `winget search GIMP`.)
2. Lancer chaque application une fois, changer un réglage (par exemple la langue ou un dossier récent) pour qu'elle écrive ses données dans `%APPDATA%`, `%LOCALAPPDATA%` et `HKCU\Software`, puis la fermer.
3. **Relevé de référence** des restes attendus, avant toute désinstallation. Dans un PowerShell de la Sandbox :
   ```powershell
   $roots = "$env:ProgramFiles", "${env:ProgramFiles(x86)}", $env:APPDATA, $env:LOCALAPPDATA, "$env:LOCALAPPDATA\Programs", $env:ProgramData
   $names = 'vlc','VideoLAN','7-Zip','Notepad++','Audacity','GIMP'
   $ref = foreach ($r in $roots) { Get-ChildItem $r -Directory -ErrorAction SilentlyContinue | Where-Object { $n = $_.Name; $names | Where-Object { $n -like "*$_*" } } | ForEach-Object FullName }
   $ref += foreach ($k in 'HKCU:\Software','HKLM:\SOFTWARE','HKLM:\SOFTWARE\WOW6432Node') { Get-ChildItem $k -ErrorAction SilentlyContinue | Where-Object { $n = $_.PSChildName; $names | Where-Object { $n -like "*$_*" } } | ForEach-Object Name }
   $ref | Sort-Object | Set-Content "$env:USERPROFILE\Desktop\reference.txt"
   ```
   Ce relevé compte les **dossiers de premier niveau et les clés** de chaque application (l'unité des groupes de CleanSlate).
4. Désinstaller les 5 applications **avec leur désinstalleur officiel** (Paramètres → Applications, ou leur `Uninstall.exe`), sans CleanSlate : c'est la « désinstallation classique » du critère.
5. Pour chaque ligne de `reference.txt`, noter si elle existe encore : ce sont les **restes réels** (R). Une ligne supprimée par le désinstalleur n'est pas un reste.
6. Installer en plus une application **qu'on garde** (par exemple `winget install Mozilla.Firefox`) et la lancer une fois : elle sert de témoin pour le critère « zéro élément d'une application encore installée ».

### 4.2 Déroulé

| # | Étape | Résultat attendu |
| --- | --- | --- |
| 1 | Lancer CleanSlate (mode simulation activé) → Restes orphelins → Analyser les restes | Analyse terminée en moins de 60 s (ENF-05) ; groupes « Probablement : … » pour VLC, 7-Zip, Notepad++, Audacity, GIMP |
| 2 | Compter, parmi les restes réels R de l'étape 4.1.5, ceux qui apparaissent dans la liste (élément ou dossier parent) : T | **T / R ≥ 90 %** (critère 2) |
| 3 | Relever tous les éléments **précochés** (confiance Élevée) | Aucun n'appartient à Firefox, à une application encore installée ni à Windows (`Microsoft`, `Windows`, `Common Files`, `Packages`…) (critère 3) |
| 4 | Réglages → désactiver la simulation ; relancer l'analyse ; garder la présélection → Mettre en quarantaine → Confirmer | Session « Terminée » ; éléments absents du disque et du registre |
| 5 | Avant l'étape 4, enregistrer l'empreinte des fichiers concernés : `Get-ChildItem <dossier> -Recurse -File \| Get-FileHash \| Export-Csv avant.csv` ; exporter les clés : `reg export <clé> avant.reg` | — |
| 6 | Historique → Restaurer la session | Session « Restaurée » ; `Get-FileHash` identique à `avant.csv` (même contenu, mêmes dates) ; `reg export` identique à `avant.reg` (critère 4) |
| 7 | Remettre en quarantaine puis Historique → Purger | Dossiers `files` et `registry` de la session supprimés ; `manifest.json` gardé |

### 4.3 Grille de résultats

| Critère (cahier § 6.3) | Mesure | Résultat |
| --- | --- | --- |
| Exigences M satisfaites | `docs/journal/revue-finale.md` | |
| ≥ 90 % des restes retrouvés | T / R = … / … | |
| Zéro élément d'une application installée ou du système précoché | Nombre d'éléments précochés fautifs : … | |
| Session restaurable à l'identique | Empreintes et `.reg` identiques : oui / non | |
| Tests automatisés et `clippy` sans alerte | `cargo test`, `cargo clippy -D warnings`, `dotnet test` | |

Noter dans la colonne « Résultat » les éléments manqués ou précochés à tort, avec leur chemin : ce sont les cas à ajouter aux tests du `matcher`.

## 5. Installeur (J6)

1. Sur le PC de développement : `pwsh scripts/package.ps1` → `artifacts\installer\CleanSlate-0.1.0-setup.exe`.
2. `pwsh scripts/sandbox/prepare.ps1 -Scenario Installer -Launch` : dans un Sandbox neuf, `inside-installer.ps1` vérifie le SHA-256 de l'installeur, le copie sur le Bureau et le lance, contrôle l'installation, attend la désinstallation faite à la main, puis la contrôle (`rapport.html`).

| # | Étape | Résultat attendu |
| --- | --- | --- |
| 1 | Lancer `CleanSlate-0.1.0-setup.exe` | Demande d'élévation (UAC) ; assistant en français, icône CleanSlate, licence MIT |
| 2 | Terminer l'installation, cocher « Exécuter CleanSlate » | Installé dans `C:\Program Files\CleanSlate` ; raccourci dans le menu Démarrer ; l'application s'ouvre sans rien installer d'autre (.NET et Windows App SDK embarqués) |
| 3 | Réglages → À propos | Icône CleanSlate, version 0.1.0 |
| 4 | Faire une quarantaine (§ 2), puis désinstaller CleanSlate depuis Paramètres → Applications | `C:\Program Files\CleanSlate` supprimé ; `%LOCALAPPDATA%\CleanSlate` (quarantaine, historique) **conservé** (D43) |

## 6. Compléments v1.0 : menu Démarrer, tâches, services, Store, progression

Lancé seul par `pwsh scripts/sandbox/prepare.ps1 -Scenario Complements -Launch` (D50). Le script `inside-complements.ps1` construit chaque cas **dans le Sandbox**, relève son état, le met en quarantaine (`cleanslate-cli-sandbox sandbox quarantine`, CLI compilée avec la feature `sandbox-tests`, D53), vérifie qu'il a disparu, le restaure (`… sandbox restore`) et compare. Rapport : `artifacts\sandbox\results\complements-<date>\rapport.html` (et `rapport.md`).

| # | Cas | Construction | Comparé à l'identique |
| --- | --- | --- | --- |
| 1 | Raccourci mort du menu Démarrer et son dossier (D20) | `%APPDATA%\…\Start Menu\Programs\CleanSlateTest Old Tool\Old Tool.lnk` vers un programme ensuite supprimé | contenu, taille, date et SHA-256 du `.lnk` ; dossier recréé |
| 2 | Tâche planifiée orpheline (D47) | `schtasks /create /tn CleanSlateTest\Agent` vers un programme ensuite supprimé | XML complet (`schtasks /query /xml`) |
| 3 | Service orphelin (D47) | `sc create CleanSlateTestSvc` (LocalService, description, actions de récupération, sous-clé `Parameters`), programme ensuite supprimé | `sc qc`, `sc qdescription`, `sc qfailure`, export `.reg` de la clé (valeurs triées par clé) |
| 4 | Application du Store (EF-04, EF-21) | paquet MSIX `CleanSlateTest.App` construit avec `makeappx`, signé avec un certificat de test créé dans le Sandbox (`signtool`), installé par `Add-AppxPackage` | listé avec `kind: store` ; désinstallé pour l'utilisateur avec relevés de progression (pourcentage) |
| 5 | Reste de paquet du Store (D48) | `%LOCALAPPDATA%\Packages\CleanSlateTest.Leftover_abcdefghjk123` et sa clé `AppContainer\Storage` | fichiers (SHA-256, dates) et export `.reg` |

Le script installe ensuite VLC et GIMP depuis les installeurs vérifiés. À faire à la main, dans l'application copiée sur le Bureau du Sandbox : désinstaller un MSI (VLC, § 3) et vérifier à l'étape 2 le pourcentage et l'action ; désinstaller GIMP (désinstalleur `.exe` Inno Setup) et vérifier la liste des processus (EF-21) ; filtrer la page Applications sur « Store ».
