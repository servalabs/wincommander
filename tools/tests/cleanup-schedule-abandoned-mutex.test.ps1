$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\..\src-tauri\wincmd-shared\scripts\auto-erase.ps1')
$tempRoot = Join-Path ([IO.Path]::GetTempPath()) ('wc-schedule-abandoned-' + [guid]::NewGuid())
$previousLocalAppData = $env:LOCALAPPDATA
try {
    New-Item -ItemType Directory -Path $tempRoot -Force | Out-Null
    function Initialize-AutoEraseStorage { @{ scripts = $tempRoot } }
    $env:LOCALAPPDATA = Join-Path $tempRoot 'local'
    $argument = ConvertTo-AutoEraseTaskArgument -CategoryId 'dnsCache' -IntervalMinutes 60 -Script '$script:AutoEraseRemoved++' -ExecutionScope 'user-test' -FirstDueUtc ([DateTime]::UtcNow.AddMinutes(-1).ToString('o')) -PayloadRevision 21
    if ($argument -notmatch '-File "([^"]+)"$') { throw 'The synthetic payload path was not returned.' }
    $payloadPath = $Matches[1]
    $mutexName = 'Local\WcCleanupAbandonedTest-' + [guid]::NewGuid().ToString('N')
    $payload = [IO.File]::ReadAllText($payloadPath).Replace('Global\WinCommanderAutoEraseQueue', $mutexName)
    [IO.File]::WriteAllText($payloadPath, $payload, [Text.Encoding]::UTF8)
    Add-Type -TypeDefinition @'
using System.Threading;
public static class WcCleanupAbandonedMutexFixture {
    private static Mutex held;
    public static void Abandon(string name) {
        var thread = new Thread(() => { held = new Mutex(false, name); held.WaitOne(); });
        thread.Start();
        thread.Join();
    }
    public static void Dispose() { if (held != null) held.Dispose(); }
}
'@
    [WcCleanupAbandonedMutexFixture]::Abandon($mutexName)
    & powershell -NoProfile -ExecutionPolicy Bypass -File $payloadPath
    if ($LASTEXITCODE -ne 0) { throw 'A previously abandoned mutex prevented the scheduled cleanup from running.' }
    if ($payloadPath -notmatch '\.g([0-9a-f]{32})\.ps1$') { throw 'The synthetic generation was not found.' }
    $resultPath = Join-Path $env:LOCALAPPDATA "WinCommander\auto-erase\dnsCache.user-test.g$($Matches[1]).result.json"
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    if (-not $result.success -or $result.removed -ne 1) { throw 'The scheduled cleanup did not record its successful retry.' }
    Write-Output 'Passed: a scheduled cleanup recovers an abandoned queue mutex.'
} finally {
    if ('WcCleanupAbandonedMutexFixture' -as [type]) { [WcCleanupAbandonedMutexFixture]::Dispose() }
    $env:LOCALAPPDATA = $previousLocalAppData
    if ((Split-Path -Parent $tempRoot) -eq [IO.Path]::GetTempPath().TrimEnd('\') -and (Split-Path -Leaf $tempRoot) -like 'wc-schedule-abandoned-*') {
        Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
