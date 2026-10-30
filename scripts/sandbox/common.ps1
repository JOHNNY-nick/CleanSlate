<#
.SYNOPSIS
    Functions shared by the inside-*.ps1 scripts (dot-sourced). Windows PowerShell 5.1.

.DESCRIPTION
    - Assert-Sandbox: the three conditions of the CLI guard (D53), checked again here;
    - Start-Run / Stop-Run: one results folder per run, with the transcript of the session;
    - Invoke-Step: a timestamped step, with an inventory of the system before and after
      it and a diff file (inventaires\ and diffs\ of the results folder);
    - Test-Installer: SHA-256 of the installers against manifeste.json;
    - Write-Report: rapport.md and rapport.html from the same data.
    Paths: the repository is shared read-only at C:\CleanSlate\repo; the only writable
    shared folder is C:\CleanSlate\results (artifacts\sandbox\results on the host).
#>

$Repo = 'C:\CleanSlate\repo'
$Results = 'C:\CleanSlate\results'
$Tools = "$Repo\artifacts\sandbox\tools"
$TokenFile = "$Repo\artifacts\sandbox\run\token.txt"

# State of the current run, filled by Start-Run and Invoke-Step.
$Run = @{ Out = $null; Journal = $null; Scenario = $null; Started = $null; Index = 0; Inventory = $null; Steps = @() }

# Same three conditions as the guard of cleanslate-cli (sandbox.rs, D53): the Sandbox
# account, the token set by the .wsb, and the same token in the read-only shared file.
function Assert-Sandbox {
    $reasons = @()
    if ($env:USERNAME -ne 'WDAGUtilityAccount') { $reasons += "compte $env:USERNAME (attendu : WDAGUtilityAccount)" }
    $token = [string]$env:CLEANSLATE_SANDBOX_RUN
    if ($token -notmatch '^[0-9a-fA-F]{64}$') { $reasons += 'variable CLEANSLATE_SANDBOX_RUN absente ou invalide' }
    $file = if (Test-Path -LiteralPath $TokenFile) { (Get-Content -LiteralPath $TokenFile -Raw).Trim() } else { $null }
    if (-not $file) { $reasons += "jeton absent ($TokenFile)" }
    elseif ($file -ne $token) { $reasons += 'jeton différent de celui écrit par prepare.ps1' }
    if ($reasons.Count) {
        Write-Error ("Ce script ne tourne que dans le Windows Sandbox préparé par prepare.ps1 : " + ($reasons -join ' ; '))
        exit 1
    }
}

# One line of journal.txt, written and closed at once: nothing is lost if the Sandbox
# closes abruptly (the PowerShell transcript, transcription.txt, is only complete at the end).
function Write-Log([string] $text, [string] $color) {
    $line = "[$(Get-Date -Format 'HH:mm:ss')] $text"
    if ($Run.Journal) {
        try { [IO.File]::AppendAllText($Run.Journal, $line + "`r`n", (New-Object System.Text.UTF8Encoding $false)) } catch { }
    }
    if ($color) { Write-Host $line -ForegroundColor $color } else { Write-Host $line }
}

# Programs whose crashes are dumped by Windows Error Reporting into results\…\dumps.
$DumpedPrograms = 'CleanSlate.App.exe', 'cleanslate-cli.exe', 'cleanslate-cli-sandbox.exe'

# Traces written straight into the shared results folder, so that they survive the Sandbox:
# - CleanSlate's logs (engine and crash log of the interface): CLEANSLATE_LOG_DIR, for this
#   process and for the user, so that CleanSlate started from the Desktop gets it too;
# - crash dumps of CleanSlate and of the CLI (WER LocalDumps, full dumps, 5 at most).
function Enable-Traces {
    $logs = "$($Run.Out)\cleanslate-logs"
    $dumps = "$($Run.Out)\dumps"
    New-Item -ItemType Directory -Force $logs, $dumps | Out-Null
    $env:CLEANSLATE_LOG_DIR = $logs
    [Environment]::SetEnvironmentVariable('CLEANSLATE_LOG_DIR', $logs, 'User')
    Write-Log "Journaux de CleanSlate : $logs (CLEANSLATE_LOG_DIR)"
    try {
        Set-Service WerSvc -StartupType Manual -ErrorAction Stop
        Start-Service WerSvc -ErrorAction Stop
    }
    catch { Write-Log "Service Windows Error Reporting : $($_.Exception.Message)" }
    foreach ($exe in $DumpedPrograms) {
        $key = "HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\$exe"
        New-Item -Path $key -Force | Out-Null
        New-ItemProperty -Path $key -Name DumpFolder -PropertyType ExpandString -Value $dumps -Force | Out-Null
        New-ItemProperty -Path $key -Name DumpType -PropertyType DWord -Value 2 -Force | Out-Null
        New-ItemProperty -Path $key -Name DumpCount -PropertyType DWord -Value 5 -Force | Out-Null
    }
    Write-Log "Vidages de plantage (WER LocalDumps) : $dumps"
}

