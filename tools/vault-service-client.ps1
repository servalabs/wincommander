# Authenticated acceptance client for the local WinCommander SYSTEM service.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('get-policy', 'get-status', 'capabilities', 'list', 'apply', 'mount', 'unmount')]
    [string]$Action,

    [string]$EntryId,
    [string]$PolicyPath
)

$ErrorActionPreference = 'Stop'

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

$feature = switch ($Action) {
    'get-policy' { 'svc.vault.get_policy' }
    'get-status' { 'svc.vault.get_status' }
    'capabilities' { 'svc.vault.capabilities' }
    'list' { 'svc.vault.list_authorized' }
    'apply' { 'svc.vault.apply_policy' }
    'mount' { 'svc.vault.mount' }
    'unmount' { 'svc.vault.unmount' }
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
        $passwordText = [Console]::In.ReadToEnd().TrimEnd([char[]]"`r`n")
        if (-not $passwordText) { throw 'Mount password must be provided on standard input.' }
        $argsValue = [ordered]@{ entry_id = $EntryId; password = $passwordText }
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
        throw "Service rejected request: $($reply.kind): $($reply.message)"
    }
    if ($reply.kind -ne 'response' -or $reply.request_id -ne 1) {
        throw 'Service returned an unexpected response.'
    }
    ConvertTo-Json -InputObject $reply.result -Compress -Depth 30

    Write-Frame $pipe '{"kind":"bye"}'
} finally {
    $passwordText = $null
    if ($pipe) { $pipe.Dispose() }
}
