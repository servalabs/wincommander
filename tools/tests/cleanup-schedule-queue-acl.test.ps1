$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-schedule-acl-' + [guid]::NewGuid())
$previousLocalAppData = $env:LOCALAPPDATA
try {
    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    function Initialize-AutoEraseStorage { @{ scripts = $tempRoot } }
    $env:LOCALAPPDATA = Join-Path $tempRoot 'local'
    $script = @'
$rules = $mutex.GetAccessControl().GetAccessRules($true, $false, [Security.Principal.SecurityIdentifier])
$users = @($rules | Where-Object { $_.IdentityReference.Value -eq 'S-1-5-11' -and $_.AccessControlType -eq 'Allow' })
$required = [Security.AccessControl.MutexRights]::Synchronize -bor [Security.AccessControl.MutexRights]::Modify
if ($users.Count -ne 1 -or $users[0].MutexRights -ne $required) { throw 'Shared queue did not grant only wait and release rights.' }
$script:AutoEraseRemoved++
'@
    $argument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script $script -ExecutionScope 'user-test' -FirstDueUtc ([DateTime]::UtcNow.AddMinutes(-1).ToString('o')) -PayloadRevision 22
    if ($argument -notmatch '-File "([^"]+)"$') { throw 'The synthetic payload path was not returned.' }
    $payloadPath = $Matches[1]
    $mutexName = 'Local\WcCleanupQueueAclTest-' + [guid]::NewGuid().ToString('N')
    $payload = [IO.File]::ReadAllText($payloadPath).Replace('Global\WinCommanderAutoEraseQueue', $mutexName)
    [IO.File]::WriteAllText($payloadPath, $payload, [Text.Encoding]::UTF8)
    & powershell -NoProfile -ExecutionPolicy Bypass -File $payloadPath
    if ($LASTEXITCODE -ne 0) { throw 'The scheduled queue ACL test failed.' }
    if ($payloadPath -notmatch '\.g([0-9a-f]{32})\.ps1$') { throw 'The synthetic generation was not found.' }
    $resultPath = Join-Path $env:LOCALAPPDATA "WinCommander\auto-erase\dnsCache.user-test.g$($Matches[1]).result.json"
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    if (-not $result.success -or $result.removed -ne 1) { throw 'The synthetic task did not complete its queue ACL check.' }
    # A pre-existing queue that denies the required rights must stop the run;
    # falling back to an unlocked or unrelated queue would allow overlap.
    $deniedName = 'Local\WcCleanupQueueDeniedTest-' + [guid]::NewGuid().ToString('N')
    $deniedSecurity = New-Object Security.AccessControl.MutexSecurity
    $authenticated = New-Object Security.Principal.SecurityIdentifier('S-1-5-11')
    $required = [Security.AccessControl.MutexRights]::Synchronize -bor [Security.AccessControl.MutexRights]::Modify
    $deniedSecurity.AddAccessRule((New-Object Security.AccessControl.MutexAccessRule($authenticated, $required, 'Deny')))
    $created = $false
    $deniedMutex = New-Object Threading.Mutex($false, $deniedName, [ref]$created, $deniedSecurity)
    try {
        $deniedArgument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script '$script:AutoEraseRemoved++' -ExecutionScope 'user-test' -FirstDueUtc ([DateTime]::UtcNow.AddMinutes(-1).ToString('o')) -PayloadRevision 22
        if ($deniedArgument -notmatch '-File "([^"]+)"$') { throw 'The denied fixture payload was not returned.' }
        $deniedPayloadPath = $Matches[1]
        $deniedPayload = [IO.File]::ReadAllText($deniedPayloadPath).Replace('Global\WinCommanderAutoEraseQueue', $deniedName)
        [IO.File]::WriteAllText($deniedPayloadPath, $deniedPayload, [Text.Encoding]::UTF8)
        try {
            $ErrorActionPreference = 'Continue'
            & powershell -NoProfile -ExecutionPolicy Bypass -File $deniedPayloadPath 2>$null
            $exitCode = $LASTEXITCODE
        } finally { $ErrorActionPreference = 'Stop' }
        if ($exitCode -eq 0) { throw 'An inaccessible pre-existing queue must stop the run.' }
        if ($deniedPayloadPath -notmatch '\.g([0-9a-f]{32})\.ps1$') { throw 'The denied fixture generation was not found.' }
        if (Test-Path -LiteralPath (Join-Path $env:LOCALAPPDATA "WinCommander\auto-erase\dnsCache.user-test.g$($Matches[1]).result.json")) { throw 'An inaccessible queue must not record cleanup success.' }
    } finally { $deniedMutex.Dispose() }
    Write-Output 'Passed: the shared cleanup queue grants only wait and release rights to authenticated users.'
} finally {
    $env:LOCALAPPDATA = $previousLocalAppData
    if ((Split-Path -Parent $tempRoot) -eq [IO.Path]::GetTempPath().TrimEnd('\') -and (Split-Path -Leaf $tempRoot) -like 'wc-schedule-acl-*') {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
