<#
.SYNOPSIS
    Gestionnaire (D79, EF-22 to EF-35), run INSIDE Windows Sandbox by the logon command written
    by "prepare.ps1 -Scenario Gestionnaire".

.DESCRIPTION
    Each case builds its test files, sends one or more requests of the Gestionnaire through
    "cleanslate-cli-sandbox sandbox manager <request.json>" (real engine, dry_run false) and
    checks the result on the disk:
     1. caches emptied, the cache folder kept;
     2. Recycle Bin: deletion « Sûr », size of the Recycle Bin, emptying;
     3. deletion « Immédiat »;
     4. mass deletion by filter (extension) after an analysis;
     5. deletion at restart of a file in use, then its cancellation;
     6. duplicates: one group, the oldest copy kept;
     7. move to another drive (a virtual disk X:, else a subst letter), SHA-256 compared;
     8. « Sûr » with the quarantine on another drive: freed now, restorable (on a subst
        letter, an alias of C:, the engine must refuse the path: rule 6);
     9. a sensitive zone: refused outside expert mode, then « CONFIRMER », then restored
        from the Recycle Bin;
    10. system clean-up: Windows Update cache, Delivery Optimization, hibernation, restore points,
        component store (return codes);
    11. measures (« Évolution »);
    12. physical disks read by DeviceIoControl, without any console window (D82).
    Writes results\gestionnaire-<date>\rapport.md and rapport.html. The test commands only run
    when the three conditions of the guard hold (D53).
#>
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
Assert-Sandbox
Start-Run 'gestionnaire'

$out = $Run.Out
$sandboxCli = "$Tools\cleanslate-cli-sandbox.exe"
$work = 'C:\CleanSlateTest'
New-Item -ItemType Directory -Force $work | Out-Null
$prepared = Get-Content "$Repo\artifacts\sandbox\run\prepare.json" -Raw -Encoding utf8 | ConvertFrom-Json
$checks = @()
$meta = @("Date : $(Get-Date -Format 'dd/MM/yyyy HH:mm')", "Préparé le $($prepared.prepare), commit $($prepared.commit)", "Dossier : $out")

if (-not (Test-Path $sandboxCli)) {
    Write-Report 'Gestionnaire — ARRÊTÉ' $meta @([pscustomobject]@{ Name = 'CLI de test présente'; Measure = "$sandboxCli absent : relancer prepare.ps1 -Scenario Gestionnaire"; Status = 'script' }) ([ordered]@{})
    Stop-Run
    exit 1
}

# Sends one request of the Gestionnaire; returns the JSON answer (or { error, message }).
$script:requestNumber = 0
function Send($request) {
    $script:requestNumber++
    $file = "$out\requete-$('{0:00}' -f $script:requestNumber)-$($request.op).json"
    $request | ConvertTo-Json -Depth 6 | Set-Content $file -Encoding utf8
    $answer = Invoke-CleanSlateCli $sandboxCli @('sandbox', 'manager', $file)
    # Every answer is kept next to its request (cli-out.json is overwritten each time).
    Copy-Item "$out\cli-out.json" ($file -replace '\\requete-', '\reponse-') -ErrorAction SilentlyContinue
    if ($answer -and $answer.error) { Write-Host "réponse : $($answer.error) : $($answer.message)" }
    $answer
}

function NewFile([string] $path, [int] $kb, [byte] $fill = 7) {
    # New-Item refuses a drive root (« X:\ »): only create a folder that is missing.
    $folder = Split-Path $path
    if (-not (Test-Path -LiteralPath $folder)) { New-Item -ItemType Directory -Force $folder | Out-Null }
    [IO.File]::WriteAllBytes($path, [Linq.Enumerable]::ToArray([Linq.Enumerable]::Repeat([byte] $fill, $kb * 1024)))
}

# $ok and $measure may be missing when the step of the script failed: the row stays.
function Check([string] $name, $ok, $measure, $step) {
    $script:checks += [pscustomobject]@{ Name = $name; Measure = "$measure"; Status = (Status ([bool] $ok) $step) }
}