# Errors of the Application event log since the start of the run (.NET Runtime, Application
# Error, Windows Error Reporting, Application Hang), full messages, into evenements.txt.
# Returns how many there are.
function Save-Events {
    $events = @(Get-WinEvent -FilterHashtable @{
            LogName = 'Application'; StartTime = $Run.Started
            ProviderName = '.NET Runtime', 'Application Error', 'Windows Error Reporting', 'Application Hang'
        } -ErrorAction SilentlyContinue | Sort-Object TimeCreated)
    $text = @("Événements d'erreur depuis $($Run.Started.ToString('HH:mm:ss')) (relevé à $(Get-Date -Format 'HH:mm:ss')) : $($events.Count)", '')
    $text += $events | ForEach-Object { "==== $($_.TimeCreated.ToString('HH:mm:ss')) $($_.ProviderName) $($_.Id) ($($_.LevelDisplayName))"; $_.Message; '' }
    try { [IO.File]::WriteAllLines("$($Run.Out)\evenements.txt", [string[]]$text, (New-Object System.Text.UTF8Encoding $false)) } catch { }
    $events.Count
}

function Start-Run([string] $scenario) {
    $Run.Scenario = $scenario
    $Run.Started = Get-Date
    $Run.Out = Join-Path $Results ($scenario + '-' + $Run.Started.ToString('yyyyMMdd-HHmmss'))
    New-Item -ItemType Directory -Force $Run.Out, "$($Run.Out)\inventaires", "$($Run.Out)\diffs" | Out-Null
    $Run.Journal = "$($Run.Out)\journal.txt"
    Start-Transcript -Path "$($Run.Out)\transcription.txt" -IncludeInvocationHeader | Out-Null
    Write-Log "Début du scénario $scenario ; résultats : $($Run.Out)"
    Enable-Traces
}

function Stop-Run {
    $count = Save-Events
    Write-Log "Fin du scénario $($Run.Scenario) ($([math]::Round(((Get-Date) - $Run.Started).TotalMinutes, 1)) min) ; $count événement(s) d'erreur dans evenements.txt"
    Stop-Transcript | Out-Null
}

# After the scenario, for the manual checks: every 10 s, copies the error events again and
# reports any new crash dump or crash log, until this console or the Sandbox is closed.
function Watch-Session {
    Write-Log 'Surveillance active pendant les vérifications à la main : événements, vidages et journaux de CleanSlate sont relevés toutes les 10 s dans le dossier de résultats. Fermez le Sandbox quand vous avez fini (Ctrl+C arrête seulement la surveillance).' 'Yellow'
    $seenEvents = -1; $seenFiles = @{}
    while ($true) {
        $count = Save-Events
        if ($count -ne $seenEvents) {
            if ($seenEvents -ge 0) { Write-Log "Nouvel événement d'erreur : $count au total (evenements.txt)" 'Red' }
            $seenEvents = $count
        }
        foreach ($f in Get-ChildItem "$($Run.Out)\dumps", "$($Run.Out)\cleanslate-logs" -Filter '*' -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*.dmp' -or $_.Name -like 'crash.*' }) {
            if ($seenFiles[$f.FullName] -ne $f.Length) {
                $seenFiles[$f.FullName] = $f.Length
                Write-Log "Trace de plantage : $($f.FullName) ($($f.Length) octets)" 'Red'
            }
        }
        Start-Sleep -Seconds 10
    }
}

