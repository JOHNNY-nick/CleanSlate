<#
.SYNOPSIS
    Prepares a Windows Sandbox run of the acceptance tests (scripts/sandbox/README.md).

.DESCRIPTION
    Runs on the development PC. It installs nothing and runs nothing of CleanSlate here;
    it only writes under artifacts\sandbox (and core\target\sandbox-tests):
    1. checks the Release build (app and cleanslate-cli.exe) and copies the CLI to
       artifacts\sandbox\tools (read-only scan in the Sandbox);
    2. Complements only: builds cleanslate-cli with the sandbox-tests feature
       (core\target\sandbox-tests) and copies it as tools\cleanslate-cli-sandbox.exe;
       for the other scenarios that file is removed, so it is never shared needlessly;
    3. downloads the installers listed in manifeste.json from their official URL
       (artifacts\sandbox\installers\<id>\) unless already there, and stops if one
       SHA-256 differs from the manifest; Installer: records the SHA-256 of the setup;
    4. writes a new random token (32 bytes) to artifacts\sandbox\run\token.txt and
       artifacts\sandbox\run\prepare.json (date, commit, SHA-256 of what is shared);
    5. writes artifacts\sandbox\CleanSlate-<Scenario>.wsb: the repository is shared
       read-only at C:\CleanSlate\repo, the only writable folder is
       artifacts\sandbox\results (C:\CleanSlate\results), network off; the logon command
       sets CLEANSLATE_SANDBOX_RUN=<token> and starts scripts\sandbox\inside-*.ps1.
    Each preparation replaces the token: an older .wsb no longer passes the guard.
    Build first: pwsh scripts/build.ps1 -Configuration Release.

.EXAMPLE
    pwsh scripts/sandbox/prepare.ps1 -Scenario Complements -Launch
