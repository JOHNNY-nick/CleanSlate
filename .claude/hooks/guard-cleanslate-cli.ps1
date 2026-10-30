# PreToolUse hook (Bash and PowerShell tools): Claude may never run cleanslate-cli on this
# machine, except "scan --dry-run" (read-only). Decision D53, after the incident of 30 Oct. 2026
# (a "cleanslate-cli sandbox quarantine" call was attempted here and stopped by the tool).
# Exit code 2 blocks the tool call; the message on stderr is shown to Claude.
[Console]::InputEncoding = [Text.Encoding]::UTF8
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$raw = [Console]::In.ReadToEnd()
try { $data = $raw | ConvertFrom-Json } catch { exit 0 }
$command = [string]$data.tool_input.command
if (-not $command) { exit 0 }
$text = $command.ToLowerInvariant()

# Package arguments of cargo build/test/clippy are not runs of the CLI.
$scrubbed = [regex]::Replace($text, '(-p|--package)\s+cleanslate-cli\b', '')
# A run: "cargo run" of the CLI, or the binary followed (after options such as
# -ArgumentList and quotes) by one of its sub-commands. A folder path such as
# "core/cleanslate-cli scripts/..." is not a run.
$subcommand = '(version|apps|scan|help|sandbox|quarantine|restore|uninstall)\b'
$runsCli = ($text -match 'cargo\s+run\b' -and $text -match 'cleanslate-cli') -or
    ($scrubbed -match ('cleanslate-cli(-sandbox)?(\.exe)?[''"]?(\s+-\w+)*[\s''"@(,]+' + $subcommand))
if (-not $runsCli) { exit 0 }

$readOnlyScan = $text -match '\bscan\b' -and $text -match '--dry-run' -and
    $text -notmatch '\b(sandbox|quarantine|restore|uninstall|clean|purge)\b'
if ($readOnlyScan) { exit 0 }

[Console]::Error.WriteLine("Bloqué (D53) : cleanslate-cli ne se lance jamais sur cette machine, sauf « scan --dry-run ». Les actions réelles se testent uniquement dans Windows Sandbox (scripts/sandbox/README.md).")
exit 2
