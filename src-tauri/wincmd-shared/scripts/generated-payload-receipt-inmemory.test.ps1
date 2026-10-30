$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Exercise the production script generator and receipt reader while replacing
# only their static filesystem calls with a synthetic in-memory store. No
# scheduled task, directory, file, or cleanup target is created or removed.
$script:fixtureFiles = @{}
$script:fixtureWritten = [DateTime]::UtcNow.AddMinutes(-1)
$oldProgramData = $env:ProgramData
$oldLocalAppData = $env:LOCALAPPDATA
try {
    $env:ProgramData = 'C:\wc-synthetic\program'
    $env:LOCALAPPDATA = 'C:\wc-synthetic\local'
    $script:SystemMaintenanceTaskPath = '\System Maintenance\'
    function Initialize-AutoEraseStorage { @{ scripts = (Join-Path $env:ProgramData 'WinCommander\auto-erase\scripts') } }
    function Save-WCFixtureText([string]$Path, [string]$Text) { $script:fixtureFiles[[IO.Path]::GetFullPath($Path)] = $Text }
    function Read-WCFixtureText([string]$Path) {
        $key = [IO.Path]::GetFullPath($Path)
        if (-not $script:fixtureFiles.ContainsKey($key)) { throw 'Synthetic file is missing.' }
        return $script:fixtureFiles[$key]
    }
    function Get-Item {
        [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
        $text = Read-WCFixtureText $LiteralPath
        [pscustomobject]@{ PSIsContainer=$false; Attributes=0; Length=[Text.Encoding]::UTF8.GetByteCount($text); LastWriteTimeUtc=$script:fixtureWritten }
    }

    $generator = ${function:ConvertTo-AutoEraseTaskArgument}.ToString()
    $write = '[System.IO.File]::WriteAllText($scriptPath, $fullScript, [System.Text.Encoding]::UTF8)'
    if ($generator.Split(@($write), [StringSplitOptions]::None).Count -ne 2) { throw 'Review the generator before replacing its write call.' }
    $generator = $generator.Replace($write, 'Save-WCFixtureText $scriptPath $fullScript')
    Set-Item -LiteralPath Function:ConvertTo-AutoEraseTaskArgument -Value ([scriptblock]::Create($generator))
    $reader = ${function:Get-AutoEraseRecordedOutcome}.ToString()
    $read = '[IO.File]::ReadAllText('
    if ($reader.Split(@($read), [StringSplitOptions]::None).Count -ne 3) { throw 'Review the receipt reader before replacing its read calls.' }
    $reader = $reader.Replace($read, 'Read-WCFixtureText (')
    Set-Item -LiteralPath Function:Get-AutoEraseRecordedOutcome -Value ([scriptblock]::Create($reader))

    $argument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dockerDesktopData' -IntervalMinutes 60 -Script $script:AutoEraseScripts.dockerDesktopData -ExecutionScope 'user-test' -FirstDueUtc ([DateTime]::UtcNow.AddMinutes(1).ToString('o')) -AllowDefaultExcluded $true
    if ($argument -notmatch '^-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "([^"]+)"$') { throw 'Generated task arguments are invalid.' }
    $payloadPath = [IO.Path]::GetFullPath($Matches[1])
    $payload = Read-WCFixtureText $payloadPath
    if (-not $payload.Contains('function Invoke-WCDockerDesktopCleanup') -or -not $payload.Contains('$script:AutoEraseCoverageIncomplete = $true') -or -not $payload.Contains('$allowDefaultExcluded = $true')) { throw 'Generated Docker payload is missing its action, consent, or coverage.' }
    $tokens = $null; $parseErrors = $null
    $null = [Management.Automation.Language.Parser]::ParseInput($payload, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count) { throw 'Generated Docker payload does not parse.' }
    if ([IO.Path]::GetFileName($payloadPath) -notmatch '\.g([0-9a-f]{32})\.ps1$') { throw 'Generated payload identity is missing.' }
    $generation = $Matches[1]
    $receiptPath = Join-Path (Join-Path $env:LOCALAPPDATA 'WinCommander\auto-erase') "dockerDesktopData.user-test.g$generation.result.json"
    $receipt = @{ attemptedUtc=[DateTimeOffset]::UtcNow.ToString('o'); success=$false; removed=2; deferred=0; failed=0; intervalMinutes=60; coverageIncomplete=$true; payloadGeneration=$generation }
    Save-WCFixtureText $receiptPath ($receipt | ConvertTo-Json -Compress)
    $task = [pscustomobject]@{ TaskPath='\System Maintenance\'; TaskName='fixture'; Actions=@([pscustomobject]@{ Arguments=$argument }) }
    $identity = @{ categoryId='dockerDesktopData'; ownerSid='S-1-5-21-1'; taskName='fixture' }
    $outcome = Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60
    if (-not $outcome -or $outcome.success -or -not $outcome.coverageIncomplete -or $outcome.removed -ne 2 -or $outcome.failed -ne 0) { throw 'Limited-coverage receipt was not retained accurately.' }
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-2' 60) { throw 'A receipt was attributed to another account.' }
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 30) { throw 'A receipt was attributed to the wrong interval.' }
    $receipt.success = $true
    Save-WCFixtureText $receiptPath ($receipt | ConvertTo-Json -Compress)
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A contradictory limited-coverage receipt was accepted.' }
    $receipt.Remove('coverageIncomplete'); $receipt.success = $true
    Save-WCFixtureText $receiptPath ($receipt | ConvertTo-Json -Compress)
    $legacy = Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60
    if (-not $legacy -or -not $legacy.success -or $legacy.ContainsKey('coverageIncomplete')) { throw 'A compatible legacy receipt was rejected.' }
    $validAttempt = $receipt.attemptedUtc
    foreach ($invalidDate in @('2026-10-01', '2026-10-01T00:00:00Z', 'not-a-date')) {
        $receipt.attemptedUtc = $invalidDate
        Save-WCFixtureText $receiptPath ($receipt | ConvertTo-Json -Compress)
        if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A non-round-trip timestamp was accepted.' }
    }
    $receipt.attemptedUtc = $validAttempt
    foreach ($invalidCount in @('2', 1.5, ([int64][int]::MaxValue + 1))) {
        $receipt.removed = $invalidCount
        Save-WCFixtureText $receiptPath ($receipt | ConvertTo-Json -Compress)
        if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'An invalid removal count was accepted.' }
    }
    $receipt.removed = 2
    $receipt.success = $false; $receipt.payloadGeneration = 'a' * 32
    $receipt.coverageIncomplete = $true
    if ($receipt.payloadGeneration -eq $generation) { $receipt.payloadGeneration = 'b' * 32 }
    Save-WCFixtureText $receiptPath ($receipt | ConvertTo-Json -Compress)
    if (Get-AutoEraseRecordedOutcome $task $identity 'S-1-5-21-1' 60) { throw 'A receipt from another payload generation was accepted.' }
    Write-Output 'Passed: generated Docker payload and receipt binding verified in memory.'
} finally {
    $env:ProgramData = $oldProgramData
    $env:LOCALAPPDATA = $oldLocalAppData
}
