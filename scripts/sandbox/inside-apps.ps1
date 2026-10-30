<#
.SYNOPSIS
    Acceptance scenario of the 5 applications (tests-acceptation.md § 4, cahier § 6.3),
    run INSIDE Windows Sandbox by the logon command written by prepare.ps1.

.DESCRIPTION
    Automates what does not need the interface, and measures criteria 2 and 3:
    1. installs the applications silently from C:\CleanSlate\installers;
    2. starts each one once so that it writes its data, then closes it;
    3. records the reference (folders and keys of each application, § 4.1.3);
    4. uninstalls the 5 applications with their own uninstaller (Firefox stays);
    5. R = reference entries still present; scan with cleanslate-cli (read-only);
    6. writes rapport.md: T / R and the prechecked (High) items that are wrong.
    Criterion 4 (identical restore) stays manual, in the app copied to the Desktop.
    Refuses to run outside the Sandbox: it installs and uninstalls software.
#>
$ErrorActionPreference = 'Continue'
if ($env:USERNAME -ne 'WDAGUtilityAccount') {
    Write-Error 'Ce script ne tourne que dans Windows Sandbox (compte WDAGUtilityAccount).'
    exit 1
}

$base = 'C:\CleanSlate'
$desktop = [Environment]::GetFolderPath('Desktop')
$out = Join-Path "$base\results" (Get-Date -Format 'yyyyMMdd-HHmmss')
New-Item -ItemType Directory -Force $out | Out-Null
Start-Transcript -Path "$out\journal.txt" | Out-Null

# Applications of the scenario: name in the Uninstall keys, main program, data names (§ 4.1.3).
$apps = @(
    @{ Name = 'VLC'; Pattern = 'VLC'; Main = 'VideoLAN\VLC\vlc.exe'; Wait = 25 },
    @{ Name = '7-Zip'; Pattern = '7-Zip'; Main = '7-Zip\7zFM.exe'; Wait = 15 },
    @{ Name = 'Notepad++'; Pattern = 'Notepad\+\+'; Main = 'Notepad++\notepad++.exe'; Wait = 15 },
    @{ Name = 'Audacity'; Pattern = 'Audacity'; Main = 'Audacity\Audacity.exe'; Wait = 30 },
    @{ Name = 'GIMP'; Pattern = 'GIMP'; Main = 'GIMP*\bin\gimp-[0-9]*.exe'; Wait = 90 }
)
$witness = @{ Name = 'Firefox'; Main = 'Mozilla Firefox\firefox.exe'; Wait = 25 }
$dataNames = 'vlc', 'VideoLAN', '7-Zip', 'Notepad++', 'Audacity', 'GIMP'

function Step([string] $text) { Write-Host "`n=== $text ===" -ForegroundColor Cyan }

# Start-Process refuses an empty argument list.
function Run([string] $program, [string] $arguments) {
    if ($arguments) { Start-Process $program $arguments -Wait -PassThru } else { Start-Process $program -Wait -PassThru }
}

