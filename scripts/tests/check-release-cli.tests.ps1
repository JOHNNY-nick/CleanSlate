<#
.SYNOPSIS
    Tests of scripts/check-release-cli.ps1 (D53): checks the real exit code, not the message.

.DESCRIPTION
    Builds cleanslate-cli twice, without ever running it:
    - clean: the release binary (core\target\release), built if missing;
    - with the "sandbox-tests" feature, in core\target\feature-check (never copied nor run).
    Then checks the exit code of check-release-cli.ps1:
    - as a separate process (pwsh -File): clean -> 0, feature -> 1, missing binary -> 2;
    - called with & in this session, after $LASTEXITCODE was left $null or 1 by what ran
      before (the defect of 30 Oct. 2026: prepare.ps1 read $null and refused a clean binary).
    The check is never run with -RunHelp here: no CLI binary is ever started.
    Exit code 0 when every case passes, 1 otherwise.

.EXAMPLE
    pwsh scripts/tests/check-release-cli.tests.ps1
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$check = Join-Path $root 'scripts\check-release-cli.ps1'
$clean = Join-Path $root 'core\target\release\cleanslate-cli.exe'
$featureDir = Join-Path $root 'core\target\feature-check'
$feature = Join-Path $featureDir 'release\cleanslate-cli.exe'
$missing = Join-Path $featureDir 'does-not-exist.exe'

Push-Location $root
try {
    if (-not (Test-Path $clean)) {
        cargo build --manifest-path core/Cargo.toml -p cleanslate-cli --release
        if ($LASTEXITCODE -ne 0) { Write-Host "cargo build (clean) failed: $LASTEXITCODE" -ForegroundColor Red; exit 1 }
    }
    cargo build --manifest-path core/Cargo.toml -p cleanslate-cli --release --features sandbox-tests --target-dir $featureDir
    if ($LASTEXITCODE -ne 0) { Write-Host "cargo build (feature) failed: $LASTEXITCODE" -ForegroundColor Red; exit 1 }
}
finally { Pop-Location }

# Exit code of the check run as its own process.
function Invoke-Process([string] $binary) {
    pwsh -NoProfile -File $check -Binary $binary | Out-Null
    $LASTEXITCODE
}

# Exit code seen by a caller in the same session, with $LASTEXITCODE preset as an earlier
# command (or none) would have left it.
function Invoke-InSession([string] $binary, $preset) {
    $global:LASTEXITCODE = $preset
    & $check -Binary $binary | Out-Null
    $LASTEXITCODE
}

$cases = @(
    @{ Name = 'clean binary, separate process'; Expected = 0; Actual = (Invoke-Process $clean) },
    @{ Name = 'feature binary, separate process'; Expected = 1; Actual = (Invoke-Process $feature) },
    @{ Name = 'missing binary, separate process'; Expected = 2; Actual = (Invoke-Process $missing) },
    @{ Name = 'clean binary, same session, $LASTEXITCODE was $null'; Expected = 0; Actual = (Invoke-InSession $clean $null) },
    @{ Name = 'clean binary, same session, $LASTEXITCODE was 1'; Expected = 0; Actual = (Invoke-InSession $clean 1) },
    @{ Name = 'feature binary, same session, $LASTEXITCODE was 0'; Expected = 1; Actual = (Invoke-InSession $feature 0) },
    @{ Name = 'missing binary, same session, $LASTEXITCODE was 0'; Expected = 2; Actual = (Invoke-InSession $missing 0) }
)

$failed = 0
foreach ($c in $cases) {
    $ok = $c.Actual -is [int] -and $c.Actual -eq $c.Expected
    if (-not $ok) { $failed++ }
    $actual = if ($null -eq $c.Actual) { '$null' } else { $c.Actual }
    Write-Host ("{0}  {1} (expected {2}, got {3})" -f $(if ($ok) { 'PASS' } else { 'FAIL' }), $c.Name, $c.Expected, $actual) `
        -ForegroundColor $(if ($ok) { 'Green' } else { 'Red' })
}
if ($failed) {
    Write-Host "$failed / $($cases.Count) case(s) failed." -ForegroundColor Red
    exit 1
}
Write-Host "$($cases.Count) / $($cases.Count) cases passed." -ForegroundColor Green
exit 0
