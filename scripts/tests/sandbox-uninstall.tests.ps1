<#
.SYNOPSIS
    Tests of Get-SilentUninstall and Wait-Program (common.ps1, D59): the classic uninstall of
    the Apps scenario must never wait for a click. Runs under Windows PowerShell 5.1 and 7:
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/sandbox-uninstall.tests.ps1
        pwsh -NoProfile -File scripts/tests/sandbox-uninstall.tests.ps1

.DESCRIPTION
    On 30 Oct. 2026 (run apps-20261030-185352, step 20), GIMP 3 registered
    QuietUninstallString = "…\uninst\unins000.exe" /SILENT; used as is, its uninstaller left
    the « remove personal settings? » box open and the step lasted 4417 s. Only the command
    line is built here (entries are plain objects); the one program started is ping.exe on
    the loopback, to check the time limit. Exit code 0 when every case passes, 1 otherwise.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $root 'scripts\sandbox\common.ps1')

$failed = 0
function Check([string] $name, $actual, $expected) {
    $ok = ($actual -join '|') -eq ($expected -join '|')
    if (-not $ok) { $script:failed++ }
    $color = if ($ok) { 'Green' } else { 'Red' }
    Write-Host "$(if ($ok) { 'PASS' } else { 'FAIL' })  $name$(if (-not $ok) { " — obtenu « $($actual -join ' | ') », attendu « $($expected -join ' | ') »" })" -ForegroundColor $color
}
function Entry([string] $key, [string] $uninstall, [string] $quiet, $msi) {
    [pscustomobject]@{ PSChildName = $key; UninstallString = $uninstall; QuietUninstallString = $quiet; WindowsInstaller = $msi }
}
$gimp = 'C:\Users\WDAGUtilityAccount\AppData\Local\Programs\GIMP 3\uninst\unins000.exe'
$silent = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'

try {
    Check 'GIMP 3 : /SILENT du QuietUninstallString remplacé (cas du 30 oct. 2026)' `
        (Get-SilentUninstall (Entry 'GIMP-3_is1' "`"$gimp`"" "`"$gimp`" /SILENT" $null)) @($gimp, $silent)
    Check 'Inno Setup sans QuietUninstallString' `
        (Get-SilentUninstall (Entry 'App_is1' "`"$gimp`"" $null $null)) @($gimp, $silent)
    Check 'Inno Setup : options déjà présentes, sans doublon, autres arguments gardés' `
        (Get-SilentUninstall (Entry 'App_is1' "`"$gimp`"" "`"$gimp`" /verysilent /LOG=`"C:\l.txt`" /NoRestart" $null)) @($gimp, "/LOG=`"C:\l.txt`" $silent")
    Check 'Inno Setup : unins001.exe, chemin sans guillemets' `
        (Get-SilentUninstall (Entry 'App_is1' 'C:\Apps\Old\unins001.exe' $null $null)) @('C:\Apps\Old\unins001.exe', $silent)
    Check 'MSI : msiexec /x par le code produit' `
        (Get-SilentUninstall (Entry '{1A2B}' 'MsiExec.exe /I{1A2B}' $null 1)) @('msiexec.exe', '/x {1A2B} /qn /norestart')
    Check 'MSI sans WindowsInstaller, reconnu par msiexec' `
        (Get-SilentUninstall (Entry '{1A2B}' 'MsiExec.exe /X{1A2B}' $null $null)) @('msiexec.exe', '/x {1A2B} /qn /norestart')
    Check 'Autre désinstalleur : QuietUninstallString gardé tel quel' `
        (Get-SilentUninstall (Entry 'App' '"C:\A\uninst.exe"' '"C:\A\uninst.exe" /quiet /x' $null)) @('C:\A\uninst.exe', '/quiet /x')
    Check 'NSIS sans QuietUninstallString : /S ajouté' `
        (Get-SilentUninstall (Entry 'Notepad++' '"C:\Program Files\Notepad++\uninstall.exe"' $null $null)) @('C:\Program Files\Notepad++\uninstall.exe', '/S')

    # Time limit: a program still running when it expires is reported, never waited for.
    $p = Start-Process "$env:windir\System32\PING.EXE" '-n 4 127.0.0.1' -PassThru -WindowStyle Hidden
    Check 'Wait-Program : délai dépassé => $false' (Wait-Program $p 1) @($false)
    Check 'Wait-Program : programme terminé dans le délai => $true' (Wait-Program $p 30) @($true)
}
catch {
    $failed++
    Write-Host "FAIL  erreur : $_" -ForegroundColor Red
}

if ($failed) { Write-Host "$failed cas en échec" -ForegroundColor Red; exit 1 }
Write-Host 'tous les cas passent' -ForegroundColor Green
exit 0
