# Authenticated acceptance client for the local WinCommander SYSTEM service.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('get-policy', 'get-status', 'capabilities', 'list', 'diagnostics', 'engine-log', 'broker-log', 'container-probe', 'apply', 'mount', 'unmount', 'unknown-verb')]
    [string]$Action,

    [string]$EntryId,
    [string]$PolicyPath,

    [ValidateSet('outer', 'hidden')]
    [string]$VolumeRole = 'outer',

    # Read-only acceptance probes can relaunch themselves through Windows'
    # explicit RunAs path.  Mutation and password-bearing actions are never
    # forwarded this way, so no secret is written to a temporary file.
    [switch]$Elevated,

    [Parameter(ValueFromPipeline = $true)]
    [string]$InputSecret
)

$ErrorActionPreference = 'Stop'

function Test-ElevatedToken {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if ($Elevated -and -not (Test-ElevatedToken)) {
    $readOnlyActions = @('get-policy', 'get-status', 'capabilities', 'list', 'diagnostics', 'engine-log', 'broker-log', 'container-probe')
    if ($Action -notin $readOnlyActions) {
        throw '-Elevated is restricted to read-only service probes.'
    }
    $resultPath = Join-Path $env:TEMP ("wincommander-vault-probe-{0}.json" -f [guid]::NewGuid().ToString('N'))
    $escapedScript = $PSCommandPath.Replace("'", "''")
    $escapedResult = $resultPath.Replace("'", "''")
    $childCommand = "try { & '$escapedScript' -Action '$Action' | Set-Content -LiteralPath '$escapedResult' -NoNewline; exit 0 } catch { exit 1 }"
    $encodedCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
    try {
        $process = Start-Process -FilePath powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList "-NoProfile -ExecutionPolicy Bypass -EncodedCommand $encodedCommand"
        if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $resultPath)) {
            throw 'The elevated read-only Vault probe did not return a result.'
        }
        Get-Content -LiteralPath $resultPath -Raw
    } finally {
        Remove-Item -LiteralPath $resultPath -Force -ErrorAction SilentlyContinue
    }
    return
}

function Write-Frame([System.IO.Stream]$Stream, [string]$Json) {
    $body = [Text.Encoding]::UTF8.GetBytes($Json)
    $length = [BitConverter]::GetBytes([uint32]$body.Length)
    $Stream.Write($length, 0, $length.Length)
    $Stream.Write($body, 0, $body.Length)
    $Stream.Flush()
}

function Read-Exact([System.IO.Stream]$Stream, [int]$Count) {
    $buffer = New-Object byte[] $Count
    $offset = 0
    while ($offset -lt $Count) {
        $read = $Stream.Read($buffer, $offset, $Count - $offset)
        if ($read -le 0) { throw 'Unexpected end of service response.' }
        $offset += $read
    }
    return $buffer
}

function Read-Frame([System.IO.Stream]$Stream) {
    $lengthBytes = Read-Exact $Stream 4
    $length = [BitConverter]::ToUInt32($lengthBytes, 0)
    if ($length -gt 8388608) { throw 'Service response exceeded the protocol limit.' }
    $body = Read-Exact $Stream ([int]$length)
    return [Text.Encoding]::UTF8.GetString($body)
}

if ($Action -eq 'engine-log') {
    # The Pro log is SYSTEM/Admin-only.  Return only the two bounded native
    # engine breadcrumbs needed to classify a fresh mount failure; never
    # expose its paths, credentials, or arbitrary log lines.
    $logPath = Join-Path $env:ProgramData 'WinCommander\pro-logs\pro.log'
    $events = @()
    $mountFailureCount = 0
    $unclassifiedMountFailureCount = 0
    $lastUnclassifiedStage = $null
    if (Test-Path -LiteralPath $logPath -PathType Leaf) {
        $events = Get-Content -LiteralPath $logPath -Tail 500 | ForEach-Object {
            if ($_ -match 'native mount failed with exit code ([0-9]+|terminated); internal status ([0-9-]+|unavailable)') {
                [ordered]@{ native_exit = $Matches[1]; internal_status = $Matches[2] }
            } elseif ($_ -match 'vault broker mount failed .*: (native engine rejected mount|native engine could not unlock the selected volume|native engine rejected: drive letter unavailable|mounted-root ACL attestation failed|mount presentation verification failed)') {
                [ordered]@{ broker_stage = $Matches[1] }
            }
        } | Where-Object { $_ }
    }
    ConvertTo-Json -InputObject @($events) -Compress
    return
}

