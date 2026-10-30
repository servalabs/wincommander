$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-schedule-staging-' + [guid]::NewGuid())
$oldLocalAppData = $env:LOCALAPPDATA
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
    # A previous generation may finish late and overwrite the shared marker.
    # Its future due date must not suppress the replacement's first run.
    $env:LOCALAPPDATA = Join-Path $tempRoot 'local'
    $stateRoot = Join-Path $env:LOCALAPPDATA 'WinCommander\auto-erase'
    New-Item -ItemType Directory -Path $stateRoot -Force | Out-Null
    $marker = Join-Path $stateRoot 'dnsCache.user-test.state.json'
    @{ payloadGeneration=('b' * 32); intervalMinutes=60; nextDueUtc=[DateTime]::UtcNow.AddDays(1).ToString('o') } | ConvertTo-Json -Compress | Set-Content -LiteralPath $marker
    $runArgument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script '$script:AutoEraseRemoved++' -ExecutionScope 'user-test' -FirstDueUtc ([DateTime]::UtcNow.AddMinutes(-1).ToString('o')) -PayloadRevision 17
    if ($runArgument -notmatch '-File "([^"]+)"$') { throw 'Executable test payload missing.' }
    $runPath = $Matches[1]
    & powershell -NoProfile -ExecutionPolicy Bypass -File $runPath
    if ($LASTEXITCODE -ne 0) { throw 'Synthetic scheduled payload failed.' }
    $result = Get-Content -LiteralPath (Join-Path $stateRoot 'dnsCache.user-test.result.json') -Raw | ConvertFrom-Json
    if ($result.removed -ne 1 -or $runPath -notmatch ('\.g' + $result.payloadGeneration + '\.ps1$')) { throw 'The current payload did not run and bind its result to its generation.' }
    Write-Output 'Passed: replacement payloads are staged separately without modifying the existing task payload.'
} finally {
    $env:LOCALAPPDATA = $oldLocalAppData
    if ((Split-Path -Parent $tempRoot) -eq [IO.Path]::GetTempPath().TrimEnd('\') -and (Split-Path -Leaf $tempRoot) -like 'wc-schedule-staging-*') {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
