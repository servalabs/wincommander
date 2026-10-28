$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('WinCommanderSchedulerTest-' + [Guid]::NewGuid().ToString('N'))
$savedLocalAppData = $env:LOCALAPPDATA
try {
    $null = New-Item -ItemType Directory -Path $testRoot -ErrorAction Stop
    $env:LOCALAPPDATA = $testRoot
    function Initialize-AutoEraseStorage { @{ scripts=$testRoot } }
    $cases = @(
        @{ id='dnsCache'; body='$script:AutoEraseRemoved = 1'; success=$true },
        @{ id='recentFiles'; body='Write-Error "Synthetic suppressed failure" -ErrorAction SilentlyContinue'; success=$false },
        @{ id='prefetchFiles'; body='throw "Synthetic terminating failure"'; success=$false },
        @{ id='pcaDatabase'; body='$script:AutoEraseDeferred = 1'; success=$false },
        @{ id='notepadState'; body='$script:AutoEraseRemoved = 1'; success=$true; approved=$true },
        @{ id='developerCaches'; body='$script:AutoEraseRemoved = 1'; success=$false; denied=$true }
    )
    foreach ($case in $cases) {
        $argument = ConvertTo-AutoEraseTaskArgument -CategoryId $case.id -IntervalMinutes 60 -Script $case.body -ExecutionScope 'user-test' -FirstDueUtc ([DateTime]::UtcNow.AddMinutes(-1).ToString('o')) -AllowDefaultExcluded ([bool]$case.approved)
        if ($argument -notmatch '-File "([^"]+)"$') { throw 'Generated task did not reference its script.' }
        $scriptPath = $Matches[1]
        # Execute only the synthetic body passed above, never a category's wipe payload.
        $savedPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        $null = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $scriptPath 2>&1
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $savedPreference
        $statePath = Join-Path $testRoot "WinCommander\auto-erase\$($case.id).user-test.state.json"
        if ($case.denied) {
            if ($exitCode -eq 0 -or (Test-Path -LiteralPath $statePath)) { throw 'An unapproved protected payload executed.' }
            continue
        }
        $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
        if ($state.success -ne $case.success -or (($exitCode -eq 0) -ne $case.success)) { throw "Receipt or task exit code disagrees for $($case.id)" }
        if ($case.success -and -not $state.lastSuccessfulUtc) { throw 'Success time was not recorded.' }
        if (-not $case.success -and $state.lastSuccessfulUtc) { throw 'An incomplete run advanced the success time.' }
    }
    Write-Output 'Passed: generated wrappers report synthetic outcomes truthfully and require explicit protected-category execution approval.'
} finally {
    $env:LOCALAPPDATA = $savedLocalAppData
    $resolved = [IO.Path]::GetFullPath($testRoot)
    $expectedParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
    if ([IO.Path]::GetDirectoryName($resolved) -ne $expectedParent -or [IO.Path]::GetFileName($resolved) -notlike 'WinCommanderSchedulerTest-*') { throw 'Test cleanup path validation failed.' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force -ErrorAction Stop }
}
