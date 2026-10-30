<#
.SYNOPSIS
    Tests of the way the Sandbox scripts call a program (common.ps1, D56). Runs under Windows
    PowerShell 5.1 (the one of the Sandbox) and PowerShell 7:
        powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/tests/sandbox-arguments.tests.ps1
        pwsh -NoProfile -File scripts/tests/sandbox-arguments.tests.ps1

.DESCRIPTION
    No CleanSlate program is ever run. A fake program (echo-args.exe, compiled here with the
    csc of the .NET Framework into a temporary folder) only prints the arguments it received.
    1. Invoke-Program gives it the exact arguments of the scenarios (and edge cases: spaces,
       quotes, trailing backslash, accents, empty): each must arrive whole, in order.
    2. The old call (Start-Process -ArgumentList with an array) splits "CleanSlate Test App"
       into three arguments: the test proves it can see that defect.
    3. No function of the scripts has the name of an alias of this PowerShell ("Cli" is an
       alias of Clear-Item: the failure of 30 Oct. 2026).
    Exit code 0 when every case passes, 1 otherwise.
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $root 'scripts\sandbox\common.ps1')

$work = Join-Path ([IO.Path]::GetTempPath()) ('cleanslate-args-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $work | Out-Null
$failed = 0
function Check([string] $name, [bool] $ok, [string] $detail) {
    if (-not $ok) { $script:failed++ }
    $status = if ($ok) { 'PASS' } else { 'FAIL' }
    $color = if ($ok) { 'Green' } else { 'Red' }
    Write-Host "$status  $name$(if (-not $ok -and $detail) { " — $detail" })" -ForegroundColor $color
}

try {
    # The fake program: one line per argument, "arg:" + base64 of its UTF-8 bytes (no escaping issue;
    # the prefix keeps an empty argument as a non-empty line).
    $source = Join-Path $work 'echo-args.cs'
    $echo = Join-Path $work 'echo-args.exe'
    @'
using System;
using System.Text;
static class EchoArgs
{
    static void Main(string[] args)
    {
        foreach (string a in args) Console.Out.WriteLine("arg:" + Convert.ToBase64String(Encoding.UTF8.GetBytes(a)));
    }
}
'@ | Set-Content -LiteralPath $source -Encoding ascii
    $csc = Join-Path $env:windir 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
    & $csc /nologo /out:$echo $source | Out-Null
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $echo)) { Write-Host 'FAIL  compilation of echo-args.exe' -ForegroundColor Red; exit 1 }

    function Received([string[]] $arguments) {
        $code = Invoke-Program $echo $arguments "$work\out.txt" "$work\err.txt"
        $lines = @(Get-Content "$work\out.txt" -ErrorAction SilentlyContinue | Where-Object { $_ -like 'arg:*' })
        @{ Code = $code; Args = @($lines | ForEach-Object { [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($_.Substring(4))) }) }
    }

    $cases = @(
        @('sandbox', 'quarantine', 'CleanSlateTest Old Tool'),
        @('sandbox', 'quarantine', '\CleanSlateTest\Agent'),
        @('sandbox', 'quarantine', 'Services\CleanSlateTestSvc'),
        @('sandbox', 'quarantine', 'CleanSlateTest.Leftover'),
        @('sandbox', 'restore', '0f8e5a2c-3b1d-4c7e-9a6f-2d4b8c1e7f30'),
        @('sandbox', 'uninstall', 'CleanSlate Test App'),
        @('scan', '--dry-run', '--json'),
        @('apps', '--json'),
        @('a "quoted" word', 'trailing\', 'C:\Program Files\x\', 'C:\a\\b', '\\server\share\'),
        @('', 'é accents Démarrer « x »', "tab`tinside", 'end')
    )
    foreach ($expected in $cases) {
        $got = Received $expected
        $same = ($got.Code -eq 0) -and ($got.Args.Count -eq $expected.Count) -and
            (-not (Compare-Object -ReferenceObject $expected -DifferenceObject $got.Args -SyncWindow 0 -CaseSensitive))
        Check "Invoke-Program : $(Join-CommandLine $expected)" $same "reçu $($got.Args.Count) argument(s) : [$($got.Args -join '] [')]"
    }

    # The defect is visible: an array given to Start-Process is split on spaces.
    Start-Process -FilePath $echo -ArgumentList @('sandbox', 'uninstall', 'CleanSlate Test App') -NoNewWindow -Wait `
        -RedirectStandardOutput "$work\old.txt" -RedirectStandardError "$work\old-err.txt"
    $old = @(Get-Content "$work\old.txt" | Where-Object { $_ -like 'arg:*' })
    Check 'ancien appel (tableau à Start-Process) : le test voit la coupure' ($old.Count -eq 5) "reçu $($old.Count) argument(s)"

    # No function of the scripts may be hidden by an alias.
    $aliases = @(Get-Alias | ForEach-Object Name)
    $scripts = Get-ChildItem (Join-Path $root 'scripts') -Recurse -Filter *.ps1 -File
    $hidden = foreach ($f in $scripts) {
        $tokens = $null; $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$tokens, [ref]$errors)
        $ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $true) |
            Where-Object { $aliases -contains $_.Name } | ForEach-Object { "$($f.Name): $($_.Name)" }
    }
    Check "aucune fonction des scripts ne porte le nom d'un alias ($($scripts.Count) scripts)" (-not $hidden) ($hidden -join ', ')
    Check 'contrôle de cohérence : « cli » est bien un alias ici' ($aliases -contains 'cli') ''
}
finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "PowerShell $($PSVersionTable.PSVersion) : $(if ($failed) { "$failed échec(s)" } else { 'tout passe' })"
if ($failed) { exit 1 }
exit 0