# Splits "C:\x\unins.exe" /a /b or C:\x\unins.exe /a into program and arguments (no shell).
function Split-Command([string] $line) {
    $line = $line.Trim()
    if ($line.StartsWith('"')) {
        $end = $line.IndexOf('"', 1)
        return @($line.Substring(1, $end - 1), $line.Substring($end + 1).Trim())
    }
    $i = $line.ToLower().IndexOf('.exe')
    if ($i -lt 0) { return @($line, '') }
    return @($line.Substring(0, $i + 4), $line.Substring($i + 4).Trim())
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

# 1. CleanSlate on the Desktop for the manual steps (the shared folder is read-only).
Step 'Copie de CleanSlate sur le Bureau'
Copy-Item "$base\app" "$desktop\CleanSlate" -Recurse -Force

# 2. Silent installation, from the winget manifest of each installer.
Step 'Installation des applications'
foreach ($dir in Get-ChildItem "$base\installers" -Directory) {
    $yaml = Get-ChildItem $dir.FullName -Filter *.yaml | Select-Object -First 1
    $file = Get-ChildItem $dir.FullName -File | Where-Object Extension -ne '.yaml' | Select-Object -First 1
    if (-not $yaml -or -not $file) { Write-Warning "$($dir.Name) : installeur absent"; continue }
    $text = Get-Content $yaml.FullName -Raw
    $type = [regex]::Match($text, '(?m)^\s*-?\s*InstallerType:\s*(\S+)').Groups[1].Value
    $silent = [regex]::Match($text, '(?m)^\s*Silent:\s*(.+)$').Groups[1].Value.Trim().Trim("'", '"')
    if ($type -in 'msi', 'wix') {
        $program = 'msiexec.exe'; $arguments = "/i `"$($file.FullName)`" /qn /norestart"
    }
    else {
        $program = $file.FullName
        $arguments = if ($silent) { $silent } elseif ($type -eq 'inno') { '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /ALLUSERS' } else { '/S' }
    }
    Write-Host "$($dir.Name) ($type) : $program $arguments"
    $p = Run $program $arguments
    Write-Host "  code $($p.ExitCode)"
}

# 3. One start each, so that the applications write their data, then a normal close.
Step 'Premier lancement de chaque application'
foreach ($a in $apps + $witness) {
    $exe = Get-ChildItem (Join-Path $env:ProgramFiles $a.Main) -ErrorAction SilentlyContinue |
        Where-Object Name -NotMatch 'console' | Select-Object -First 1
    if (-not $exe) { Write-Warning "$($a.Name) : programme introuvable"; continue }
    Write-Host "$($a.Name) : $($exe.FullName) ($($a.Wait) s)"
    Start-Process $exe.FullName | Out-Null
    Start-Sleep -Seconds $a.Wait
    $name = [IO.Path]::GetFileNameWithoutExtension($exe.Name)
    Get-Process $name -ErrorAction SilentlyContinue | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep -Seconds 5
    Get-Process $name -ErrorAction SilentlyContinue | Stop-Process -Force
}

# 4. Reference: top-level folders and keys of each application (§ 4.1.3).
Step 'Relevé de référence'
$roots = $env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:APPDATA, $env:LOCALAPPDATA, "$env:LOCALAPPDATA\Programs", $env:ProgramData
$reference = foreach ($r in $roots) {
    Get-ChildItem $r -Directory -ErrorAction SilentlyContinue |
        Where-Object { $n = $_.Name; $dataNames | Where-Object { $n -like "*$_*" } } | ForEach-Object FullName
}
$reference += foreach ($k in 'HKCU:\Software', 'HKLM:\SOFTWARE', 'HKLM:\SOFTWARE\WOW6432Node') {
    Get-ChildItem $k -ErrorAction SilentlyContinue |
        Where-Object { $n = $_.PSChildName; $dataNames | Where-Object { $n -like "*$_*" } } | ForEach-Object Name
}
$reference = $reference | Sort-Object -Unique
$reference | Set-Content "$out\reference.txt" -Encoding utf8
Write-Host "$($reference.Count) entrées"

# 5. Classic uninstall with each application's own uninstaller (Firefox is kept).
Step 'Désinstallation classique'
foreach ($a in $apps) {
    foreach ($e in Uninstall-Entries | Where-Object DisplayName -Match $a.Pattern) {
        if ($e.WindowsInstaller -eq 1 -or $e.UninstallString -match 'msiexec') {
            $program = 'msiexec.exe'; $arguments = "/x $($e.PSChildName) /qn /norestart"
        }
        elseif ($e.QuietUninstallString) {
            $program, $arguments = Split-Command $e.QuietUninstallString
        }
        else {
            $program, $rest = Split-Command $e.UninstallString
            $quiet = if ($program -match 'unins\d*\.exe$') { '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' } else { '/S' }
            $arguments = "$rest $quiet".Trim()
        }
        Write-Host "$($e.DisplayName) : $program $arguments"
        Run $program $arguments | Out-Null
        # NSIS uninstallers copy themselves and return at once: wait for the entry to go.
        for ($i = 0; $i -lt 60 -and (Uninstall-Entries | Where-Object PSChildName -EQ $e.PSChildName); $i++) { Start-Sleep -Seconds 2 }
    }
}
Start-Sleep -Seconds 5

# 6. R: reference entries still present after the classic uninstall.
$remaining = $reference | Where-Object {
    if ($_ -like 'HKEY_*') { Test-Path "Registry::$_" } else { Test-Path -LiteralPath $_ }
}
$remaining | Set-Content "$out\restes-reels.txt" -Encoding utf8

# 7. Read-only scan with the engine (same as the « Analyser les restes » button).
Step 'Analyse CleanSlate (lecture seule)'
$cli = "$base\tools\cleanslate-cli.exe"
Start-Process $cli -ArgumentList 'scan', '--dry-run', '--json' -NoNewWindow -Wait `
    -RedirectStandardOutput "$out\scan.json" -RedirectStandardError "$out\scan-progression.txt"
Start-Process $cli -ArgumentList 'apps', '--json' -NoNewWindow -Wait `
    -RedirectStandardOutput "$out\apps.json" -RedirectStandardError "$out\apps-err.txt"
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
$lines = @(
    '# Scénario des 5 applications — résultats',
    '',
    "Date : $(Get-Date -Format 'dd/MM/yyyy HH:mm') · durée de l'analyse : $([math]::Round($report.duration_ms / 1000, 1)) s · $($items.Count) éléments proposés, dont $($high.Count) précochés",
    '',
    '| Critère (cahier § 6.3) | Mesure | Résultat |',
    '| --- | --- | --- |',
    "| ≥ 90 % des restes retrouvés | T / R = $($found.Count) / $($remaining.Count) ($ratio %) | $(if ($ratio -ge 90) { 'atteint' } else { 'NON atteint' }) |",
    "| Zéro élément d'une application installée ou du système précoché | $($wrong.Count) précoché(s) fautif(s) | $(if ($wrong.Count -eq 0) { 'atteint' } else { 'NON atteint' }) |",
    '| Session restaurable à l''identique | étapes 4 à 7 de tests-acceptation.md § 4.2, dans CleanSlate (Bureau) | à faire à la main |',
    '',
    '## Restes réels manqués',
    ''
) + ($(if ($missed.Count) { $missed | ForEach-Object { "- ``$_``" } } else { '- aucun' })) + @(
    '',
    '## Éléments précochés fautifs',
    ''
) + ($(if ($wrong.Count) { $wrong | ForEach-Object { "- ``$($_.location)`` ($($_.guessed_app))" } } else { '- aucun' })) + @(
    '',
    '## Tous les éléments précochés (à relire)',
    ''
) + ($high | ForEach-Object { "- ``$($_.location)`` — $($_.reasons -join ' ; ')" })
$lines | Set-Content "$out\rapport.md" -Encoding utf8

Step "Terminé : $out\rapport.md"
Stop-Transcript | Out-Null
Start-Process notepad.exe "$out\rapport.md"
