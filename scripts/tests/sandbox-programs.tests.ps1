<#
.SYNOPSIS
    Tests of Find-MainProgram (common.ps1, D58): finding the main program of each application
    of the Apps scenario, to start it once. Runs under Windows PowerShell 5.1 and PowerShell 7:
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/sandbox-programs.tests.ps1
        pwsh -NoProfile -File scripts/tests/sandbox-programs.tests.ps1

.DESCRIPTION
    Builds, in a temporary folder only, the layout the installers left in Windows Sandbox on
    30 Oct. 2026 (empty files named like the programs; nothing is run):
    - Audacity 4: C:\Program Files\Audacity 4\bin\Audacity.exe — the old pattern
      « Audacity*\Audacity.exe » did not find it, so Audacity was never started (step 12);
    - VLC, 7-Zip, Notepad++, GIMP 3 (bin\gimp-3.2.exe next to gimp-console-3.2.exe), Firefox;
    - a Start menu shortcut, used when no folder pattern matches.
    Exit code 0 when every case passes, 1 otherwise.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $root 'scripts\sandbox\common.ps1')

$work = Join-Path ([IO.Path]::GetTempPath()) ('cleanslate-programs-' + [guid]::NewGuid().ToString('N'))
$failed = 0
function Check([string] $name, [bool] $ok, [string] $detail) {
    if (-not $ok) { $script:failed++ }
    $color = if ($ok) { 'Green' } else { 'Red' }
    Write-Host "$(if ($ok) { 'PASS' } else { 'FAIL' })  $name$(if (-not $ok -and $detail) { " — $detail" })" -ForegroundColor $color
}

try {
    $programFiles = Join-Path $work 'Program Files'
    $localPrograms = Join-Path $work 'Programs'
    $startMenu = Join-Path $work 'Start Menu'
    foreach ($file in @(
            "$programFiles\Audacity 4\bin\Audacity.exe",
            "$programFiles\Audacity 4\bin\crashreporter.exe",
            "$programFiles\VideoLAN\VLC\vlc.exe",
            "$programFiles\7-Zip\7zFM.exe",
            "$programFiles\Notepad++\notepad++.exe",
            "$programFiles\Mozilla Firefox\firefox.exe",
            "$localPrograms\GIMP 3\bin\gimp-console-3.2.exe",
            "$localPrograms\GIMP 3\bin\gimp-3.2.exe",
            "$work\Elsewhere\Tool\tool.exe")) {
        New-Item -ItemType File -Force $file | Out-Null
    }
    $roots = @($programFiles, $localPrograms)

    # The old pattern of inside-apps.ps1 misses Audacity 4: this is the failure of step 12.
    $old = @($roots | ForEach-Object { Get-ChildItem (Join-Path $_ 'Audacity*\Audacity.exe') -ErrorAction SilentlyContinue })
    Check 'ancien motif « Audacity*\Audacity.exe » : ne trouve pas Audacity 4 (cas reproduit)' ($old.Count -eq 0) "trouvé $($old.Count)"

    $cases = @(
        @{ Name = 'Audacity 4 (bin\)'; Folder = 'Audacity*'; Exe = 'Audacity.exe'; Expected = "$programFiles\Audacity 4\bin\Audacity.exe" },
        @{ Name = 'VLC'; Folder = 'VideoLAN'; Exe = 'vlc.exe'; Expected = "$programFiles\VideoLAN\VLC\vlc.exe" },
        @{ Name = '7-Zip'; Folder = '7-Zip'; Exe = '7zFM.exe'; Expected = "$programFiles\7-Zip\7zFM.exe" },
        @{ Name = 'Notepad++'; Folder = 'Notepad++'; Exe = 'notepad++.exe'; Expected = "$programFiles\Notepad++\notepad++.exe" },
        @{ Name = 'GIMP 3 (pas la console)'; Folder = 'GIMP*'; Exe = 'gimp-[0-9]*.exe'; Expected = "$localPrograms\GIMP 3\bin\gimp-3.2.exe" },
        @{ Name = 'Firefox'; Folder = 'Mozilla Firefox'; Exe = 'firefox.exe'; Expected = "$programFiles\Mozilla Firefox\firefox.exe" }
    )
    foreach ($c in $cases) {
        $found = Find-MainProgram $roots $c.Folder $c.Exe $c.Name @($startMenu)
        Check "Find-MainProgram : $($c.Name)" ($found -eq $c.Expected) "trouvé « $found »"
    }

    # No folder pattern matches: the Start menu shortcut gives the program, never the uninstaller.
    New-Item -ItemType Directory -Force "$startMenu\Tool" | Out-Null
    New-Item -ItemType File -Force "$work\Elsewhere\Tool\unins000.exe" | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    foreach ($link in @(@('Tool.lnk', "$work\Elsewhere\Tool\tool.exe"), @('Uninstall Tool.lnk', "$work\Elsewhere\Tool\unins000.exe"))) {
        $l = $shell.CreateShortcut("$startMenu\Tool\$($link[0])"); $l.TargetPath = $link[1]; $l.Save()
    }
    $found = Find-MainProgram $roots 'NoSuchFolder*' 'tool.exe' 'Tool' @($startMenu)
    Check 'Find-MainProgram : raccourci du menu Démarrer (pas celui de désinstallation)' ($found -eq "$work\Elsewhere\Tool\tool.exe") "trouvé « $found »"

    $none = Find-MainProgram $roots 'NoSuchFolder*' 'none.exe' 'Nothing' @($startMenu)
    Check 'Find-MainProgram : rien trouvé → $null' ($null -eq $none) "trouvé « $none »"
}
finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "PowerShell $($PSVersionTable.PSVersion) : $(if ($failed) { "$failed échec(s)" } else { 'tout passe' })"
if ($failed) { exit 1 }
exit 0
