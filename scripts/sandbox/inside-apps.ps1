<#
.SYNOPSIS
    Acceptance scenario of the 5 applications (tests-acceptation.md § 4, cahier § 6.3),
    run INSIDE Windows Sandbox by the logon command written by prepare.ps1.

.DESCRIPTION
    Automates what does not need the interface, and measures criteria 2 and 3:
    1. checks the SHA-256 of every installer against manifeste.json (stops on a mismatch);
    2. installs the applications silently, one step each;
    3. starts each one once so that it writes its data, then closes it;
    4. records the reference (folders and keys of each application, § 4.1.3);
    5. uninstalls the 5 applications with their own uninstaller (Firefox stays);
    6. R = reference entries still present; scan with cleanslate-cli (read-only);
    7. writes rapport.md and rapport.html: T / R and the prechecked (High) items that are wrong.
    Every step is timestamped, with an inventory of the system before and after it and a
    diff file. Criterion 4 (D58): each real leftover found is quarantined by CleanSlate (sandbox
    CLI), restored and compared with its state before; the same steps in the interface stay
    optional (tests-acceptation.md § 4.2).
    Refuses to run outside the Sandbox prepared by prepare.ps1 (common.ps1, Assert-Sandbox).
#>
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
Assert-Sandbox
Start-Run 'apps'

$desktop = [Environment]::GetFolderPath('Desktop')
$out = $Run.Out
$cli = "$Tools\cleanslate-cli.exe"
$prepared = Get-Content "$Repo\artifacts\sandbox\run\prepare.json" -Raw -Encoding utf8 | ConvertFrom-Json

# Applications of the scenario: name in the Uninstall keys, main program, data names (§ 4.1.3).
$apps = @(
    @{ Name = 'VLC'; Pattern = 'VLC'; Folder = 'VideoLAN'; Exe = 'vlc.exe'; Wait = 25 },
    @{ Name = '7-Zip'; Pattern = '7-Zip'; Folder = '7-Zip'; Exe = '7zFM.exe'; Wait = 15 },
    @{ Name = 'Notepad++'; Pattern = 'Notepad\+\+'; Folder = 'Notepad++'; Exe = 'notepad++.exe'; Wait = 15 },
    # Audacity 4 keeps its program in bin\ (step 12 of 30 Oct. 2026, D58).
    @{ Name = 'Audacity'; Pattern = 'Audacity'; Folder = 'Audacity*'; Exe = 'Audacity.exe'; Wait = 30 },
    @{ Name = 'GIMP'; Pattern = 'GIMP'; Folder = 'GIMP*'; Exe = 'gimp-[0-9]*.exe'; Wait = 90 }
)
$witness = @{ Name = 'Firefox'; Folder = 'Mozilla Firefox'; Exe = 'firefox.exe'; Wait = 25 }
$dataNames = 'vlc', 'VideoLAN', '7-Zip', 'Notepad++', 'Audacity', 'GIMP'

# Start-Process refuses an empty argument list.
function Run([string] $program, [string] $arguments) {
    if ($arguments) { Start-Process $program $arguments -Wait -PassThru } else { Start-Process $program -Wait -PassThru }
}

function Uninstall-Entries {
    $keys = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    foreach ($k in $keys) {
        Get-ChildItem $k -ErrorAction SilentlyContinue | Get-ItemProperty | Where-Object DisplayName
    }
}

