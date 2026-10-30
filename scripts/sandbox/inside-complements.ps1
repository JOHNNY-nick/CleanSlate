<#
.SYNOPSIS
    v1.0 complements (tests-acceptation.md § 6), run INSIDE Windows Sandbox by the logon
    command written by "prepare.ps1 -Scenario Complements".

.DESCRIPTION
    For each case: build the test object, record its state, quarantine it with
    "cleanslate-cli sandbox quarantine", check it is gone, restore it with
    "cleanslate-cli sandbox restore", and compare with the recorded state.
    1. a dead shortcut of the Start menu and its emptied folder (D20);
    2. a scheduled task whose program was deleted (D47);
    3. a service whose program was deleted (D47);
    4. a test MSIX package signed with a local test certificate (the Store does not exist
       in the Sandbox), removed with progress (EF-04, EF-21), then a package leftover (D48).
    Writes results\<date>\rapport-complements.md. Refuses to run outside the Sandbox.
#>
$ErrorActionPreference = 'Continue'
if ($env:USERNAME -ne 'WDAGUtilityAccount') {
    Write-Error 'Ce script ne tourne que dans Windows Sandbox (compte WDAGUtilityAccount).'
    exit 1
}

$base = 'C:\CleanSlate'
$cli = "$base\tools\cleanslate-cli.exe"
$out = Join-Path "$base\results" ('complements-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
$work = 'C:\CleanSlateTest'
New-Item -ItemType Directory -Force $out, $work | Out-Null
Start-Transcript -Path "$out\journal.txt" | Out-Null
$rows = @()

function Step([string] $text) { Write-Host "`n=== $text ===" -ForegroundColor Cyan }

# Runs the CLI; stdout is JSON, stderr goes to the journal.
function Cli([string[]] $arguments) {
    $stdout = "$out\cli-out.json"; $stderr = "$out\cli-err.txt"
    $p = Start-Process $cli -ArgumentList $arguments -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Get-Content $stderr -ErrorAction SilentlyContinue | Add-Content "$out\cli-progression.txt"
    if ($p.ExitCode -ne 0) { Write-Warning "cleanslate-cli $($arguments -join ' ') : code $($p.ExitCode)"; return $null }
    Get-Content $stdout -Raw -Encoding utf8 | ConvertFrom-Json
}

# State of files: relative path, size, modification time and SHA-256.
function FileState([string] $path) {
    if (-not (Test-Path -LiteralPath $path)) { return '<absent>' }
    $item = Get-Item -LiteralPath $path -Force
    $files = if ($item.PSIsContainer) { Get-ChildItem -LiteralPath $path -Recurse -Force } else { @($item) }
    ($files | Sort-Object FullName | ForEach-Object {
            $rel = $_.FullName.Substring($item.FullName.Length)
            if ($_.PSIsContainer) { "D $rel" } else { "F $rel $($_.Length) $($_.LastWriteTimeUtc.Ticks) $((Get-FileHash -LiteralPath $_.FullName).Hash)" }
        }) -join "`n"
}

# State of a registry key: its .reg export.
function RegState([string] $key) {
    $file = "$out\state.reg"
    & reg.exe export $key $file /y 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) { return '<absent>' }
    # Same content in another order is identical: values sorted within each key.
    $sections = @(); $current = $null
    foreach ($line in Get-Content $file) {
        if ($line -match '^\[.*\]$') { if ($current) { $sections += $current }; $current = @{ Head = $line; Values = @() } }
        elseif ($current -and $line.Trim()) { $current.Values += $line }
    }
    if ($current) { $sections += $current }
    ($sections | ForEach-Object { $_.Head; $_.Values | Sort-Object }) -join "`n"
}

function Record([string] $case, [bool] $gone, [string] $before, [string] $after, [string] $note) {
    $same = $before -eq $after
    $script:rows += "| $case | $(if ($gone) { 'oui' } else { 'NON' }) | $(if ($same) { 'oui' } else { 'NON' }) | $note |"
    if (-not $same) {
        Set-Content "$out\$($case -replace '\W', '_')-avant.txt" $before
        Set-Content "$out\$($case -replace '\W', '_')-apres.txt" $after
    }
}

