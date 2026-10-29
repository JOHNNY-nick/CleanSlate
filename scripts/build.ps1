<#
.SYNOPSIS
    Builds CleanSlate: the Rust engine (cleanslate.dll) first, then the .NET solution.

.DESCRIPTION
    1. cargo build of the Cargo workspace (also regenerates NativeMethods.g.cs).
    2. dotnet build of app/CleanSlate.sln for x64. MSBuild copies cleanslate.dll
       from core/target/<profile> next to the app and the tests.
    -Configuration applies to both: Debug = cargo "dev" profile, Release = --release.

.EXAMPLE
    pwsh scripts/build.ps1 -Configuration Debug
#>
[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string] $Configuration = 'Debug'
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

# rustup installs cargo in ~/.cargo/bin, which is not always on PATH yet.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $HOME '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:Path = "$cargoBin;$env:Path"
    }
    else {
        throw 'cargo not found: install Rust with rustup (https://rustup.rs).'
    }
}
if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
    throw 'dotnet not found: install the .NET 8 SDK.'
}

$cargoArgs = @('build', '--workspace')
$profileDir = 'debug'
if ($Configuration -eq 'Release') {
    $cargoArgs += '--release'
    $profileDir = 'release'
}

Push-Location (Join-Path $root 'core')
try {
    Invoke-Step "cargo $($cargoArgs -join ' ')" { cargo @cargoArgs }
}
finally {
    Pop-Location
}

$dll = Join-Path $root "core\target\$profileDir\cleanslate.dll"
if (-not (Test-Path $dll)) {
    throw "cleanslate.dll was not produced at $dll."
}

$solution = Join-Path $root 'app\CleanSlate.sln'
Invoke-Step "dotnet build ($Configuration|x64)" {
    dotnet build $solution -c $Configuration -p:Platform=x64 --nologo
}

$exe = Join-Path $root "app\CleanSlate.App\bin\x64\$Configuration\net8.0-windows10.0.19041.0\win-x64\CleanSlate.App.exe"
Write-Host "==> Done: $exe" -ForegroundColor Green