if ($Action -eq 'broker-log') {
    # The Pro log is SYSTEM/Admin-only. Return only the fixed diagnostic
    # vocabulary selected by the broker; never expose raw request data,
    # paths, credentials, or arbitrary log lines.
    $logPath = Join-Path $env:ProgramData 'WinCommander\pro-logs\pro.log'
    $allowed = @(
        'mount stdout attestation rejected: (?:scope|mount mode|root ACL)(?: \+ (?:mount mode|root ACL))?',
        'per-user presentation rejected: (?:query|foreign mapping|drive letter)',
        'mount plan rejected: (?:volume kind|volume role|mount mode|presentation|root ACL)',
        'native engine rejected: drive letter unavailable',
        'native engine could not unlock the selected volume',
        'engine authentication timed out',
        'mounted-root ACL attestation failed',
        'mount presentation verification failed',
        'native engine rejected the mount',
        'mount rejected'
    ) -join '|'
    $events = @()
    $logPresent = Test-Path -LiteralPath $logPath -PathType Leaf
    $lastWriteUtc = $null
    if ($logPresent) {
        $lastWriteUtc = (Get-Item -LiteralPath $logPath).LastWriteTimeUtc.ToString('o')
    }
    if ($logPresent) {
        $events = Get-Content -LiteralPath $logPath -Tail 500 | ForEach-Object {
            if ($_ -match 'vault broker mount failed') {
                $mountFailureCount++
                if ($_ -match 'vault broker mount failed \(operation=[0-9]+\): ([A-Za-z0-9 ,+:-]+)$') {
                    $lastUnclassifiedStage = $Matches[1]
                }
            }
            if ($_ -match "vault broker mount failed .*: ($allowed)$") {
                [ordered]@{ broker_stage = $Matches[1] }
            }
        } | Where-Object { $_ }
        $unclassifiedMountFailureCount = [Math]::Max(0, $mountFailureCount - @($events).Count)
    }
    [ordered]@{
        log_present = $logPresent
        last_write_utc = $lastWriteUtc
        matching_stage_count = @($events).Count
        unclassified_mount_failure_count = $unclassifiedMountFailureCount
        last_unclassified_stage = $lastUnclassifiedStage
        stages = @($events)
    } | ConvertTo-Json -Compress
    return
}

$feature = switch ($Action) {
    'get-policy' { 'svc.vault.get_policy' }
    'get-status' { 'svc.vault.get_status' }
    'capabilities' { 'svc.vault.capabilities' }
    'list' { 'svc.vault.list_authorized' }
    'diagnostics' { 'svc.diagnostics.query' }
    'container-probe' { 'svc.vault.get_policy' }
    'apply' { 'svc.vault.apply_policy' }
    'mount' { 'svc.vault.mount' }
    'unmount' { 'svc.vault.unmount' }
    # Fixed acceptance probe only; this does not expose arbitrary service verbs.
    'unknown-verb' { 'svc.vault.__unknown_acceptance_probe' }
}