#>
[CmdletBinding()]
param(
    [ValidateSet('Apps', 'Installer', 'Complements')]
    [string] $Scenario = 'Apps',
    [switch] $Launch
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $root 'artifacts\sandbox'
$installers = Join-Path $out 'installers'
$toolsDir = Join-Path $out 'tools'
$resultsDir = Join-Path $out 'results'
$runDir = Join-Path $out 'run'
New-Item -ItemType Directory -Force $installers, $toolsDir, $resultsDir, $runDir | Out-Null

function Step([string] $text) { Write-Host "[$(Get-Date -Format 'HH:mm:ss')] $text" -ForegroundColor Cyan }

if (Get-Process -Name 'WindowsSandbox*' -ErrorAction SilentlyContinue) {
    throw 'A Windows Sandbox is open: close it first (one Sandbox at a time; a new token would also break its run).'
}

# 1. Release build.
Step 'Release build'
$app = Join-Path $root 'app\CleanSlate.App\bin\x64\Release\net8.0-windows10.0.19041.0\win-x64'
$cli = Join-Path $root 'core\target\release\cleanslate-cli.exe'
foreach ($needed in "$app\CleanSlate.App.exe", $cli) {
    if (-not (Test-Path $needed)) { throw "Missing $needed : run scripts/build.ps1 -Configuration Release first." }
}
# Separate process: its exit code is the one of the check (0 clean, 1 markers, 2 error),
# never a value left by an earlier command of this session.
pwsh -NoProfile -File (Join-Path $root 'scripts\check-release-cli.ps1') -Binary $cli
switch ($LASTEXITCODE) {
    0 { }
    1 { throw 'The release cleanslate-cli.exe contains the test commands (check-release-cli.ps1).' }
    default { throw "check-release-cli.ps1 could not run (exit code $LASTEXITCODE)." }
}
Copy-Item $cli $toolsDir -Force

# 2. Test commands, for the Complements scenario only (D53).
$sandboxCli = Join-Path $toolsDir 'cleanslate-cli-sandbox.exe'
if ($Scenario -eq 'Complements') {
    Step 'cleanslate-cli with the sandbox-tests feature (core\target\sandbox-tests)'
    Push-Location $root   # the static C runtime comes from .cargo\config.toml at the root (D51)
    try {
        cargo build --manifest-path core/Cargo.toml -p cleanslate-cli --release --features sandbox-tests --target-dir core/target/sandbox-tests
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
    }
    finally { Pop-Location }
    Copy-Item (Join-Path $root 'core\target\sandbox-tests\release\cleanslate-cli.exe') $sandboxCli -Force
}
elseif (Test-Path $sandboxCli) {
    Remove-Item -LiteralPath $sandboxCli
}

# 3. Installers of the manifest, then their SHA-256.
$manifest = Join-Path $PSScriptRoot 'manifeste.json'
$scenarioKey = $Scenario.ToLowerInvariant()
foreach ($row in Test-Installer $manifest $installers $scenarioKey | Where-Object { -not $_.Ok }) {
    if ($row.Actual) { throw "$($row.Id) : SHA-256 $($row.Actual) differs from the manifest ($($row.Expected)). Delete $($row.Path) to download it again." }
    Step "Download $($row.Name) $($row.Version) : $($row.Url)"
    New-Item -ItemType Directory -Force (Split-Path $row.Path) | Out-Null
    $partial = "$($row.Path).partial"
    Invoke-WebRequest -Uri $row.Url -OutFile $partial -UseBasicParsing
    $hash = (Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash
    if ($hash -ne $row.Expected) {
        Remove-Item -LiteralPath $partial
        throw "$($row.Id) : downloaded SHA-256 $hash differs from the manifest ($($row.Expected)); nothing kept."
    }
    Move-Item -LiteralPath $partial $row.Path -Force
}
$checked = @(Test-Installer $manifest $installers $scenarioKey)
foreach ($row in $checked) {
    if (-not $row.Ok) { throw "$($row.Id) : SHA-256 check failed." }
    Write-Host "  OK  $($row.Name) $($row.Version)  $($row.Actual)"
}

$setup = $null
if ($Scenario -eq 'Installer') {
    $setup = Get-ChildItem (Join-Path $root 'artifacts\installer') -Filter 'CleanSlate-*-setup.exe' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $setup) { throw 'No installer in artifacts\installer : run scripts/package.ps1 first.' }
}

# Complements: makeappx and signtool of the Windows SDK, shared read-only.
$sdk = $null
if ($Scenario -eq 'Complements') {
    $sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1 | ForEach-Object DirectoryName
    if (-not $sdk) { throw 'makeappx.exe not found: install the Windows SDK.' }
}

# 4. Token of this run and description of what is shared.
Step 'Run token'
$bytes = New-Object byte[] 32
[System.Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
$token = -join ($bytes | ForEach-Object { $_.ToString('x2') })
[IO.File]::WriteAllText((Join-Path $runDir 'token.txt'), $token)
$commit = (git -C $root rev-parse --short HEAD 2>$null)
$dirty = [bool](git -C $root status --porcelain 2>$null)
$shared = [ordered]@{}
foreach ($f in @($cli, "$app\CleanSlate.App.exe", "$app\cleanslate.dll") + @(if ($Scenario -eq 'Complements') { $sandboxCli }) + @(if ($setup) { $setup.FullName })) {
    $shared[$f.Substring($root.Length + 1)] = (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash
}
[ordered]@{
    scenario    = $Scenario
    prepare     = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    commit      = if ($dirty) { "$commit (modifications non commitées)" } else { $commit }
    sha256      = $shared
    installeurs = @($checked | ForEach-Object { [ordered]@{ id = $_.Id; version = $_.Version; sha256 = $_.Actual } })
} | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $runDir 'prepare.json') -Encoding utf8

# 5. Sandbox configuration.
function Mapped([string] $hostFolder, [string] $inside, [bool] $readOnly) {
    "    <MappedFolder><HostFolder>$hostFolder</HostFolder><SandboxFolder>$inside</SandboxFolder><ReadOnly>$($readOnly.ToString().ToLower())</ReadOnly></MappedFolder>"
}
$folders = @(
    (Mapped $root 'C:\CleanSlate\repo' $true),
    (Mapped $resultsDir 'C:\CleanSlate\results' $false)
)
if ($sdk) { $folders += Mapped $sdk 'C:\CleanSlate\sdk' $true }
$script = @{ Apps = 'inside-apps.ps1'; Installer = 'inside-installer.ps1'; Complements = 'inside-complements.ps1' }[$Scenario]
$command = "powershell.exe -NoExit -ExecutionPolicy Bypass -NoProfile -Command `"`$env:CLEANSLATE_SANDBOX_RUN='$token'; &amp; 'C:\CleanSlate\repo\scripts\sandbox\$script'`""
$wsb = Join-Path $out "CleanSlate-$Scenario.wsb"
@"
<Configuration>
  <MappedFolders>
$($folders -join "`r`n")
  </MappedFolders>
  <Networking>Disable</Networking>
  <LogonCommand>
    <Command>$command</Command>
  </LogonCommand>
</Configuration>
"@ | Set-Content -Path $wsb -Encoding utf8
Write-Host "Sandbox configuration: $wsb"
Write-Host "Results: $resultsDir\$($Scenario.ToLowerInvariant())-<date>\rapport.html"

if ($Launch) {
    $sandbox = Join-Path $env:windir 'System32\WindowsSandbox.exe'
    if (-not (Test-Path $sandbox)) { throw 'Windows Sandbox is not enabled (scripts/sandbox/README.md).' }
    Start-Process $wsb
}
exit 0
