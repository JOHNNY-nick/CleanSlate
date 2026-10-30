<#
.SYNOPSIS
    v1.0 complements (tests-acceptation.md § 6), run INSIDE Windows Sandbox by the logon
    command written by "prepare.ps1 -Scenario Complements".

.DESCRIPTION
    For each case, three inventoried steps: build the test object; quarantine it with
    "cleanslate-cli-sandbox sandbox quarantine" and check it is gone; restore it with
    "cleanslate-cli-sandbox sandbox restore" and compare with the state recorded before.
    1. a dead shortcut of the Start menu and its emptied folder (D20);
    2. a scheduled task whose program was deleted (D47);
    3. a service whose program was deleted (D47);
    4. a test MSIX package signed with a certificate created here (the Store does not
       exist in the Sandbox), removed with progress (EF-04, EF-21), then a package leftover (D48);
    5. VLC and GIMP installed from the verified installers, for the manual checks.
    Writes results\complements-<date>\rapport.md and rapport.html. The test commands only
    run when the three conditions of the guard hold (D53); this script checks them first.
#>
$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"
Assert-Sandbox
Start-Run 'complements'

$out = $Run.Out
$desktop = [Environment]::GetFolderPath('Desktop')
$cli = "$Tools\cleanslate-cli.exe"
$sandboxCli = "$Tools\cleanslate-cli-sandbox.exe"
$work = 'C:\CleanSlateTest'
New-Item -ItemType Directory -Force $work | Out-Null
$prepared = Get-Content "$Repo\artifacts\sandbox\run\prepare.json" -Raw -Encoding utf8 | ConvertFrom-Json
$checks = @()
$meta = @(
    "Date : $(Get-Date -Format 'dd/MM/yyyy HH:mm')",
    "Préparé le $($prepared.prepare), commit $($prepared.commit)",
    "Dossier : $out"
)

if (-not (Test-Path $sandboxCli)) {
    Write-Report 'Compléments v1.0 — ARRÊTÉ' $meta @(
        [pscustomobject]@{ Name = 'CLI de test présente'; Measure = "$sandboxCli absent : relancer prepare.ps1 -Scenario Complements"; Status = 'script' }
    ) ([ordered]@{})
    Stop-Run
    exit 1
}

# Round trip through the quarantine of everything matching $match, in two steps.
function RoundTrip([string] $case, [string] $match, [scriptblock] $state, [scriptblock] $isGone, [string] $note) {
    $ctx = @{ Before = (& $state); Session = $null; Gone = $false; After = $null }
    Invoke-Step "$case : mise en quarantaine" {
        $ctx.Session = Invoke-CleanSlateCli $sandboxCli @('sandbox', 'quarantine', $match)
        Start-Sleep -Seconds 2
        $ctx.Gone = [bool](& $isGone)
        Write-Host "retiré : $($ctx.Gone)"
    }
    $quarantineStep = $Run.Steps[-1]
    $restoreStep = $null
    if ($ctx.Session) {
        Invoke-Step "$case : restauration" {
            $restored = Invoke-CleanSlateCli $sandboxCli @('sandbox', 'restore', $ctx.Session.id)
            if ($restored -and $restored.errors.Count) { Write-Warning ($restored.errors | ConvertTo-Json -Compress) }
            Start-Sleep -Seconds 2
            $ctx.After = & $state
        }
        $restoreStep = $Run.Steps[-1]
    }
    $same = $ctx.Session -and ($ctx.Before -eq $ctx.After)
    if ($ctx.Session -and -not $same) {
        $slug = $case -replace '\W', '_'
        Set-Content "$out\$slug-avant.txt" $ctx.Before -Encoding utf8
        Set-Content "$out\$slug-apres.txt" $ctx.After -Encoding utf8
    }
    $detail = if ($quarantineStep.Failure) { "échec du script : $($quarantineStep.Failure)" }
    elseif ($ctx.Session) { "$note ; session $($ctx.Session.id), $($ctx.Session.items.Count) élément(s)" }
    else { "$note ; CleanSlate n'a rien mis en quarantaine (voir cli-progression.txt)" }
    $script:checks += [pscustomobject]@{ Name = "$case : retiré par la quarantaine"; Measure = $detail; Status = (Status $ctx.Gone $quarantineStep) }
    $restoreMeasure = if ($quarantineStep.Failure -or ($restoreStep -and $restoreStep.Failure)) { 'non mesuré (échec du script)' } elseif ($same) { 'état identique' } else { 'états différents (fichiers -avant / -apres)' }
    $script:checks += [pscustomobject]@{ Name = "$case : restauré à l'identique"; Measure = $restoreMeasure; Status = $(if ($quarantineStep.Failure) { 'script' } else { Status $same $restoreStep }) }
}
# A small program to point at (deleted afterwards).
function DummyExe([string] $path) {
    New-Item -ItemType Directory -Force (Split-Path $path) | Out-Null
    Copy-Item "$env:windir\System32\whoami.exe" $path -Force
}