# Round trip through the quarantine of everything matching $match.
function RoundTrip([string] $case, [string] $match, [scriptblock] $state, [scriptblock] $isGone, [string] $note) {
    $before = & $state
    $session = Cli @('sandbox', 'quarantine', $match)
    if (-not $session) { Record $case $false $before '<non mis en quarantaine>' $note; return }
    Start-Sleep -Seconds 2
    $gone = [bool](& $isGone)
    $restored = Cli @('sandbox', 'restore', $session.id)
    if ($restored -and $restored.errors.Count) { Write-Warning ($restored.errors | ConvertTo-Json -Compress) }
    Start-Sleep -Seconds 2
    Record $case $gone $before (& $state) "$note ; session $($session.id), $($session.items.Count) élément(s)"
}

# A small program to point at (deleted afterwards).
function DummyExe([string] $path) {
    New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
    Copy-Item "$env:windir\System32\whoami.exe" $path -Force
}

# 1. Dead shortcut of the Start menu and its folder (D20).
Step 'Raccourci mort du menu Démarrer'
$menu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\CleanSlateTest Old Tool"
$target = "$work\OldTool\oldtool.exe"
DummyExe $target
New-Item -ItemType Directory -Force $menu | Out-Null
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut("$menu\Old Tool.lnk"); $link.TargetPath = $target; $link.Save()
Remove-Item "$work\OldTool" -Recurse -Force
RoundTrip 'Raccourci du menu Démarrer + dossier' 'CleanSlateTest Old Tool' { FileState $menu } { -not (Test-Path -LiteralPath $menu) } 'raccourci et dossier vidé'

# 2. Scheduled task whose program was deleted (D47).
Step 'Tâche planifiée orpheline'
$agent = "$work\TaskApp\agent.exe"
DummyExe $agent
& schtasks.exe /create /tn 'CleanSlateTest\Agent' /tr $agent /sc daily /st 12:00 /f | Out-Null
Remove-Item "$work\TaskApp" -Recurse -Force
RoundTrip 'Tâche planifiée' '\CleanSlateTest\Agent' {
    (& schtasks.exe /query /tn 'CleanSlateTest\Agent' /xml 2>$null) -join "`n"
} { $null = & schtasks.exe /query /tn 'CleanSlateTest\Agent' 2>$null; $LASTEXITCODE -ne 0 } 'XML complet comparé'

# 3. Service whose program was deleted (D47).
Step 'Service orphelin'
$svcExe = "$work\SvcApp\svc.exe"
DummyExe $svcExe
& sc.exe create CleanSlateTestSvc binPath= "`"$svcExe`" -run" start= demand obj= 'NT AUTHORITY\LocalService' DisplayName= 'CleanSlate service de test' | Out-Null
& sc.exe description CleanSlateTestSvc 'Service de test de CleanSlate' | Out-Null
& sc.exe failure CleanSlateTestSvc reset= 86400 actions= restart/60000/restart/120000 | Out-Null
& reg.exe add 'HKLM\SYSTEM\CurrentControlSet\Services\CleanSlateTestSvc\Parameters' /v Channel /t REG_SZ /d stable /f | Out-Null
Remove-Item "$work\SvcApp" -Recurse -Force
RoundTrip 'Service' 'Services\CleanSlateTestSvc' {
    $config = ((& sc.exe qc CleanSlateTestSvc 4096) + (& sc.exe qdescription CleanSlateTestSvc 4096) + (& sc.exe qfailure CleanSlateTestSvc 4096)) -join "`n"
    $config + "`n" + (RegState 'HKLM\SYSTEM\CurrentControlSet\Services\CleanSlateTestSvc')
} { $null = & sc.exe query CleanSlateTestSvc; $LASTEXITCODE -ne 0 } 'sc qc / qdescription / qfailure et clé .reg comparés'