# Folders of the first two levels, without following junctions or symbolic links.
function Get-Folders([string[]] $roots) {
    foreach ($root in $roots) {
        if (-not $root -or -not (Test-Path -LiteralPath $root)) { continue }
        foreach ($d in Get-ChildItem -LiteralPath $root -Directory -Force -ErrorAction SilentlyContinue) {
            if ($d.Attributes -band [IO.FileAttributes]::ReparsePoint) { "$($d.FullName) (lien)"; continue }
            $d.FullName
            Get-ChildItem -LiteralPath $d.FullName -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object FullName
        }
    }
}

# Keys of the first two levels (Vendor\Product); Classes is listed without its children.
function Get-Keys([string] $root) {
    foreach ($k in Get-ChildItem $root -ErrorAction SilentlyContinue) {
        $k.Name
        if ($k.PSChildName -ne 'Classes') { Get-ChildItem $k.PSPath -ErrorAction SilentlyContinue | ForEach-Object Name }
    }
}

function Short([string] $name) { $name -replace '^HKEY_CURRENT_USER', 'HKCU' -replace '^HKEY_LOCAL_MACHINE', 'HKLM' }

# Inventory of the system: one sorted list of lines per category.
function Get-Inventory {
    $uninstall = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall'
    $apps = foreach ($u in $uninstall) {
        Get-ChildItem $u -ErrorAction SilentlyContinue | ForEach-Object {
            $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
            "$(Short $_.Name) | $($p.DisplayName) | $($p.DisplayVersion)"
        }
    }
    $apps = @($apps) + @(Get-AppxPackage -ErrorAction SilentlyContinue | ForEach-Object { "Appx | $($_.PackageFullName)" })
    $startMenu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs", "$env:ProgramData\Microsoft\Windows\Start Menu\Programs"
    [ordered]@{
        'Applications'           = $apps
        'AppData'                = Get-Folders @($env:APPDATA, $env:LOCALAPPDATA, "$env:USERPROFILE\AppData\LocalLow")
        'ProgramData'            = Get-Folders @($env:ProgramData)
        'Program Files'          = Get-Folders @($env:ProgramFiles, ${env:ProgramFiles(x86)}, "$env:LOCALAPPDATA\Programs")
        'Menu Démarrer'          = foreach ($m in $startMenu) { Get-ChildItem -LiteralPath $m -Recurse -Force -ErrorAction SilentlyContinue | ForEach-Object FullName }
        'HKCU\Software'          = Get-Keys 'HKCU:\Software' | ForEach-Object { Short $_ }
        'HKLM\SOFTWARE'          = Get-Keys 'HKLM:\SOFTWARE' | ForEach-Object { Short $_ }
        'Tâches planifiées'      = Get-ScheduledTask -ErrorAction SilentlyContinue | ForEach-Object { "$($_.TaskPath)$($_.TaskName) | $($_.State)" }
        'Services'               = Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) | $($_.StartMode) | $($_.PathName)" }
    }
}

# Lines added and removed per category; writes the diff file of the step.
function Compare-Inventory($before, $after, [string] $file, [string] $title) {
    $added = 0; $removed = 0
    $text = @($title, '')
    foreach ($category in $after.Keys) {
        $old = New-Object 'System.Collections.Generic.HashSet[string]' ([string[]]@($before[$category] | Where-Object { $_ }), [StringComparer]::OrdinalIgnoreCase)
        $new = New-Object 'System.Collections.Generic.HashSet[string]' ([string[]]@($after[$category] | Where-Object { $_ }), [StringComparer]::OrdinalIgnoreCase)
        $plus = @($new | Where-Object { -not $old.Contains($_) } | Sort-Object)
        $minus = @($old | Where-Object { -not $new.Contains($_) } | Sort-Object)
        $added += $plus.Count; $removed += $minus.Count
        $text += "== $category : +$($plus.Count) / -$($minus.Count)"
        $text += $plus | ForEach-Object { "+ $_" }
        $text += $minus | ForEach-Object { "- $_" }
        $text += ''
    }
    $text | Set-Content -LiteralPath $file -Encoding utf8
    @{ Added = $added; Removed = $removed }
}

function Save-Inventory($inventory, [string] $name) {
    $inventory | ConvertTo-Json -Depth 3 | Set-Content -LiteralPath "$($Run.Out)\inventaires\$name.json" -Encoding utf8
}