# Registry names as CleanSlate writes them (HKCU\…, HKLM\…), WOW6432Node ignored.
function Normalize([string] $location) {
    $l = $location -replace '^HKEY_CURRENT_USER', 'HKCU' -replace '^HKEY_LOCAL_MACHINE', 'HKLM'
    ($l -replace '\\WOW6432Node\\', '\').TrimEnd('\').ToLowerInvariant()
}

$meta = @(
    "Date : $(Get-Date -Format 'dd/MM/yyyy HH:mm')",
    "Préparé le $($prepared.prepare), commit $($prepared.commit)",
    "Dossier : $out"
)

# 1. Installers against the manifest, before anything is installed.
$script:installers = @()
Invoke-Step 'Vérification des installeurs (SHA-256 du manifeste)' {
    $script:installers = @(Test-Installer "$Repo\scripts\sandbox\manifeste.json" "$Repo\artifacts\sandbox\installers" 'apps')
    $script:installers | ForEach-Object { "$(if ($_.Ok) { 'OK ' } else { 'KO ' }) $($_.Name) $($_.Version) $($_.Actual)" }
} -NoInventory
$verified = $script:installers | ForEach-Object { "$(if ($_.Ok) { 'conforme' } else { 'NON CONFORME' }) — $($_.Name) $($_.Version) — SHA-256 $($_.Actual) — $($_.Url)" }
if (-not $script:installers.Count -or ($script:installers | Where-Object { -not $_.Ok })) {
    Write-Report 'Scénario des 5 applications — ARRÊTÉ' $meta @(
        [pscustomobject]@{ Name = 'Installeurs conformes au manifeste'; Measure = 'SHA-256'; Status = 'script' }
    ) ([ordered]@{ 'Installeurs' = $verified })
    Stop-Run
    exit 1
}

# 2. CleanSlate on the Desktop for the manual steps (the shared folder is read-only).
Invoke-Step 'Copie de CleanSlate sur le Bureau' {
    Copy-Item "$Repo\app\CleanSlate.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64" "$desktop\CleanSlate" -Recurse -Force
}

# 3. Silent installation, one step per application, with the command of the manifest.
foreach ($i in $script:installers) {
    Invoke-Step "Installation de $($i.Name) $($i.Version)" {
        $line = $i.Install.Replace('<fichier>', "`"$($i.Path)`"")
        $program, $arguments = Split-Command $line
        Write-Host "$program $arguments"
        $p = Run $program $arguments
        Write-Host "code de sortie $($p.ExitCode)"
    }
}

# 4. One start each, so that the applications write their data, then a normal close.
$programRoots = $env:ProgramFiles, ${env:ProgramFiles(x86)}, "$env:LOCALAPPDATA\Programs"
$startMenus = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs", "$env:ProgramData\Microsoft\Windows\Start Menu\Programs"
$launchSteps = @()
foreach ($a in $apps + $witness) {
    Invoke-Step "Premier lancement de $($a.Name)" {
        $exe = Find-MainProgram $programRoots $a.Folder $a.Exe $a.Name $startMenus
        if (-not $exe) { throw "$($a.Name) : programme introuvable" }
        Write-Host "$exe ($($a.Wait) s)"
        Start-Process $exe | Out-Null
        Start-Sleep -Seconds $a.Wait
        $name = [IO.Path]::GetFileNameWithoutExtension($exe)
        Get-Process $name -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
        Start-Sleep -Seconds 5
        Get-Process $name -ErrorAction SilentlyContinue | Stop-Process -Force
    }
    $launchSteps += $Run.Steps[-1]
}

# 5. Reference: top-level folders and keys of each application (§ 4.1.3).
$script:reference = @()
Invoke-Step 'Relevé de référence (§ 4.1.3)' {
    $roots = $env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:APPDATA, $env:LOCALAPPDATA, "$env:LOCALAPPDATA\Programs", $env:ProgramData
    $found = foreach ($r in $roots) {
        Get-ChildItem $r -Directory -ErrorAction SilentlyContinue |
            Where-Object { $n = $_.Name; $dataNames | Where-Object { $n -like "*$_*" } } | ForEach-Object FullName
    }
    $found += foreach ($k in 'HKCU:\Software', 'HKLM:\SOFTWARE', 'HKLM:\SOFTWARE\WOW6432Node') {
        Get-ChildItem $k -ErrorAction SilentlyContinue |
            Where-Object { $n = $_.PSChildName; $dataNames | Where-Object { $n -like "*$_*" } } | ForEach-Object Name
    }
    $script:reference = @($found | Sort-Object -Unique)
    $script:reference | Set-Content "$out\reference.txt" -Encoding utf8
    Write-Host "$($script:reference.Count) entrées"
} -NoInventory

# 6. Classic uninstall with each application's own uninstaller (Firefox is kept).
# No uninstaller is waited for more than 10 min: a box left open is reported, not clicked (D59).
$uninstallTimeout = 600
$uninstallSteps = @()
foreach ($a in $apps) {
    Invoke-Step "Désinstallation classique de $($a.Name)" {
        foreach ($e in Uninstall-Entries | Where-Object DisplayName -Match $a.Pattern) {
            $program, $arguments = Get-SilentUninstall $e
            Write-Host "$($e.DisplayName) : $program $arguments"
            $p = if ($arguments) { Start-Process $program $arguments -PassThru } else { Start-Process $program -PassThru }
            if (-not (Wait-Program $p $uninstallTimeout)) {
                $windows = Get-Process | Where-Object MainWindowTitle | ForEach-Object { "$($_.ProcessName) « $($_.MainWindowTitle) »" }
                throw "$($e.DisplayName) : désinstalleur toujours actif après $uninstallTimeout s ; fenêtres ouvertes : $($windows -join ' ; ')"
            }
            # NSIS uninstallers copy themselves and return at once: wait for the entry to go.
            for ($w = 0; $w -lt 60 -and (Uninstall-Entries | Where-Object PSChildName -EQ $e.PSChildName); $w++) { Start-Sleep -Seconds 2 }
        }
        Start-Sleep -Seconds 3
    }
    $uninstallSteps += $Run.Steps[-1]
}

# 7. R: reference entries still present after the classic uninstall.
$remaining = @($script:reference | Where-Object {
        if ($_ -like 'HKEY_*') { Test-Path "Registry::$_" } else { Test-Path -LiteralPath $_ }
    })
$remaining | Set-Content "$out\restes-reels.txt" -Encoding utf8

# 8. Read-only scan with the engine (same as the « Analyser les restes » button). Its
#    inventory diff shows that it changes nothing but CleanSlate's own logs.
$script:scanCode = $null
Invoke-Step 'Analyse CleanSlate (scan --dry-run, lecture seule)' {
    # Invoke-Program: separate arguments, raw JSON output, throws if the CLI cannot start.
    $script:scanCode = Invoke-Program $cli @('scan', '--dry-run', '--json') "$out\scan.json" "$out\scan-progression.txt"
    Write-Host "scan : code de sortie $($script:scanCode)"
    $appsCode = Invoke-Program $cli @('apps', '--json') "$out\apps.json" "$out\apps-err.txt"
    Write-Host "apps : code de sortie $appsCode"
}
# An empty analysis must never be reported as a measurement. The CLI that could not start
# is a failure of the test script; a CLI that ran and failed is a result of CleanSlate.
$scanStep = $Run.Steps[-1]
if ($scanStep.Failure -or $script:scanCode -ne 0 -or -not (Test-Path "$out\scan.json") -or (Get-Item "$out\scan.json").Length -eq 0) {
    $measure = if ($scanStep.Failure) { "échec du script : $($scanStep.Failure)" } else { "cleanslate-cli : code de sortie $($script:scanCode), voir scan-progression.txt" }
    Write-Report 'Scénario des 5 applications — ÉCHEC' $meta @(
        [pscustomobject]@{ Name = 'Analyse produite'; Measure = $measure; Status = $(if ($scanStep.Failure) { 'script' } else { 'ko' }) }
    ) ([ordered]@{ 'Installeurs vérifiés' = $verified })
    Stop-Run
    Start-Process "$out\rapport.html"
    exit 1
}
$report = Get-Content "$out\scan.json" -Raw -Encoding utf8 | ConvertFrom-Json
$installed = Get-Content "$out\apps.json" -Raw -Encoding utf8 | ConvertFrom-Json
$items = @($report.groups | ForEach-Object { $_.items })

# Criterion 2: a real leftover is found if CleanSlate proposes it or one of its parents.
$found = @(); $missed = @()
foreach ($r in $remaining) {
    $nr = Normalize $r
    $hit = $items | Where-Object { $ni = Normalize $_.location; $nr -eq $ni -or $nr.StartsWith("$ni\") } | Select-Object -First 1
    if ($hit) { $found += $r } else { $missed += $r }
}

# Criterion 3: a prechecked (High) item must not belong to an installed application or to Windows.
$locations = @($installed | Where-Object install_location | ForEach-Object { Normalize $_.install_location })
$high = @($items | Where-Object confidence -EQ 'high')
$wrong = @($high | Where-Object {
        $l = Normalize $_.location
        $l -match 'mozilla|firefox' -or
        $l -match '\\(microsoft|windows|common files|packages)(\\|$)' -or
        ($locations | Where-Object { $l -eq $_ -or $l.StartsWith("$_\") -or $_.StartsWith("$l\") })
    })

$ratio = if ($remaining.Count) { [math]::Round(100.0 * $found.Count / $remaining.Count, 1) } else { 100 }
# An application never started wrote no data: the measure is incomplete, not « atteint ».
$notStarted = @($launchSteps | Where-Object Failure | ForEach-Object { $_.Name -replace '^Premier lancement de ', '' })
# An uninstall that did not end leaves R unknown (D59).
$notUninstalled = @($uninstallSteps | Where-Object Failure | ForEach-Object { $_.Name -replace '^Désinstallation classique de ', '' })
$criterion2 = if ($notStarted.Count -or $notUninstalled.Count) { 'script' } elseif ($ratio -ge 90) { 'ok' } else { 'ko' }
$measure2 = "T / R = $($found.Count) / $($remaining.Count) ($ratio %)" +
    $(if ($notStarted.Count) { " ; mesure incomplète : $($notStarted -join ', ') jamais lancé(s)" } else { '' }) +
    $(if ($notUninstalled.Count) { " ; mesure incomplète : désinstallation de $($notUninstalled -join ', ') non terminée" } else { '' })

# Criterion 4 (D58): each real leftover found is quarantined by CleanSlate (sandbox CLI, on
# the item it proposed), checked gone, restored, and compared with its state before: files
# by path, size, date and SHA-256; keys by their .reg export, values sorted.
$restoreChecks = @()
foreach ($r in $found) {
    $nr = Normalize $r
    $item = $items | Where-Object { $ni = Normalize $_.location; $nr -eq $ni -or $nr.StartsWith("$ni\") } | Select-Object -First 1
    $location = [string]$item.location
    $isKey = $location -match '^HK'
    $state = if ($isKey) { { RegState $location } } else { { FileState $location } }
    $ctx = @{ Before = (& $state); Session = $null; Gone = $false; After = $null }
    Invoke-Step "Critère 4 : mise en quarantaine de $location" {
        $ctx.Session = Invoke-CleanSlateCli $SandboxCli @('sandbox', 'quarantine', $location)
        Start-Sleep -Seconds 1
        $ctx.Gone = if ($isKey) { -not (Test-Path "Registry::$location") } else { -not (Test-Path -LiteralPath $location) }
        Write-Host "retiré : $($ctx.Gone)"
    }
    $qStep = $Run.Steps[-1]; $rStep = $null
    if ($ctx.Session) {
        Invoke-Step "Critère 4 : restauration de $location" {
            $restored = Invoke-CleanSlateCli $SandboxCli @('sandbox', 'restore', $ctx.Session.id)
            if ($restored -and $restored.errors.Count) { Write-Warning ($restored.errors | ConvertTo-Json -Compress) }
            Start-Sleep -Seconds 1
            $ctx.After = & $state
        }
        $rStep = $Run.Steps[-1]
    }
    $same = [bool]($ctx.Session -and $ctx.Gone -and ($ctx.Before -eq $ctx.After))
    if ($ctx.Session -and -not $same) {
        $slug = ($location -replace '\W+', '_')
        Set-Content "$out\critere4-$slug-avant.txt" $ctx.Before -Encoding utf8
        Set-Content "$out\critere4-$slug-apres.txt" $ctx.After -Encoding utf8
    }
    $failure = if ($qStep.Failure) { $qStep } elseif ($rStep -and $rStep.Failure) { $rStep } else { $null }
    $restoreChecks += [pscustomobject]@{ Location = $location; Same = $same; Gone = $ctx.Gone; Session = $ctx.Session.id; Status = (Status $same $failure) }
}
$criterion4 = if (-not $restoreChecks.Count) { 'script' }
elseif ($restoreChecks | Where-Object Status -EQ 'script') { 'script' }
elseif ($restoreChecks | Where-Object Status -EQ 'ko') { 'ko' }
else { 'ok' }
$measure4 = if ($restoreChecks.Count) { "$(@($restoreChecks | Where-Object Same).Count) / $($restoreChecks.Count) reste(s) mis en quarantaine puis restauré(s) à l'identique" } else { 'aucun reste retrouvé à restaurer' }
$meta += "Analyse : $([math]::Round($report.duration_ms / 1000, 1)) s · $($items.Count) éléments proposés, dont $($high.Count) précochés"
Write-Report 'Scénario des 5 applications — résultats' $meta @(
    [pscustomobject]@{ Name = 'Installeurs conformes au manifeste'; Measure = "$($script:installers.Count) SHA-256 vérifiés avant installation"; Status = 'ok' },
    [pscustomobject]@{ Name = '≥ 90 % des restes retrouvés (critère 2)'; Measure = $measure2; Status = $criterion2 },
    [pscustomobject]@{ Name = "Zéro élément d'une application installée ou du système précoché (critère 3)"; Measure = "$($wrong.Count) précoché(s) fautif(s)"; Status = $(if ($wrong.Count -eq 0) { 'ok' } else { 'ko' }) },
    [pscustomobject]@{ Name = 'Analyse sous 60 s (ENF-05)'; Measure = "$([math]::Round($report.duration_ms / 1000, 1)) s"; Status = $(if ($report.duration_ms -lt 60000) { 'ok' } else { 'ko' }) },
    [pscustomobject]@{ Name = 'Session restaurable à l''identique (critère 4)'; Measure = $measure4; Status = $criterion4 },
    [pscustomobject]@{ Name = 'Critère 4 dans l''interface (facultatif)'; Measure = 'tests-acceptation.md § 4.2, étapes 4 à 7, dans CleanSlate (Bureau), mode simulation désactivé'; Status = 'manuel' }
) ([ordered]@{
        'Restes réels manqués'                = $missed
        'Éléments précochés fautifs'          = @($wrong | ForEach-Object { "$($_.location) ($($_.guessed_app))" })
        'Tous les éléments précochés (à relire)' = @($high | ForEach-Object { "$($_.location) — $($_.reasons -join ' ; ')" })
        'Restes réels (R)'                    = $remaining
        'Critère 4 : quarantaine puis restauration' = @($restoreChecks | ForEach-Object { "$($_.Location) — retiré : $(if ($_.Gone) { 'oui' } else { 'non' }) ; identique : $(if ($_.Same) { 'oui' } else { 'non' }) ; session $($_.Session)" })
        'Installeurs vérifiés'                = $verified
    })

Stop-Run
Start-Process "$out\rapport.html"
Watch-Session
exit 0
