<#
.SYNOPSIS
    Installer test (tests-acceptation.md § 5), run INSIDE Windows Sandbox by the logon
    command written by "prepare.ps1 -Scenario Installer".

.DESCRIPTION
    The checks of § 5 are partly visual (UAC, French wizard, icon): the installation and the
    uninstallation are done by hand, framed by inventoried steps:
    1. checks the SHA-256 of the setup against the one recorded by prepare.ps1;
    2. starts the setup and waits for it to end (the diff shows what it installed);
    3. automatic checks: Program Files\CleanSlate, Start menu shortcut, Uninstall entry;
    4. waits while you check § 5 and uninstall CleanSlate from Settings > Apps, then
       checks that Program Files\CleanSlate is gone and %LOCALAPPDATA%\CleanSlate kept (D43).
    Writes results\installer-<date>\rapport.md and rapport.html.
    Refuses to run outside the Sandbox prepared by prepare.ps1.
#>
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
Assert-Sandbox
Start-Run 'installer'

$out = $Run.Out
$desktop = [Environment]::GetFolderPath('Desktop')
$prepared = Get-Content "$Repo\artifacts\sandbox\run\prepare.json" -Raw -Encoding utf8 | ConvertFrom-Json
$meta = @(
    "Date : $(Get-Date -Format 'dd/MM/yyyy HH:mm')",
    "Préparé le $($prepared.prepare), commit $($prepared.commit)",
    "Dossier : $out"
)
$installDir = "$env:ProgramFiles\CleanSlate"
$checks = @()

function Uninstall-Entry {
    foreach ($k in 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall') {
        Get-ChildItem $k -ErrorAction SilentlyContinue | Get-ItemProperty | Where-Object DisplayName -Like 'CleanSlate*'
    }
}
function Shortcut { Get-ChildItem "$env:ProgramData\Microsoft\Windows\Start Menu\Programs" -Recurse -Filter 'CleanSlate*.lnk' -ErrorAction SilentlyContinue }

# 1. The setup shared by the host, checked against prepare.json.
$setup = Get-ChildItem "$Repo\artifacts\installer" -Filter 'CleanSlate-*-setup.exe' -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
$expected = if ($setup) { $prepared.sha256."artifacts\installer\$($setup.Name)" } else { $null }
$actual = if ($setup) { (Get-FileHash -LiteralPath $setup.FullName -Algorithm SHA256).Hash } else { $null }
$checks += [pscustomobject]@{ Name = 'Installeur identique à celui préparé sur l''hôte'; Measure = "$($setup.Name) SHA-256 $actual"; Status = $(if ($actual -and $actual -eq $expected) { 'ok' } else { 'ko' }) }
if (-not $actual -or $actual -ne $expected) {
    Write-Report 'Installeur — ARRÊTÉ' $meta $checks ([ordered]@{})
    Stop-Run
    exit 1
}

Invoke-Step 'Copie de l''installeur sur le Bureau' { Copy-Item $setup.FullName $desktop -Force } -NoInventory

# 2. The wizard, by hand. WaitForExit waits for the setup only, not for CleanSlate started at the end.
Invoke-Step 'Installation de CleanSlate (assistant, à la main)' {
    Write-Host 'Suivez l''assistant (§ 5, étapes 1 et 2) ; ce script attend sa fermeture.' -ForegroundColor Yellow
    $p = Start-Process (Join-Path $desktop $setup.Name) -PassThru
    $p.WaitForExit()
    Write-Host "code de sortie $($p.ExitCode)"
}

# 3. Automatic checks of the installation.
$entry = @(Uninstall-Entry)
$checks += [pscustomobject]@{ Name = 'Installé dans C:\Program Files\CleanSlate'; Measure = $(if (Test-Path "$installDir\CleanSlate.App.exe") { 'CleanSlate.App.exe présent' } else { 'CleanSlate.App.exe absent' }); Status = $(if (Test-Path "$installDir\CleanSlate.App.exe") { 'ok' } else { 'ko' }) }
$checks += [pscustomobject]@{ Name = 'Raccourci dans le menu Démarrer'; Measure = (@(Shortcut | ForEach-Object FullName) -join ' ; '); Status = $(if (Shortcut) { 'ok' } else { 'ko' }) }
$checks += [pscustomobject]@{ Name = 'Entrée dans Paramètres > Applications'; Measure = (@($entry | ForEach-Object { "$($_.DisplayName) $($_.DisplayVersion)" }) -join ' ; '); Status = $(if ($entry.Count) { 'ok' } else { 'ko' }) }
$checks += [pscustomobject]@{ Name = 'UAC, assistant en français, icône, licence MIT, À propos (version)'; Measure = 'tests-acceptation.md § 5, étapes 1 à 3'; Status = 'manuel' }

# 4. By hand: § 5 checks, a quarantine (§ 2), then uninstall from Settings.
Invoke-Step 'Vérifications § 5 puis désinstallation de CleanSlate (à la main)' {
    Write-Host @'
À faire maintenant, à la main :
  1. Réglages > À propos : icône et version ;
  2. une quarantaine en mode simulation désactivé (§ 2) pour créer %LOCALAPPDATA%\CleanSlate ;
  3. fermer CleanSlate, puis Paramètres > Applications > CleanSlate > Désinstaller.
'@ -ForegroundColor Yellow
    Read-Host 'Appuyez sur Entrée quand la désinstallation est terminée' | Out-Null
}
$checks += [pscustomobject]@{ Name = 'C:\Program Files\CleanSlate supprimé'; Measure = $(if (Test-Path $installDir) { 'dossier encore présent' } else { 'absent' }); Status = $(if (Test-Path $installDir) { 'ko' } else { 'ok' }) }
$checks += [pscustomobject]@{ Name = 'Entrée de désinstallation retirée'; Measure = "$(@(Uninstall-Entry).Count) entrée(s)"; Status = $(if (@(Uninstall-Entry).Count) { 'ko' } else { 'ok' }) }
$kept = Test-Path "$env:LOCALAPPDATA\CleanSlate"
$checks += [pscustomobject]@{ Name = '%LOCALAPPDATA%\CleanSlate conservé (D43)'; Measure = $(if ($kept) { 'présent' } else { 'absent (aucune quarantaine faite ?)' }); Status = $(if ($kept) { 'ok' } else { 'ko' }) }

Write-Report 'Installeur — résultats dans Windows Sandbox' $meta $checks ([ordered]@{})
Stop-Run
Start-Process "$out\rapport.html"
