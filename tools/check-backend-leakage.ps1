param(
    [string]$Binary = 'src-tauri/target/release/wincommander-free.exe'
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$binaryPath = Join-Path $repo $Binary
if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
    throw "Release binary not found: $binaryPath"
}

$bytes = [IO.File]::ReadAllBytes($binaryPath)
$asciiView = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
$utf16View = [Text.Encoding]::Unicode.GetString($bytes)
$moduleRoot = Join-Path $repo 'src-tauri/commander-free/scripts'
$failures = [Collections.Generic.List[string]]::new()
$checked = 0

Get-ChildItem -LiteralPath $moduleRoot -Recurse -File -Filter '*.ps1' | ForEach-Object {
    $module = $_
    $lines = @(Get-Content -LiteralPath $module.FullName)
    $candidates = [Collections.Generic.List[string]]::new()
    for ($index = 0; $index -le $lines.Count - 3 -and $candidates.Count -lt 3; $index++) {
        $window = @($lines[$index..($index + 2)])
        $candidate = $window -join "`n"
        $meaningful = @($window | Where-Object {
            $line = $_.Trim()
            $line.Length -gt 0 -and -not $line.StartsWith('#')
        }).Count
        if ($meaningful -gt 0 -and $candidate.Length -ge 128 -and $candidate -cmatch '^[\x09\x0A\x20-\x7E]+$') {
            $candidates.Add($candidate)
        }
    }
    foreach ($candidate in $candidates) {
        $checked++
        $windowsCandidate = $candidate.Replace("`n", "`r`n")
        if (
            $asciiView.Contains($candidate) -or $asciiView.Contains($windowsCandidate) -or
            $utf16View.Contains($candidate) -or $utf16View.Contains($windowsCandidate)
        ) {
            $failures.Add($module.FullName.Substring($repo.Length + 1))
            break
        }
    }
}

if ($checked -eq 0) { throw 'No protected plaintext probes were available.' }
if ($failures.Count -gt 0) {
    $failures | Sort-Object -Unique | ForEach-Object { Write-Error "Protected plaintext detected from module: $_" }
    throw 'Release binary contains protected backend plaintext.'
}
Write-Host "Protected backend plaintext leakage check passed ($checked plaintext probes)."