# 4. Test MSIX package (EF-04, EF-21), then a package leftover (D48).
Step 'Paquet MSIX de test'
$pkg = "$work\msix"
New-Item -ItemType Directory -Force "$pkg\Assets" | Out-Null
Copy-Item $cli "$pkg\app.exe"
Add-Type -AssemblyName System.Drawing
foreach ($logo in @(@('StoreLogo', 50), @('Square150', 150), @('Square44', 44))) {
    $bmp = New-Object System.Drawing.Bitmap $logo[1], $logo[1]
    $g = [System.Drawing.Graphics]::FromImage($bmp); $g.Clear([System.Drawing.Color]::Teal); $g.Dispose()
    $bmp.Save("$pkg\Assets\$($logo[0]).png", [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}
@'
<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="CleanSlateTest.App" Publisher="CN=CleanSlateTest" Version="1.0.0.0" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>CleanSlate Test App</DisplayName>
    <PublisherDisplayName>CleanSlate Test</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources><Resource Language="fr-FR" /></Resources>
  <Applications>
    <Application Id="App" Executable="app.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="CleanSlate Test App" Description="Paquet de test" BackgroundColor="transparent"
                          Square150x150Logo="Assets\Square150.png" Square44x44Logo="Assets\Square44.png" />
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>
'@ | Set-Content "$pkg\AppxManifest.xml" -Encoding utf8
$msix = "$work\CleanSlateTest.msix"
& "$base\sdk\makeappx.exe" pack /d $pkg /p $msix /o | Out-Null
$cert = New-SelfSignedCertificate -Type Custom -Subject 'CN=CleanSlateTest' -KeyUsage DigitalSignature `
    -CertStoreLocation 'Cert:\CurrentUser\My' -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
$password = ConvertTo-SecureString -String 'sandbox' -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath "$work\test.pfx" -Password $password | Out-Null
Export-Certificate -Cert $cert -FilePath "$work\test.cer" | Out-Null
Import-Certificate -FilePath "$work\test.cer" -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
& "$base\sdk\signtool.exe" sign /fd SHA256 /f "$work\test.pfx" /p sandbox $msix | Out-Null
Add-AppxPackage -Path $msix
$installed = Get-AppxPackage -Name 'CleanSlateTest.App'
if ($installed) {
    $apps = Cli @('apps', '--json')
    $listed = @($apps | Where-Object { $_.kind -eq 'store' -and $_.package_family_name -eq $installed.PackageFamilyName })
    $removed = Cli @('sandbox', 'uninstall', 'CleanSlate Test App')
    $events = @(Get-Content "$out\cli-progression.txt" -ErrorAction SilentlyContinue | Where-Object { $_ -like '{*"step"*' })
    $percents = @($events | ForEach-Object { ($_ | ConvertFrom-Json).percent } | Where-Object { $_ -ne $null })
    $still = [bool](Get-AppxPackage -Name 'CleanSlateTest.App')
    $rows += "| Paquet MSIX : listé (badge Store) | $(if ($listed.Count -eq 1) { 'oui' } else { 'NON' }) | — | famille $($installed.PackageFamilyName) |"
    $rows += "| Paquet MSIX : désinstallé pour l'utilisateur | $(if ($removed -and -not $still) { 'oui' } else { 'NON' }) | — | $($percents.Count) relevés de progression, dernier $($percents | Select-Object -Last 1) % |"
}
else {
    $rows += '| Paquet MSIX : installation | NON | — | Add-AppxPackage a échoué (voir journal) |'
}

Step 'Reste de paquet'
$family = 'CleanSlateTest.Leftover_abcdefghjk123'
$folder = "$env:LOCALAPPDATA\Packages\$family"
New-Item -ItemType Directory -Force "$folder\LocalState" | Out-Null
Set-Content "$folder\LocalState\settings.dat" 'theme=dark' -Encoding ascii
$key = "HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Storage\$($family.ToLower())"
& reg.exe add $key /v Moniker /t REG_SZ /d cleanslatetest.leftover /f | Out-Null
RoundTrip 'Reste de paquet du Store (dossier + clé)' 'CleanSlateTest.Leftover' {
    (FileState $folder) + "`n" + (RegState $key)
} { -not (Test-Path -LiteralPath $folder) } 'dossier Packages\<famille> et clé AppContainer\Storage'

$lines = @(
    '# Compléments v1.0 — résultats dans Windows Sandbox',
    '',
    "Date : $(Get-Date -Format 'dd/MM/yyyy HH:mm')",
    '',
    '| Cas | Retiré après la mise en quarantaine | Restauré à l''identique | Détail |',
    '| --- | --- | --- | --- |'
) + $rows + @('', 'Les états comparés (avant / après) des cas en échec sont dans ce dossier ; la progression de la CLI est dans cli-progression.txt.')
$lines | Set-Content "$out\rapport-complements.md" -Encoding utf8
Step "Terminé : $out\rapport-complements.md"
Stop-Transcript | Out-Null
Start-Process notepad.exe "$out\rapport-complements.md"