# A timestamped step. Unless -NoInventory, the system is inventoried before (the previous
# "after" is reused) and after the step, and diffs\<n>-<name>.txt lists the differences.
# The action runs in a child scope: it assigns results with $script:name.
#  Its locals are prefixed with "step" so that they never hide a variable of the script.
function Invoke-Step([string] $StepName, [scriptblock] $StepAction, [switch] $NoInventory) {
    $Run.Index++
    $stepNo = '{0:D2}' -f $Run.Index
    $stepSlug = ($StepName -replace '[^\w]+', '-').Trim('-').ToLowerInvariant()
    $stepStart = Get-Date
    Write-Log "=== $stepNo. $StepName ===" 'Cyan'
    if (-not $NoInventory -and -not $Run.Inventory) {
        Write-Log '  inventaire initial…'
        $Run.Inventory = Get-Inventory
        Save-Inventory $Run.Inventory "$stepNo-avant"
    }
    # Every stream of the action (output, errors, warnings, Write-Host) goes line by line to
    # journal.txt. An exception is a failure of the test script, logged with its position.
    $stepFailure = $null
    try { & $StepAction *>&1 | ForEach-Object { Write-Log "  $_" } }
    catch {
        $stepFailure = $_.Exception.Message
        Write-Log "  ÉCHEC DU SCRIPT à l'étape $stepNo : $stepFailure" 'Red'
        Write-Log "  $($_.InvocationInfo.PositionMessage -replace "`r?`n", "`r`n  ")"
        Write-Log "  $($_.ScriptStackTrace -replace "`r?`n", "`r`n  ")"
    }
    [void](Save-Events)
    $stepDiff = $null; $stepCounts = @{ Added = $null; Removed = $null }
    if (-not $NoInventory) {
        $stepAfter = Get-Inventory
        Save-Inventory $stepAfter "$stepNo-apres"
        $stepDiff = "diffs\$stepNo-$stepSlug.txt"
        $stepCounts = Compare-Inventory $Run.Inventory $stepAfter "$($Run.Out)\$stepDiff" "Étape $stepNo — $StepName (début $($stepStart.ToString('HH:mm:ss')))"
        $Run.Inventory = $stepAfter
    }
    $stepEnd = Get-Date
    $stepSeconds = [math]::Round(($stepEnd - $stepStart).TotalSeconds)
    $stepSummary = if ($stepDiff) { " ; +$($stepCounts.Added) / -$($stepCounts.Removed) dans l'inventaire ($stepDiff)" } else { '' }
    Write-Log "fin de l'étape $stepNo ($stepSeconds s)$stepSummary"
    $Run.Steps += [pscustomobject]@{
        Number = $stepNo; Name = $StepName; Start = $stepStart.ToString('HH:mm:ss'); Seconds = $stepSeconds
        Added = $stepCounts.Added; Removed = $stepCounts.Removed; Diff = $stepDiff; Failure = $stepFailure
    }
}