Invoke-Step 'Copie de CleanSlate sur le Bureau' {
    Copy-Item "$Repo\app\CleanSlate.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64" "$desktop\CleanSlate" -Recurse -Force
}

# 1. Dead shortcut of the Start menu and its folder (D20).
$menu = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\CleanSlateTest Old Tool"
Invoke-Step 'Construction : raccourci mort du menu Démarrer' {
    $target = "$work\OldTool\oldtool.exe"
    DummyExe $target
    New-Item -ItemType Directory -Force $menu | Out-Null
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut("$menu\Old Tool.lnk"); $link.TargetPath = $target; $link.Save()
    Remove-Item "$work\OldTool" -Recurse -Force
}
RoundTrip 'Raccourci du menu Démarrer + dossier' 'CleanSlateTest Old Tool' { FileState $menu } { -not (Test-Path -LiteralPath $menu) } 'raccourci et dossier vidé'

# 2. Scheduled task whose program was deleted (D47).
Invoke-Step 'Construction : tâche planifiée orpheline' {
    $agent = "$work\TaskApp\agent.exe"
    DummyExe $agent
    & schtasks.exe /create /tn 'CleanSlateTest\Agent' /tr $agent /sc daily /st 12:00 /f
    Remove-Item "$work\TaskApp" -Recurse -Force
}
RoundTrip 'Tâche planifiée' '\CleanSlateTest\Agent' {
    (& schtasks.exe /query /tn 'CleanSlateTest\Agent' /xml 2>$null) -join "`n"
} { $null = & schtasks.exe /query /tn 'CleanSlateTest\Agent' 2>$null; $LASTEXITCODE -ne 0 } 'XML complet comparé'

# 3. Service whose program was deleted (D47).
Invoke-Step 'Construction : service orphelin' {
    $svcExe = "$work\SvcApp\svc.exe"
    DummyExe $svcExe
    & sc.exe create CleanSlateTestSvc binPath= "`"$svcExe`" -run" start= demand obj= 'NT AUTHORITY\LocalService' DisplayName= 'CleanSlate service de test'
    & sc.exe description CleanSlateTestSvc 'Service de test de CleanSlate'
    & sc.exe failure CleanSlateTestSvc reset= 86400 actions= restart/60000/restart/120000
    & reg.exe add 'HKLM\SYSTEM\CurrentControlSet\Services\CleanSlateTestSvc\Parameters' /v Channel /t REG_SZ /d stable /f
    Remove-Item "$work\SvcApp" -Recurse -Force
}
RoundTrip 'Service' 'Services\CleanSlateTestSvc' {
    $config = ((& sc.exe qc CleanSlateTestSvc 4096) + (& sc.exe qdescription CleanSlateTestSvc 4096) + (& sc.exe qfailure CleanSlateTestSvc 4096)) -join "`n"
    $config + "`n" + (RegState 'HKLM\SYSTEM\CurrentControlSet\Services\CleanSlateTestSvc')
} { $null = & sc.exe query CleanSlateTestSvc; $LASTEXITCODE -ne 0 } 'sc qc / qdescription / qfailure et clé .reg comparés'

