<#
.SYNOPSIS
    Prepares a Windows Sandbox run of the acceptance tests (tests-acceptation.md § 4 and § 5).

.DESCRIPTION
    Runs on the development PC and changes nothing on it outside artifacts\sandbox:
    1. downloads the installers of the test applications with "winget download"
       (nothing is installed here); winget is not available inside the Sandbox;
    2. copies cleanslate-cli.exe (read-only scan) next to them;
    3. writes artifacts\sandbox\CleanSlate-<Scenario>.wsb: the app, the scripts, the
       installers and the tools are shared read-only; only artifacts\sandbox\results
       is writable, so the report survives the Sandbox.
    Double-click the .wsb (or pass -Launch): inside the Sandbox, inside-apps.ps1 runs
    by itself. Build first: pwsh scripts/build.ps1 -Configuration Release.

.EXAMPLE
    pwsh scripts/sandbox/prepare.ps1 -Launch
#>
[CmdletBinding()]
param(
    [ValidateSet('Apps', 'Installer', 'Complements')]
    [string] $Scenario = 'Apps',
    # Applications of the scenario (tests-acceptation.md § 4.1); the last one is kept installed.
    [string[]] $Packages = @('VideoLAN.VLC', '7zip.7zip', 'Notepad++.Notepad++', 'Audacity.Audacity', 'GIMP.GIMP.3', 'Mozilla.Firefox'),
    [switch] $Launch
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $root 'artifacts\sandbox'
$installers = Join-Path $out 'installers'
$tools = Join-Path $out 'tools'
$results = Join-Path $out 'results'
New-Item -ItemType Directory -Force $installers, $tools, $results | Out-Null

$app = Join-Path $root 'app\CleanSlate.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64'
$cli = Join-Path $root 'core\target\release\cleanslate-cli.exe'
$setup = Join-Path $root 'artifacts\installer'
foreach ($needed in $app, $cli) {
    if (-not (Test-Path $needed)) { throw "Missing $needed : run scripts/build.ps1 -Configuration Release first." }
}
Copy-Item $cli $tools -Force

if ($Scenario -eq 'Apps') {
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    $wingetPath = if ($winget) { $winget.Source } else { Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\winget.exe' }
    foreach ($id in $Packages) {
        $folder = Join-Path $installers $id
        if (Get-ChildItem $folder -Filter *.yaml -ErrorAction SilentlyContinue) {
            Write-Host "== $id : already downloaded"
            continue
        }
        Write-Host "== $id : download"
        & $wingetPath download --id $id --exact --source winget --architecture x64 --download-directory $folder `
            --accept-package-agreements --accept-source-agreements --disable-interactivity
        if ($LASTEXITCODE -ne 0) { throw "winget download $id failed ($LASTEXITCODE)" }
    }
}
elseif ($Scenario -eq 'Installer' -and -not (Get-ChildItem $setup -Filter 'CleanSlate-*-setup.exe' -ErrorAction SilentlyContinue)) {
    throw "No installer in $setup : run scripts/package.ps1 first."
}

# Complements (§ 6): makeappx and signtool of the Windows SDK build and sign the test
# MSIX inside the Sandbox; their folder is shared read-only, nothing is signed here.
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1 | ForEach-Object DirectoryName
if ($Scenario -eq 'Complements' -and -not $sdk) { throw 'makeappx.exe not found: install the Windows SDK.' }

function Mapped([string] $host_, [string] $inside, [bool] $readOnly) {
    "    <MappedFolder><HostFolder>$host_</HostFolder><SandboxFolder>$inside</SandboxFolder><ReadOnly>$($readOnly.ToString().ToLower())</ReadOnly></MappedFolder>"
}
$folders = @(
    (Mapped $app 'C:\CleanSlate\app' $true),
    (Mapped $PSScriptRoot 'C:\CleanSlate\scripts' $true),
    (Mapped $tools 'C:\CleanSlate\tools' $true),
    (Mapped $results 'C:\CleanSlate\results' $false)
)
switch ($Scenario) {
    'Apps' { $folders += Mapped $installers 'C:\CleanSlate\installers' $true }
    'Installer' { $folders += Mapped $setup 'C:\CleanSlate\setup' $true }
    'Complements' { $folders += Mapped $sdk 'C:\CleanSlate\sdk' $true }
}
$script = @{ Apps = 'inside-apps.ps1'; Installer = 'inside-installer.ps1'; Complements = 'inside-complements.ps1' }[$Scenario]
$wsb = Join-Path $out "CleanSlate-$Scenario.wsb"
@"
<Configuration>
  <MappedFolders>
$($folders -join "`r`n")
  </MappedFolders>
  <Networking>Disable</Networking>
  <LogonCommand>
    <Command>powershell.exe -ExecutionPolicy Bypass -NoProfile -File C:\CleanSlate\scripts\$script</Command>
  </LogonCommand>
</Configuration>
"@ | Set-Content -Path $wsb -Encoding utf8
Write-Host "Sandbox configuration: $wsb"
Write-Host "Report: $results"

if ($Launch) {
    $sandbox = Join-Path $env:windir 'System32\WindowsSandbox.exe'
    if (-not (Test-Path $sandbox)) { throw 'Windows Sandbox is not enabled (tests-acceptation.md § 1).' }
    Start-Process $wsb
}
