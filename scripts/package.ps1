<#
.SYNOPSIS
    Builds the CleanSlate installer (Inno Setup) in Release.

.DESCRIPTION
    1. scripts/build.ps1 -Configuration Release (Rust engine + .NET solution).
    2. dotnet publish of CleanSlate.App, self-contained for win-x64: the .NET 8
       runtime and the Windows App SDK runtime ship with the application, so the
       target PC needs nothing else (spec § 10.4).
    3. ISCC compiles installer/cleanslate.iss into artifacts/installer.
    Inno Setup 6 must be installed (winget install JRSoftware.InnoSetup).

.EXAMPLE
    pwsh scripts/package.ps1
#>
[CmdletBinding()]
param(
    [switch] $SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Invoke-Step {
    param([string] $Title, [scriptblock] $Command)
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "Step failed ($Title), exit code $LASTEXITCODE."
    }
}

function Find-Iscc {
    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }
    throw 'ISCC.exe not found: install Inno Setup 6 (winget install JRSoftware.InnoSetup).'
}

if (Get-Process CleanSlate.App -ErrorAction SilentlyContinue) {
    throw 'Close CleanSlate.App.exe first: it locks its bin folder.'
}

if (-not $SkipBuild) {
    Invoke-Step 'build.ps1 -Configuration Release' {
        & (Join-Path $PSScriptRoot 'build.ps1') -Configuration Release
        $global:LASTEXITCODE = 0
    }
}

$publishDir = Join-Path $root 'artifacts\publish'
if (Test-Path $publishDir) {
    # Only our own build output: artifacts/ is ignored by Git.
    Remove-Item $publishDir -Recurse -Force
}

$project = Join-Path $root 'app\CleanSlate.App\CleanSlate.App.csproj'
Invoke-Step 'dotnet publish (Release, win-x64, self-contained)' {
    dotnet publish $project -c Release -p:Platform=x64 -r win-x64 --self-contained true -o $publishDir --nologo
}

foreach ($required in 'CleanSlate.App.exe', 'cleanslate.dll') {
    if (-not (Test-Path (Join-Path $publishDir $required))) {
        throw "$required is missing from $publishDir."
    }
}

$version = (Select-String -Path (Join-Path $root 'core\Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$iscc = Find-Iscc
Invoke-Step "ISCC (version $version)" {
    & $iscc "/DAppVersion=$version" "/DPublishDir=$publishDir" (Join-Path $root 'installer\cleanslate.iss')
}

$setup = Join-Path $root "artifacts\installer\CleanSlate-$version-setup.exe"
Write-Host "==> Done: $setup" -ForegroundColor Green
