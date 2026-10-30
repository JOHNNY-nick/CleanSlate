<#
.SYNOPSIS
    Fails if a CLI binary contains the destructive "sandbox" commands (D53).

.DESCRIPTION
    The sandbox commands of cleanslate-cli are compiled only with the Cargo feature
    "sandbox-tests". This check proves that a release binary does not contain them:
    it searches the bytes of the file for markers that exist only in that code
    (the guard's variable, the Sandbox account, the command words). It never runs the
    binary, except with -RunHelp (CI only), which also checks that "help" does not list
    the commands. Exit code 1 when a marker is found.

.EXAMPLE
    pwsh scripts/check-release-cli.ps1
    pwsh scripts/check-release-cli.ps1 -Binary core/target/release/cleanslate-cli.exe -RunHelp
#>
[CmdletBinding()]
param(
    [string] $Binary = (Join-Path (Split-Path -Parent $PSScriptRoot) 'core\target\release\cleanslate-cli.exe'),
    [switch] $RunHelp
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Binary)) { throw "Binary not found: $Binary (build it first)." }

$markers = @('CLEANSLATE_SANDBOX_RUN', 'WDAGUtilityAccount', 'sandbox quarantine', 'restore <session-id>')
$text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($Binary))
$found = @($markers | Where-Object { $text.Contains($_) })

if ($RunHelp) {
    $help = & $Binary help 2>&1 | Out-String
    if ($help -match 'sandbox') { $found += 'help lists "sandbox"' }
}

if ($found.Count) {
    Write-Host "FAILED: $Binary contains the sandbox test commands: $($found -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host "OK: $Binary contains no sandbox test command." -ForegroundColor Green
