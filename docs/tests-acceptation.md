# Tests d'acceptation — CleanSlate

Dernière mise à jour : 29 oct. 2026

Les opérations destructives (mise en quarantaine, désinstallation) ne se testent **jamais** sur un PC principal : uniquement dans Windows Sandbox (jetable : tout disparaît à la fermeture) ou dans une machine virtuelle.

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

Voir la section ajoutée en J5.

## 4. Scénario d'acceptation des 5 applications (cahier § 6.3)

Voir la section ajoutée à la fin du projet.