$passwordText = $null
$pipe = $null
try {
    $argsValue = [ordered]@{}
    if ($Action -eq 'apply') {
        if (-not $PolicyPath) { throw '-PolicyPath is required for apply.' }
        $argsValue = Get-Content -LiteralPath $PolicyPath -Raw | ConvertFrom-Json
    } elseif ($Action -eq 'mount') {
        if (-not $EntryId) { throw '-EntryId is required for mount.' }
        $secretInput = if (-not [string]::IsNullOrEmpty($InputSecret)) { $InputSecret } else { [Console]::In.ReadToEnd() }
        $secretLines = @($secretInput -split "`r?`n" | Where-Object { $_.Length -gt 0 })
        $passwordText = $secretLines | Select-Object -First 1
        if (-not $passwordText) { throw 'Mount password must be provided on standard input.' }
        $argsValue = [ordered]@{ entry_id = $EntryId; password = $passwordText; volume_role = $VolumeRole }
        if ($secretLines.Count -gt 1) {
            $argsValue.hidden_protection_password = $secretLines[1]
        }
    } elseif ($Action -eq 'unmount') {
        if (-not $EntryId) { throw '-EntryId is required for unmount.' }
        $argsValue = [ordered]@{ entry_id = $EntryId }
    }

    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(
        '.', 'wincmd-svc', [System.IO.Pipes.PipeDirection]::InOut,
        [System.IO.Pipes.PipeOptions]::None
    )
    $pipe.Connect(5000)
    $pipe.ReadMode = [System.IO.Pipes.PipeTransmissionMode]::Byte

    $token = [Guid]::NewGuid().ToString()
    $hello = [ordered]@{
        kind = 'hello'
        protocol_version = 'wincmd-svc-v1'
        session_token = $token
        free_version = 'live-acceptance-client'
    } | ConvertTo-Json -Compress
    Write-Frame $pipe $hello
    $ack = Read-Frame $pipe | ConvertFrom-Json
    if ($ack.kind -ne 'hello') { throw 'Service handshake failed.' }

    $inner = [ordered]@{
        kind = 'request'
        request_id = 1
        feature_id = $feature
        args = $argsValue
    } | ConvertTo-Json -Compress -Depth 30
    $hmac = New-Object Security.Cryptography.HMACSHA256
    $hmac.Key = [Text.Encoding]::UTF8.GetBytes($token)
    $tag = -join ($hmac.ComputeHash([Text.Encoding]::UTF8.GetBytes($inner)) | ForEach-Object { $_.ToString('x2') })
    $signed = [ordered]@{ kind = 'signed'; tag = $tag; inner = $inner } | ConvertTo-Json -Compress -Depth 30
    Write-Frame $pipe $signed

    $reply = Read-Frame $pipe | ConvertFrom-Json
    if ($reply.kind -eq 'error') {
        if ($Action -eq 'unknown-verb') {
            [ordered]@{
                request_id = $reply.request_id
                error_kind = $reply.error_kind
                message = $reply.message
            } | ConvertTo-Json -Compress
            Write-Frame $pipe '{"kind":"bye"}'
            return
        }
        throw "Service rejected request: $($reply.error_kind): $($reply.message)"
    }
    if ($reply.kind -ne 'response' -or $reply.request_id -ne 1) {
        throw 'Service returned an unexpected response.'
    }
    if ($Action -eq 'container-probe') {
        # The service chooses the registered path; this cannot be used as a
        # general-purpose filesystem reader.  Report only existence, size,
        # an elevated read probe, and whether SYSTEM has Full Control — enough
        # to diagnose an engine access-denied result without leaking a path.
        $entries = @($reply.result.entries)
        $results = foreach ($entry in $entries) {
            $path = [string]$entry.container_path
            $exists = Test-Path -LiteralPath $path -PathType Leaf
            $readable = $false
            $systemFullControl = $false
            $size = $null
            if ($exists) {
                try {
                    $file = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
                    try { $size = $file.Length; $readable = $true } finally { $file.Dispose() }
                    $acl = Get-Acl -LiteralPath $path
                    $systemFullControl = @($acl.Access | Where-Object {
                        $_.IdentityReference.Value -eq 'NT AUTHORITY\SYSTEM' -and
                        $_.AccessControlType -eq [Security.AccessControl.AccessControlType]::Allow -and
                        ($_.FileSystemRights -band [Security.AccessControl.FileSystemRights]::FullControl) -eq [Security.AccessControl.FileSystemRights]::FullControl
                    }).Count -gt 0
                } catch { $readable = $false }
            }
            [ordered]@{ entry_id = $entry.id; exists = $exists; readable_elevated = $readable; system_full_control = $systemFullControl; size_bytes = $size }
        }
        ConvertTo-Json -InputObject @($results) -Compress -Depth 10
    } else {
        ConvertTo-Json -InputObject $reply.result -Compress -Depth 30
    }

    Write-Frame $pipe '{"kind":"bye"}'
} finally {
    $passwordText = $null
    $secretInput = $null
    $secretLines = $null
    $InputSecret = $null
    if ($pipe) { $pipe.Dispose() }
}