# 4. Test MSIX package (EF-04, EF-21), built and signed here, never on the host.
$msix = "$work\CleanSlateTest.msix"
Invoke-Step 'Construction : paquet MSIX de test signé (certificat créé dans le Sandbox)' {
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
    & 'C:\CleanSlate\sdk\makeappx.exe' pack /d $pkg /p $msix /o
    $cert = New-SelfSignedCertificate -Type Custom -Subject 'CN=CleanSlateTest' -KeyUsage DigitalSignature `
        -CertStoreLocation 'Cert:\CurrentUser\My' -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
    $password = ConvertTo-SecureString -String 'sandbox' -Force -AsPlainText
    Export-PfxCertificate -Cert $cert -FilePath "$work\test.pfx" -Password $password | Out-Null
    Export-Certificate -Cert $cert -FilePath "$work\test.cer" | Out-Null
    Import-Certificate -FilePath "$work\test.cer" -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
    & 'C:\CleanSlate\sdk\signtool.exe' sign /fd SHA256 /f "$work\test.pfx" /p sandbox $msix
    Add-AppxPackage -Path $msix
}
$package = Get-AppxPackage -Name 'CleanSlateTest.App'
if ($package) {
    $store = @{ Listed = 0; Removed = $null; Percents = @() }
    Invoke-Step 'Paquet MSIX : liste puis désinstallation avec progression' {
        $apps = Invoke-CleanSlateCli $cli @('apps', '--json')
        $store.Listed = @($apps | Where-Object { $_.kind -eq 'store' -and $_.package_family_name -eq $package.PackageFamilyName }).Count
        $store.Removed = Invoke-CleanSlateCli $sandboxCli @('sandbox', 'uninstall', 'CleanSlate Test App')
        $events = @(Get-Content "$out\cli-progression.txt" -ErrorAction SilentlyContinue | Where-Object { $_ -like '{*"step"*' })
        $store.Percents = @($events | ForEach-Object { ($_ | ConvertFrom-Json).percent } | Where-Object { $_ -ne $null })
    }
    $storeStep = $Run.Steps[-1]
    $still = [bool](Get-AppxPackage -Name 'CleanSlateTest.App')
    $checks += [pscustomobject]@{ Name = 'Paquet MSIX : listé (badge Store)'; Measure = "famille $($package.PackageFamilyName)"; Status = (Status ($store.Listed -eq 1) $storeStep) }
    $checks += [pscustomobject]@{ Name = "Paquet MSIX : désinstallé pour l'utilisateur, avec progression"; Measure = "$($store.Percents.Count) relevés de progression, dernier $($store.Percents | Select-Object -Last 1) %"; Status = (Status ($store.Removed -and -not $still -and $store.Percents.Count) $storeStep) }
}
else {
    $checks += [pscustomobject]@{ Name = 'Paquet MSIX : installation'; Measure = 'Add-AppxPackage a échoué (préparation du test, voir journal.txt)'; Status = 'script' }
}

# 5. Package leftover (D48).
$family = 'CleanSlateTest.Leftover_abcdefghjk123'
$folder = "$env:LOCALAPPDATA\Packages\$family"
$key = "HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Storage\$($family.ToLower())"
Invoke-Step 'Construction : reste de paquet du Store' {
    New-Item -ItemType Directory -Force "$folder\LocalState" | Out-Null
    Set-Content "$folder\LocalState\settings.dat" 'theme=dark' -Encoding ascii
    & reg.exe add $key /v Moniker /t REG_SZ /d cleanslatetest.leftover /f
}
RoundTrip 'Reste de paquet du Store (dossier + clé)' 'CleanSlateTest.Leftover' {
    (FileState $folder) + "`n" + (RegState $key)
} { -not (Test-Path -LiteralPath $folder) } 'dossier Packages\<famille> et clé AppContainer\Storage'

# 6. VLC (MSI) and GIMP (Inno Setup), from the verified installers: installed, uninstalled
#    by CleanSlate with an automatic check of the progress events (EF-21, D57), then
#    installed again for the manual checks.
$installers = @(Test-Installer "$Repo\scripts\sandbox\manifeste.json" "$Repo\artifacts\sandbox\installers" 'complements')
$bad = @($installers | Where-Object { -not $_.Ok })
$checks += [pscustomobject]@{ Name = 'Installeurs VLC et GIMP conformes au manifeste'; Measure = "$($installers.Count - $bad.Count) / $($installers.Count) SHA-256"; Status = $(if ($installers.Count -and -not $bad.Count) { 'ok' } else { 'script' }) }

