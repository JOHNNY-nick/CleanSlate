<#
.SYNOPSIS
    Installer test (tests-acceptation.md § 5), run INSIDE Windows Sandbox by the logon
    command written by "prepare.ps1 -Scenario Installer".

.DESCRIPTION
    The checks of § 5 are visual (UAC, French wizard, icon, Start menu): this script only
    copies the installer to the Desktop and starts it. Refuses to run outside the Sandbox.
#>
if ($env:USERNAME -ne 'WDAGUtilityAccount') {
    Write-Error 'Ce script ne tourne que dans Windows Sandbox (compte WDAGUtilityAccount).'
    exit 1
}

$desktop = [Environment]::GetFolderPath('Desktop')
$setup = Get-ChildItem 'C:\CleanSlate\setup' -Filter 'CleanSlate-*-setup.exe' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $setup) {
    Write-Error 'Aucun installeur dans C:\CleanSlate\setup.'
    exit 1
}
Copy-Item $setup.FullName $desktop -Force
Start-Process (Join-Path $desktop $setup.Name)
