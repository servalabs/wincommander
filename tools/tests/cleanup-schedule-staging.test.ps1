$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-schedule-staging-' + [guid]::NewGuid())
try {
    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    function Initialize-AutoEraseStorage { @{ scripts = $tempRoot } }
    $due = [DateTime]::UtcNow.AddMinutes(60).ToString('o')
    $first = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script '# First payload' -ExecutionScope 'user-test' -FirstDueUtc $due -PayloadRevision 16
    if ($first -notmatch '-File "([^"]+)"$') { throw 'First payload path was not returned.' }
    $firstPath = $Matches[1]
    $firstBytes = [Convert]::ToBase64String([IO.File]::ReadAllBytes($firstPath))
    $second = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 120 -Script '# Second payload' -ExecutionScope 'user-test' -FirstDueUtc $due -PayloadRevision 16
    if ($second -notmatch '-File "([^"]+)"$') { throw 'Second payload path was not returned.' }
    $secondPath = $Matches[1]
    if ($firstPath -eq $secondPath) { throw 'Replacement reused the currently registered payload path.' }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($firstPath)) -ne $firstBytes) { throw 'Staging a replacement changed the previous payload.' }
    if (-not [IO.File]::ReadAllText($secondPath).Contains('# Second payload')) { throw 'The new payload was not staged.' }
    Write-Output 'Passed: replacement payloads are staged separately without modifying the existing task payload.'
} finally {
    if ((Split-Path -Parent $tempRoot) -eq [IO.Path]::GetTempPath().TrimEnd('\') -and (Split-Path -Leaf $tempRoot) -like 'wc-schedule-staging-*') {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