function Install-FromManifest($installer, [string] $purpose) {
    Invoke-Step "Installation de $($installer.Name) $($installer.Version) ($purpose)" {
        $program, $arguments = Split-Command $installer.Install.Replace('<fichier>', "`"$($installer.Path)`"")
        Write-Host "$program $arguments"
        $p = Start-Process $program $arguments -Wait -PassThru
        Write-Host "code de sortie $($p.ExitCode)"
    }
}

# Uninstalls with "sandbox uninstall" (quiet, like « Désinstallation silencieuse » in the
# app), keeps every progress event in progression-<app>.txt and checks them. MSI: real
# percentages up to 100 % and Windows Installer actions; .exe: the processes of the Job
# Object and never a percentage. Both: really uninstalled, and the summary line
# « uninstall finished » in CleanSlate's log.
function Test-UninstallProgress($installer, [string] $cliName, [bool] $isMsi) {
    $ctx = @{ Result = $null; Events = @() }
    Invoke-Step "Désinstallation de $($installer.Name) par CleanSlate (progression vérifiée)" {
        $ctx.Result = Invoke-CleanSlateCli $sandboxCli @('sandbox', 'uninstall', $cliName)
        $ctx.Events = @(Get-Content "$out\cli-err.txt" -Encoding utf8 -ErrorAction SilentlyContinue | Where-Object { $_ -like '{*' } | ForEach-Object { $_ | ConvertFrom-Json })
        Write-Host "$($ctx.Events.Count) événement(s) de progression"
    }
    $step = $Run.Steps[-1]
    $slug = ($cliName -replace '\W+', '-').ToLowerInvariant()
    $lines = @("Progression de la désinstallation de $($installer.Name) $($installer.Version) par cleanslate-cli-sandbox", 'elapsed_ms | étape | % | action | processus')
    $lines += $ctx.Events | ForEach-Object { "$($_.elapsed_ms) | $($_.step) | $($_.percent) | $($_.action) | $(($_.processes | ForEach-Object { "$($_.name) ($($_.pid))" }) -join ', ')" }
    $lines | Set-Content "$out\progression-$slug.txt" -Encoding utf8

    $events = $ctx.Events
    $percents = @($events | Where-Object { $null -ne $_.percent } | ForEach-Object { [double]$_.percent })
    $actions = @($events | Where-Object { $_.action })
    $names = @($events | ForEach-Object { $_.processes } | ForEach-Object { $_.name } | Select-Object -Unique)
    $withProcesses = @($events | Where-Object { @($_.processes).Count -gt 0 })
    $duration = if ($events.Count) { $events[-1].elapsed_ms } else { 0 }
    $steps = (@($events | ForEach-Object { $_.step }) | Get-Unique) -join ' > '
    $gone = [bool]($ctx.Result -and -not $ctx.Result.still_installed)
    $script:checks += [pscustomobject]@{ Name = "$($installer.Name) : désinstallé par CleanSlate"; Measure = "$($events.Count) événement(s) en $duration ms ($steps) ; code $($ctx.Result.exit_code) ; progression-$slug.txt"; Status = (Status $gone $step) }
    if ($isMsi) {
        $maxPercent = if ($percents.Count) { ($percents | Measure-Object -Maximum).Maximum } else { $null }
        $distinct = @($percents | Select-Object -Unique).Count
        $script:checks += [pscustomobject]@{ Name = "$($installer.Name) (MSI) : pourcentage réel jusqu'à 100 % (EF-21)"; Measure = "$($percents.Count) relevé(s), $distinct valeur(s) distincte(s), maximum $maxPercent %"; Status = (Status ($distinct -ge 2 -and $maxPercent -eq 100) $step) }
        $script:checks += [pscustomobject]@{ Name = "$($installer.Name) (MSI) : action en cours de Windows Installer"; Measure = "$($actions.Count) relevé(s) avec action$(if ($actions.Count) { ", ex. « $($actions[0].action) »" })"; Status = (Status ($actions.Count -ge 1) $step) }
    }
    else {
        $script:checks += [pscustomobject]@{ Name = "$($installer.Name) (.exe) : processus du désinstalleur (EF-21)"; Measure = "$($withProcesses.Count) relevé(s) avec processus : $(if ($names.Count) { $names -join ', ' } else { 'aucun' })"; Status = (Status ($withProcesses.Count -ge 1) $step) }
        $script:checks += [pscustomobject]@{ Name = "$($installer.Name) (.exe) : aucun pourcentage inventé"; Measure = "$($percents.Count) relevé(s) avec pourcentage"; Status = (Status ($percents.Count -eq 0) $step) }
    }
    $logLine = Get-ChildItem "$out\cleanslate-logs" -Filter 'cleanslate.*.log' -ErrorAction SilentlyContinue |
        Select-String -Pattern 'uninstall finished' | Where-Object { $_.Line -match [regex]::Escape($cliName) } | Select-Object -Last 1
    $logMeasure = if ($logLine) { $logLine.Line -replace '^.*?uninstall finished\s*', '' } else { 'absente de cleanslate-logs' }
    $script:checks += [pscustomobject]@{ Name = "$($installer.Name) : ligne récapitulative dans le journal de CleanSlate"; Measure = $logMeasure; Status = (Status ([bool]$logLine) $step) }
}

if (-not $bad.Count) {
    $cliNames = @{ 'VideoLAN.VLC' = 'VLC media player'; 'GIMP.GIMP.3' = 'GIMP' }
    foreach ($i in $installers) {
        Install-FromManifest $i 'désinstallation vérifiée'
        Test-UninstallProgress $i $cliNames[$i.Id] ($i.Install -like 'msiexec*')
        Install-FromManifest $i 'vérifications manuelles'
    }
}
$checks += [pscustomobject]@{ Name = 'Dans CleanSlate (Bureau) : VLC et GIMP par « Désinstaller complètement », filtre « Store »'; Measure = 'd''abord Réglages → désactiver « Mode simulation » (sinon rien n''est lancé : l''étape 4 l''indique) ; puis comparer le « Déroulé de la désinstallation » de l''étape 4 avec progression-*.txt'; Status = 'manuel' }

Write-Report 'Compléments v1.0 — résultats dans Windows Sandbox' $meta $checks ([ordered]@{
        'Installeurs vérifiés' = @($installers | ForEach-Object { "$(if ($_.Ok) { 'conforme' } else { 'NON CONFORME' }) — $($_.Name) $($_.Version) — SHA-256 $($_.Actual)" })
    })
Stop-Run
Start-Process "$out\rapport.html"
Watch-Session
exit 0