# Checks the installers of a scenario against manifeste.json. Returns one row per
# installer; Ok is false when the file is missing or its SHA-256 differs.
function Test-Installer([string] $manifestPath, [string] $installers, [string] $scenario) {
    $manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
    foreach ($entry in $manifest.telechargements | Where-Object { ($_.scenario -split ',\s*') -contains $scenario }) {
        $path = Join-Path (Join-Path $installers $entry.id) $entry.fichier
        $hash = if (Test-Path -LiteralPath $path) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { $null }
        [pscustomobject]@{
            Id = $entry.id; Name = $entry.nom; Version = $entry.version; Path = $path; Url = $entry.url
            Install = $entry.installation; Expected = $entry.sha256; Actual = $hash; Ok = ($hash -eq $entry.sha256)
        }
    }
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

# One argument quoted by the rules of CommandLineToArgvW and the MSVC runtime (Rust's
# std::env::args): quotes when it is empty or holds a space, a tab or a quote; a quote is
# escaped as \" and the backslashes just before a quote (or the closing quote) are doubled.
function ConvertTo-ArgumentText([string] $argument) {
    if ($argument.Length -and $argument -notmatch '[\s"]') { return $argument }
    $text = New-Object System.Text.StringBuilder
    [void]$text.Append('"')
    $backslashes = 0
    foreach ($c in $argument.ToCharArray()) {
        if ($c -eq '\') { $backslashes++; continue }
        if ($c -eq '"') { [void]$text.Append([char]'\', 2 * $backslashes + 1) }
        elseif ($backslashes) { [void]$text.Append([char]'\', $backslashes) }
        $backslashes = 0
        [void]$text.Append($c)
    }
    [void]$text.Append([char]'\', 2 * $backslashes)
    [void]$text.Append('"')
    $text.ToString()
}

# The command line of separate arguments. Start-Process -ArgumentList joins an array with
# spaces without quoting (Windows PowerShell 5.1 and PowerShell 7): "CleanSlate Test App"
# would arrive as three arguments. It always receives this single string instead.
function Join-CommandLine([string[]] $arguments) {
    (@($arguments) | ForEach-Object { ConvertTo-ArgumentText $_ }) -join ' '
}

# Runs a program with separate arguments, waits for it, and returns its exit code.
# stdout and stderr go to files as raw bytes (no console re-encoding of the JSON).
# Throws when the program cannot be started: that is a failure of the test script.
function Invoke-Program([string] $program, [string[]] $arguments, [string] $stdout, [string] $stderr) {
    if (-not (Test-Path -LiteralPath $program -PathType Leaf)) { throw "programme introuvable : $program" }
    $options = @{ FilePath = $program; NoNewWindow = $true; Wait = $true; PassThru = $true; RedirectStandardOutput = $stdout; RedirectStandardError = $stderr }
    $line = Join-CommandLine $arguments
    if ($line) { $options.ArgumentList = $line }
    $p = Start-Process @options
    $p.ExitCode
}

function Html([string] $text) { [System.Net.WebUtility]::HtmlEncode($text) }

# Writes rapport.md and rapport.html in the results folder.
#   $criteria: objects with Name, Measure, Status: 'ok', 'ko' (CleanSlate did not give the
#   expected result), 'script' (the test itself failed: no conclusion on CleanSlate), 'manuel'
#   $sections: ordered dictionary title -> lines (each line becomes a list item)
function Write-Report([string] $title, [string[]] $meta, $criteria, $sections) {
    $label = @{ ok = 'atteint'; ko = 'NON atteint'; script = 'échec du script'; manuel = 'à faire à la main' }
    # A failed step of the test script is never a result of CleanSlate: it gets its own line.
    $failed = @($Run.Steps | Where-Object Failure)
    if ($failed.Count) {
        $criteria = @([pscustomobject]@{ Name = 'Script de test exécuté sans erreur'; Measure = "$($failed.Count) étape(s) en échec : $(($failed | ForEach-Object Number) -join ', ') (voir journal.txt)"; Status = 'script' }) + @($criteria)
    }
    $meta = @($meta) + 'Légende : « NON atteint » = CleanSlate n''a pas donné le résultat attendu ; « échec du script » = le test lui-même n''a pas pu s''exécuter, aucune conclusion sur CleanSlate.'
    $md = @("# $title", '') + ($meta | ForEach-Object { "- $_" }) + @('')
    if ($criteria) {
        $md += @('| Vérification | Mesure | Résultat |', '| --- | --- | --- |')
        $md += $criteria | ForEach-Object { "| $($_.Name) | $($_.Measure) | $($label[$_.Status]) |" }
        $md += ''
    }
    foreach ($s in $sections.Keys) {
        $md += @("## $s", '')
        $lines = @($sections[$s])
        $md += if ($lines.Count) { $lines | ForEach-Object { "- $_" } } else { '- aucun' }
        $md += ''
    }
    $md += @('## Étapes', '', '| # | Étape | Début | Durée | Inventaire | Diff |', '| --- | --- | --- | --- | --- | --- |')
    $md += $Run.Steps | ForEach-Object {
        $inv = if ($_.Diff) { "+$($_.Added) / -$($_.Removed)" } else { '—' }
        $state = if ($_.Failure) { " (échec du script : $($_.Failure))" } else { '' }
        "| $($_.Number) | $($_.Name)$state | $($_.Start) | $($_.Seconds) s | $inv | $(if ($_.Diff) { $_.Diff } else { '—' }) |"
    }
    $md += @('', 'Journal complet : journal.txt ; inventaires bruts : inventaires\ ; différences : diffs\.')
    $md | Set-Content -LiteralPath "$($Run.Out)\rapport.md" -Encoding utf8

    $h = New-Object System.Text.StringBuilder
    $style = @'
:root { --bg: #ffffff; --fg: #1b1b1f; --muted: #5f6168; --line: #d9dae0; --ok: #1f7a3a; --ko: #b3261e; --manual: #8a5a00; --head: #f3f4f7; }
@media (prefers-color-scheme: dark) { :root { --bg: #17181c; --fg: #e8e8ec; --muted: #a2a4ad; --line: #34363d; --ok: #6fd38b; --ko: #ff8a80; --manual: #f2c063; --head: #22242a; } }
body { background: var(--bg); color: var(--fg); font: 15px/1.5 "Segoe UI", system-ui, sans-serif; margin: 0 auto; max-width: 1100px; padding: 24px 16px; }
h1 { font-size: 24px; margin: 0 0 8px; } h2 { font-size: 18px; margin: 28px 0 8px; }
ul.meta { color: var(--muted); padding-left: 18px; }
table { border-collapse: collapse; width: 100%; margin: 8px 0; } th, td { border: 1px solid var(--line); padding: 6px 8px; text-align: left; vertical-align: top; }
th { background: var(--head); } code { font-family: Consolas, monospace; font-size: 13px; word-break: break-all; }
.ok { color: var(--ok); font-weight: 600; } .ko { color: var(--ko); font-weight: 600; } .manuel { color: var(--manual); font-weight: 600; } .script { color: var(--muted); font-weight: 600; font-style: italic; }
.wrap { overflow-x: auto; }
'@
    [void]$h.Append("<!doctype html><html lang=`"fr`"><head><meta charset=`"utf-8`"><meta name=`"viewport`" content=`"width=device-width, initial-scale=1`"><title>$(Html $title)</title><style>$style</style></head><body>")
    [void]$h.Append("<h1>$(Html $title)</h1><ul class=`"meta`">")
    foreach ($m in $meta) { [void]$h.Append("<li>$(Html $m)</li>") }
    [void]$h.Append('</ul>')
    if ($criteria) {
        [void]$h.Append('<div class="wrap"><table><tr><th>Vérification</th><th>Mesure</th><th>Résultat</th></tr>')
        foreach ($c in $criteria) {
            [void]$h.Append("<tr><td>$(Html $c.Name)</td><td>$(Html $c.Measure)</td><td class=`"$($c.Status)`">$(Html $label[$c.Status])</td></tr>")
        }
        [void]$h.Append('</table></div>')
    }
    foreach ($s in $sections.Keys) {
        [void]$h.Append("<h2>$(Html $s)</h2><ul>")
        $lines = @($sections[$s])
        if (-not $lines.Count) { [void]$h.Append('<li>aucun</li>') }
        foreach ($l in $lines) { [void]$h.Append("<li><code>$(Html $l)</code></li>") }
        [void]$h.Append('</ul>')
    }
    [void]$h.Append('<h2>Étapes</h2><div class="wrap"><table><tr><th>#</th><th>Étape</th><th>Début</th><th>Durée</th><th>Inventaire</th><th>Diff</th></tr>')
    foreach ($st in $Run.Steps) {
        $inv = if ($st.Diff) { "+$($st.Added) / -$($st.Removed)" } else { '—' }
        $link = if ($st.Diff) { "<a href=`"$(Html ($st.Diff -replace '\\', '/'))`">$(Html $st.Diff)</a>" } else { '—' }
        $name = if ($st.Failure) { "$(Html $st.Name) <span class=`"script`">(échec du script : $(Html $st.Failure))</span>" } else { Html $st.Name }
        [void]$h.Append("<tr><td>$($st.Number)</td><td>$name</td><td>$($st.Start)</td><td>$($st.Seconds) s</td><td>$inv</td><td>$link</td></tr>")
    }
    [void]$h.Append('</table></div><p>Journal complet : <a href="journal.txt">journal.txt</a> ; inventaires bruts : dossier <code>inventaires</code> ; différences : dossier <code>diffs</code>.</p></body></html>')
    [IO.File]::WriteAllText("$($Run.Out)\rapport.html", $h.ToString(), (New-Object System.Text.UTF8Encoding $false))
}
