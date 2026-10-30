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
        [pscustomobject]@{ Name = 'CLI de test présente'; Measure = "$sandboxCli absent : relancer prepare.ps1 -Scenario Complements"; Status = 'ko' }
    ) ([ordered]@{})
    Stop-Run
    exit 1
}

# Runs a CLI; stdout is JSON, stderr is appended to cli-progression.txt.
function Cli([string] $program, [string[]] $arguments) {
    $stdout = "$out\cli-out.json"; $stderr = "$out\cli-err.txt"
    Write-Host "$([IO.Path]::GetFileName($program)) $($arguments -join ' ')"
    $p = Start-Process $program -ArgumentList $arguments -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    Get-Content $stderr -ErrorAction SilentlyContinue | Add-Content "$out\cli-progression.txt"
    if ($p.ExitCode -ne 0) { Write-Warning "code de sortie $($p.ExitCode)"; return $null }
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

# State of a registry key: its .reg export, values sorted within each key.
function RegState([string] $key) {
    $file = "$out\state.reg"
    & reg.exe export $key $file /y 2>$null | Out-Null
    if ($LASTEXITCODE -ne 0) { return '<absent>' }
    $sections = @(); $current = $null
    foreach ($line in Get-Content $file) {
        if ($line -match '^\[.*\]$') { if ($current) { $sections += $current }; $current = @{ Head = $line; Values = @() } }
        elseif ($current -and $line.Trim()) { $current.Values += $line }
    }
    if ($current) { $sections += $current }
    ($sections | ForEach-Object { $_.Head; $_.Values | Sort-Object }) -join "`n"
}

# Round trip through the quarantine of everything matching $match, in two steps.
function RoundTrip([string] $case, [string] $match, [scriptblock] $state, [scriptblock] $isGone, [string] $note) {
    $ctx = @{ Before = (& $state); Session = $null; Gone = $false; After = $null }
    Invoke-Step "$case : mise en quarantaine" {
        $ctx.Session = Cli $sandboxCli @('sandbox', 'quarantine', $match)
        Start-Sleep -Seconds 2
        $ctx.Gone = [bool](& $isGone)
        Write-Host "retiré : $($ctx.Gone)"
    }
    if ($ctx.Session) {
        Invoke-Step "$case : restauration" {
            $restored = Cli $sandboxCli @('sandbox', 'restore', $ctx.Session.id)
            if ($restored -and $restored.errors.Count) { Write-Warning ($restored.errors | ConvertTo-Json -Compress) }
            Start-Sleep -Seconds 2
            $ctx.After = & $state
        }
    }
    $same = $ctx.Session -and ($ctx.Before -eq $ctx.After)
    if ($ctx.Session -and -not $same) {
        $slug = $case -replace '\W', '_'
        Set-Content "$out\$slug-avant.txt" $ctx.Before -Encoding utf8
        Set-Content "$out\$slug-apres.txt" $ctx.After -Encoding utf8
    }
    $detail = if ($ctx.Session) { "$note ; session $($ctx.Session.id), $($ctx.Session.items.Count) élément(s)" } else { "$note ; non mis en quarantaine" }
    $script:checks += [pscustomobject]@{ Name = "$case : retiré par la quarantaine"; Measure = $detail; Status = $(if ($ctx.Gone) { 'ok' } else { 'ko' }) }
    $script:checks += [pscustomobject]@{ Name = "$case : restauré à l'identique"; Measure = $(if ($same) { 'état identique' } else { 'états différents (fichiers -avant / -apres)' }); Status = $(if ($same) { 'ok' } else { 'ko' }) }
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
        $apps = Cli $cli @('apps', '--json')
        $store.Listed = @($apps | Where-Object { $_.kind -eq 'store' -and $_.package_family_name -eq $package.PackageFamilyName }).Count
        $store.Removed = Cli $sandboxCli @('sandbox', 'uninstall', 'CleanSlate Test App')
        $events = @(Get-Content "$out\cli-progression.txt" -ErrorAction SilentlyContinue | Where-Object { $_ -like '{*"step"*' })
        $store.Percents = @($events | ForEach-Object { ($_ | ConvertFrom-Json).percent } | Where-Object { $_ -ne $null })
    }
    $still = [bool](Get-AppxPackage -Name 'CleanSlateTest.App')
    $checks += [pscustomobject]@{ Name = 'Paquet MSIX : listé (badge Store)'; Measure = "famille $($package.PackageFamilyName)"; Status = $(if ($store.Listed -eq 1) { 'ok' } else { 'ko' }) }
    $checks += [pscustomobject]@{ Name = "Paquet MSIX : désinstallé pour l'utilisateur, avec progression"; Measure = "$($store.Percents.Count) relevés de progression, dernier $($store.Percents | Select-Object -Last 1) %"; Status = $(if ($store.Removed -and -not $still -and $store.Percents.Count) { 'ok' } else { 'ko' }) }
}
else {
    $checks += [pscustomobject]@{ Name = 'Paquet MSIX : installation'; Measure = 'Add-AppxPackage a échoué (voir journal.txt)'; Status = 'ko' }
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

# 6. VLC (MSI) and GIMP (Inno Setup) for the manual checks, from the verified installers.
$installers = @(Test-Installer "$Repo\scripts\sandbox\manifeste.json" "$Repo\artifacts\sandbox\installers" 'complements')
$bad = @($installers | Where-Object { -not $_.Ok })
$checks += [pscustomobject]@{ Name = 'Installeurs VLC et GIMP conformes au manifeste'; Measure = "$($installers.Count - $bad.Count) / $($installers.Count) SHA-256"; Status = $(if ($installers.Count -and -not $bad.Count) { 'ok' } else { 'ko' }) }
if (-not $bad.Count) {
    foreach ($i in $installers) {
        Invoke-Step "Installation de $($i.Name) $($i.Version) (vérifications manuelles)" {
            $line = $i.Install.Replace('<fichier>', "`"$($i.Path)`"")
            $program, $arguments = Split-Command $line
            Write-Host "$program $arguments"
            $p = Start-Process $program $arguments -Wait -PassThru
            Write-Host "code de sortie $($p.ExitCode)"
        }
    }
}
$checks += [pscustomobject]@{ Name = 'MSI (VLC) : pourcentage et action à l''étape 2 ; GIMP (Inno Setup) : liste des processus (EF-21) ; filtre « Store »'; Measure = 'dans CleanSlate (Bureau), mode simulation désactivé'; Status = 'manuel' }

Write-Report 'Compléments v1.0 — résultats dans Windows Sandbox' $meta $checks ([ordered]@{
        'Installeurs vérifiés' = @($installers | ForEach-Object { "$(if ($_.Ok) { 'conforme' } else { 'NON CONFORME' }) — $($_.Name) $($_.Version) — SHA-256 $($_.Actual)" })
    })
Stop-Run
Start-Process "$out\rapport.html"
exit 0