# PendingFileRenameOperations as Windows stores it, empty targets included (read only).
function PendingRaw {
    @((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager' -ErrorAction SilentlyContinue).PendingFileRenameOperations) -join '|'
}

$manual = @{ expert_mode = $false; confirmation = $null }

# For the optional tour of the 11 tabs by hand at the end.
$null = Invoke-Step 'Copie de CleanSlate sur le Bureau' -NoInventory {
    Copy-Item "$Repo\app\CleanSlate.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64" "$([Environment]::GetFolderPath('Desktop'))\CleanSlate" -Recurse -Force
}

# 1. Caches.
$step = Invoke-Step 'Caches : vider le contenu de %LOCALAPPDATA%\Temp' -NoInventory {
    $temp = Join-Path $env:LOCALAPPDATA 'Temp'
    NewFile "$temp\cs-cache\a.tmp" 512
    $r = Send @{ op = 'clean_caches'; paths = @($temp); manual = $manual; dry_run = $false }
    $script:cacheOk = (-not (Test-Path "$temp\cs-cache\a.tmp")) -and (Test-Path $temp)
    $script:cacheMeasure = "fait : $($r.items[0].done) ; libéré $($r.freed_now_bytes) octets"
}
Check 'Caches vidés, dossier du cache conservé (EF-25)' $cacheOk $cacheMeasure $step

# 2. Recycle Bin.
$step = Invoke-Step 'Corbeille : suppression « Sûr », taille, vidage' -NoInventory {
    NewFile "$work\corbeille.bin" 2048
    $r = Send @{ op = 'delete'; paths = @("$work\corbeille.bin"); mode = 'safe'; manual = $manual; dry_run = $false }
    $bins = Send @{ op = 'recycle_bins' }
    $c = @($bins | Where-Object { $_.drive -eq 'C:' })[0]
    $empty = Send @{ op = 'empty_recycle_bin'; drive = 'C:'; dry_run = $false }
    $after = @((Send @{ op = 'recycle_bins' }) | Where-Object { $_.drive -eq 'C:' })[0]
    $script:binOk = ($r.items[0].done -eq 'recycle_bin') -and ($c.size_bytes -ge 2MB) -and ($after.size_bytes -eq 0)
    $script:binMeasure = "supprimé : $($r.items[0].done) ; Corbeille C: $($c.size_bytes) octets avant le vidage, $($after.size_bytes) après ; libéré $($empty.freed_now_bytes)"
}
Check 'Corbeille : « Sûr », taille par lecteur, vidage (EF-28)' $binOk $binMeasure $step

# 3. Immediate.
$step = Invoke-Step 'Suppression « Immédiat »' -NoInventory {
    NewFile "$work\definitif.bin" 1024
    $r = Send @{ op = 'delete'; paths = @("$work\definitif.bin"); mode = 'immediate'; manual = $manual; dry_run = $false }
    $script:immOk = ($r.items[0].done -eq 'deleted') -and -not (Test-Path "$work\definitif.bin") -and ($r.freed_now_bytes -ge 1MB)
    $script:immMeasure = "fait : $($r.items[0].done) ; libéré maintenant $($r.freed_now_bytes) octets"
}
Check 'Suppression définitive, gain immédiat (EF-23)' $immOk $immMeasure $step

# 4. Mass deletion by filter.
$step = Invoke-Step 'Suppression en masse : les .log de C:\CleanSlateTest\masse' -NoInventory {
    1..5 | ForEach-Object { NewFile "$work\masse\journal$_.log" 64 }
    NewFile "$work\masse\garder.txt" 64
    $a = Send @{ op = 'analyze'; root = "$work\masse"; largest = 100 }
    $logs = @($a.largest | Where-Object { $_.path -like '*.log' } | ForEach-Object { $_.path })
    $r = Send @{ op = 'delete'; paths = $logs; mode = 'immediate'; manual = $manual; dry_run = $false }
    $left = @(Get-ChildItem "$work\masse" -Filter *.log).Count
    $script:massOk = ($logs.Count -eq 5) -and ($left -eq 0) -and (Test-Path "$work\masse\garder.txt")
    $script:massMeasure = "$($logs.Count) fichier(s) .log trouvés par l'analyse, $left restant(s), garder.txt présent : $(Test-Path "$work\masse\garder.txt")"
}
Check 'Suppression en masse par filtre, aperçu par l''analyse (EF-28)' $massOk $massMeasure $step

# 5. At restart, then cancelled.
$step = Invoke-Step 'Suppression au redémarrage d''un fichier ouvert, puis annulation' -NoInventory {
    NewFile "$work\ouvert.bin" 64
    $rawBefore = PendingRaw
    $lock = [IO.File]::Open("$work\ouvert.bin", 'Open', 'ReadWrite', 'None')
    try {
        $now = Send @{ op = 'delete'; paths = @("$work\ouvert.bin"); mode = 'immediate'; manual = $manual; dry_run = $false }
        $later = Send @{ op = 'delete_at_restart'; paths = @("$work\ouvert.bin"); manual = $manual; dry_run = $false }
    } finally { $lock.Close() }
    $pending = @(Send @{ op = 'pending_at_restart' })
    $listed = [bool]($pending | Where-Object { $_ -like '*ouvert.bin' })
    $cancel = Send @{ op = 'cancel_at_restart'; path = "$work\ouvert.bin"; dry_run = $false }
    $still = [bool](@(Send @{ op = 'pending_at_restart' }) | Where-Object { $_ -like '*ouvert.bin' })
    # The other operations waiting in Windows must come out exactly as they were.
    $same = (PendingRaw) -eq $rawBefore
    $script:rebootOk = ($now.items[0].done -eq 'failed') -and ($later.items[0].done -eq 'at_restart') -and $listed -and -not $still -and -not $cancel.error -and $same
    $script:rebootMeasure = "fichier ouvert : $($now.items[0].error.code) ; au redémarrage : $($later.items[0].done) ; listé : $listed ; annulation : $(if ($cancel.error) { $cancel.error } else { 'ok' }) ; encore listé : $still ; autres opérations de Windows inchangées : $same"
}
Check 'Suppression au redémarrage, liste, annulation (EF-28)' $rebootOk $rebootMeasure $step

# 6. Duplicates.
$step = Invoke-Step 'Doublons : deux copies identiques et un fichier de même taille' -NoInventory {
    NewFile "$work\doublons\ancien.bin" 2048 1
    (Get-Item "$work\doublons\ancien.bin").LastWriteTime = (Get-Date).AddDays(-30)
    NewFile "$work\doublons\copie.bin" 2048 1
    NewFile "$work\doublons\autre.bin" 2048 2
    $a = Send @{ op = 'analyze'; root = "$work\doublons"; duplicate_min_bytes = 1024 }
    $groups = @(Send @{ op = 'duplicates'; candidates = $a.duplicate_candidates })
    $script:dupOk = ($groups.Count -eq 1) -and ($groups[0].files.Count -eq 2) -and ($groups[0].keep -like '*ancien.bin')
    $script:dupMeasure = "$($groups.Count) groupe(s) ; à garder : $($groups[0].keep)"
}
Check 'Doublons par taille puis SHA-256, la plus ancienne gardée (EF-30)' $dupOk $dupMeasure $step

# A second drive: the Sandbox has only C:. A real volume (a VHDX attached by diskpart)
# when the Sandbox allows it; else a subst letter, which is only an alias of C:.
$vhd = "$work\second-lecteur.vhdx"
@"
create vdisk file="$vhd" maximum=256 type=expandable
select vdisk file="$vhd"
attach vdisk
create partition primary
format fs=ntfs quick label=CSTEST
assign letter=X
exit
"@ | Set-Content "$out\diskpart-creer.txt" -Encoding ascii
diskpart /s "$out\diskpart-creer.txt" *>> "$out\diskpart.txt"
Start-Sleep -Seconds 3
$realDrive = Test-Path 'X:\'
if (-not $realDrive) {
    New-Item -ItemType Directory -Force "$work\autre-lecteur" | Out-Null
    subst X: "$work\autre-lecteur"
}
$driveKind = if ($realDrive) { 'disque virtuel' } else { 'subst' }
Write-Log "Second lecteur X: : $driveKind"

# 7. Move to another drive.
$step = Invoke-Step "Déplacement vers un autre lecteur (X:, $driveKind)" -NoInventory {
    NewFile "$work\a-deplacer\video.bin" 4096 3
    $hash = (Get-FileHash "$work\a-deplacer\video.bin").Hash
    $r = Send @{ op = 'move'; paths = @("$work\a-deplacer\video.bin"); destination = 'X:\'; manual = $manual; dry_run = $false }
    $moved = (Test-Path 'X:\video.bin') -and -not (Test-Path "$work\a-deplacer\video.bin") -and ((Get-FileHash 'X:\video.bin').Hash -eq $hash)
    $script:moveOk = ($r.items[0].done -eq 'moved') -and $moved
    $script:moveMeasure = "fait : $($r.items[0].done) $($r.items[0].error.message) ; copie identique (SHA-256) et original supprimé : $moved"
}
Check 'Déplacement vers un autre lecteur, espace vérifié, SHA-256 comparé (EF-27)' $moveOk $moveMeasure $step

# 8. « Sûr » with the quarantine on another drive.
$step = Invoke-Step "« Sûr » depuis X: ($driveKind) : quarantaine de CleanSlate sur C:" -NoInventory {
    NewFile 'X:\a-mettre-en-quarantaine.bin' 1024 4
    $r = Send @{ op = 'delete'; paths = @('X:\a-mettre-en-quarantaine.bin'); mode = 'safe'; manual = $manual; dry_run = $false }
    $q = Join-Path $env:LOCALAPPDATA 'CleanSlate\quarantine'
    $inside = [bool](Get-ChildItem $q -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.Length -eq 1MB })
    if ($realDrive) {
        $script:qOk = ($r.items[0].done -eq 'quarantine') -and -not (Test-Path 'X:\a-mettre-en-quarantaine.bin') -and $inside -and ($r.freed_now_bytes -ge 1MB)
    } else {
        # X: is an alias of C: (subst): the path does not resolve to itself, refused (rule 6).
        $script:qOk = ($r.items[0].error.code -eq 'PROTECTED') -and (Test-Path 'X:\a-mettre-en-quarantaine.bin')
    }
    $script:qMeasure = "X: = $driveKind ; fait : $($r.items[0].done) $($r.items[0].error.code) ; libéré maintenant $($r.freed_now_bytes) octets ; présent dans la quarantaine : $inside"
}
$qName = if ($realDrive) { 'Quarantaine sur un autre lecteur : libéré maintenant, restaurable (EF-24)' } else { 'Lecteur subst (alias de C:) refusé, règle 6 ; EF-24 à confirmer sur deux disques' }
Check $qName $qOk $qMeasure $step
if ($realDrive) {
    @"
select vdisk file="$vhd"
detach vdisk
exit
"@ | Set-Content "$out\diskpart-detacher.txt" -Encoding ascii
    diskpart /s "$out\diskpart-detacher.txt" *>> "$out\diskpart.txt"
} else {
    subst X: /D
}

# 9. Sensitive zone, expert mode, restored from the Recycle Bin.
$step = Invoke-Step 'Zone sensible : C:\Windows\Temp, mode expert, « CONFIRMER », restauration' -NoInventory {
    $sensitive = 'C:\Windows\Temp\CleanSlateTest-sensible.bin'
    NewFile $sensitive 256 5
    $hash = (Get-FileHash $sensitive).Hash
    $no = Send @{ op = 'delete'; paths = @($sensitive); mode = 'safe'; manual = $manual; dry_run = $false }
    $noWord = Send @{ op = 'delete'; paths = @($sensitive); mode = 'safe'; manual = @{ expert_mode = $true; confirmation = $null }; dry_run = $false }
    $yes = Send @{ op = 'delete'; paths = @($sensitive); mode = 'safe'; manual = @{ expert_mode = $true; confirmation = 'CONFIRMER' }; dry_run = $false }
    $bin = (New-Object -ComObject Shell.Application).NameSpace(10)
    $item = @($bin.Items() | Where-Object { $_.Name -like 'CleanSlateTest-sensible*' })[0]
    if ($item) { $item.InvokeVerb('undelete'); Start-Sleep -Seconds 2 }
    $back = (Test-Path $sensitive) -and ((Get-FileHash $sensitive).Hash -eq $hash)
    $script:zoneOk = ($no.items[0].error.code -eq 'EXPERT_REQUIRED') -and ($noWord.items[0].error.code -eq 'CONFIRMATION_REQUIRED') -and ($yes.items[0].done -eq 'recycle_bin') -and $back
    $script:zoneMeasure = "hors mode expert : $($no.items[0].error.code) ; sans le mot : $($noWord.items[0].error.code) ; avec « CONFIRMER » : $($yes.items[0].done) ; restauré à l'identique : $back"
}
Check 'Zone sensible : mode expert, « CONFIRMER », restauration (EF-22, A.13)' $zoneOk $zoneMeasure $step

# 10. System clean-up, by the official tools.
foreach ($task in 'update_cache', 'delivery_optimization', 'hibernation', 'restore_points', 'component_store') {
    $step = Invoke-Step "Nettoyage système : $task" -NoInventory {
        $r = Send @{ op = 'system'; task = $task; expert_mode = $false; dry_run = $false }
        $script:sysOk = $null -ne $r -and -not $r.error
        $script:sysMeasure = if ($r.error) { "$($r.error) : $($r.message)" } else { "libéré $($r.freed_bytes) octets ; redémarrage : $($r.restart_needed) ; $($r.steps -join ' | ')" }
    }
    Check "Nettoyage système : $task (EF-26, ENF-14)" $sysOk $sysMeasure $step
}

# 12. Physical disks (D82): DeviceIoControl, no PowerShell; compared with Get-Disk.
$step = Invoke-Step 'Disques physiques par DeviceIoControl, comparés à Get-Disk' -NoInventory {
    $disks = @(Send @{ op = 'disks' })
    $reference = @(Get-Disk)
    $script:diskOk = ($disks.Count -ge 1) -and ($disks.Count -eq $reference.Count) -and ($disks[0].size_bytes -eq $reference[0].Size)
    $script:diskMeasure = "moteur : $($disks.Count) disque(s), $(($disks | ForEach-Object { "$($_.number) $($_.name) $($_.size_bytes) octets, $($_.unallocated_bytes) non alloués" }) -join ' ; ') — Get-Disk : $($reference.Count) disque(s), $($reference[0].Size) octets"
}
Check 'Disques physiques sans fenêtre PowerShell (EF-34, D82)' $diskOk $diskMeasure $step

# 11. Measures.
$step = Invoke-Step 'Évolution : enregistrer puis relire une mesure' -NoInventory {
    $drive = Get-PSDrive C
    $null = Send @{ op = 'record_measure'; taken_at = (Get-Date).ToUniversalTime().ToString('o'); drive = 'C:'; total_bytes = [uint64]($drive.Used + $drive.Free); free_bytes = [uint64]$drive.Free; largest = @() }
    $m = @(Send @{ op = 'measures'; drive = 'C:' })
    $script:measureOk = $m.Count -ge 1
    $script:measureMeasure = "$($m.Count) mesure(s) relue(s)"
}
Check 'Mesures de l''espace libre (EF-33)' $measureOk $measureMeasure $step

Write-Report 'Gestionnaire — résultats dans Windows Sandbox' $meta $checks ([ordered]@{})
Stop-Run
Start-Process "$out\rapport.html"
Watch-Session
exit 0
