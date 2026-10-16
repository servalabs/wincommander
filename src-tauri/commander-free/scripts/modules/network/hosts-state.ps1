function Assert-HostsRegularPath {
    param([string]$Path)
    $itemPath = $Path
    while ($itemPath) {
        if (Test-Path -LiteralPath $itemPath) {
            $item = Get-Item -LiteralPath $itemPath -Force -ErrorAction Stop
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Hosts protection cannot use a redirected file or folder.'
            }
        }
        $itemPath = Split-Path -Parent $itemPath
    }
}

function Read-HostsContent {
    Assert-HostsRegularPath $script:HostsFilePath
    if (-not [IO.File]::Exists($script:HostsFilePath)) { return '' }
    if ((Get-Item -LiteralPath $script:HostsFilePath -Force).Length -gt 16MB) { throw 'The hosts file is too large to safely inspect.' }
    return [IO.File]::ReadAllText($script:HostsFilePath)
}

function Get-HostsManagedSection {
    param([string]$Content)
    $start = [regex]::Matches($Content, '(?m)^# WINCOMMANDER_BLOCKLIST_START\r?$')
    $end = [regex]::Matches($Content, '(?m)^# WINCOMMANDER_BLOCKLIST_END\r?$')
    if ($start.Count -eq 0 -and $end.Count -eq 0) { return '' }
    if ($start.Count -ne 1 -or $end.Count -ne 1 -or $end[0].Index -le $start[0].Index) {
        throw 'The WinCom hosts section is incomplete or duplicated. Review the hosts file before changing protection.'
    }
    return $Content.Substring($start[0].Index, $end[0].Index + $end[0].Length - $start[0].Index).TrimEnd("`r")
}

function Set-HostsManagedSection {
    param([string]$Content, [string]$Section)
    $old = Get-HostsManagedSection $Content
    if ($old) { return $Content.Replace($old, $Section) }
    if (-not $Section) { return $Content }
    $separator = if (-not $Content -or $Content.EndsWith("`n")) { '' } else { "`r`n" }
    return $Content + $separator + $Section + "`r`n"
}

function Write-HostsAtomicFile {
    param([string]$Path, [string]$Content)
    Assert-HostsRegularPath $Path
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Content)
            $stream.Write($bytes, 0, $bytes.Length)
            $stream.Flush($true)
        } finally { $stream.Dispose() }
        if ([IO.File]::Exists($Path)) { [IO.File]::Replace($temporary, $Path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $Path) }
    } finally {
        if ([IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
    }
}

function Get-HostsUndoReceipt {
    $path = $script:HostsFilePath + '.wincommander-undo.json'
    Assert-HostsRegularPath $path
    if (-not [IO.File]::Exists($path)) { return $null }
    if ((Get-Item -LiteralPath $path -Force).Length -gt 16MB) { throw 'The previous hosts change is too large to restore safely.' }
    $receipt = [IO.File]::ReadAllText($path) | ConvertFrom-Json -ErrorAction Stop
    if ($receipt.schemaVersion -ne 1 -or $receipt.before -isnot [string] -or $receipt.after -isnot [string]) {
        throw 'The previous hosts change could not be read.'
    }
    foreach ($section in @($receipt.before, $receipt.after)) {
        if ($section -and (Get-HostsManagedSection $section) -cne $section) { throw 'The previous hosts change is invalid.' }
    }
    return $receipt
}

function Get-HostsUndoStatus {
    param([string]$Content)
    try {
        $receipt = Get-HostsUndoReceipt
        $canUndo = $null -ne $receipt -and [string]::Equals((Get-HostsManagedSection $Content), $receipt.after, [StringComparison]::Ordinal)
        return @{ available = $canUndo; changedAt = $receipt.changedAt; label = $receipt.label }
    } catch { return @{ available = $false; reason = $_.Exception.Message } }
}

function Save-HostsChange {
    param([string]$Before, [string]$After, [string]$Label)
    if ([string]::Equals($Before, $After, [StringComparison]::Ordinal)) { return $false }
    if (-not [string]::Equals((Read-HostsContent), $Before, [StringComparison]::Ordinal)) {
        throw 'The hosts file changed during this operation. Refresh and try again.'
    }
    $receipt = @{
        schemaVersion = 1; changedAt = [DateTime]::UtcNow.ToString('o'); label = $Label
        before = Get-HostsManagedSection $Before; after = Get-HostsManagedSection $After
    }
    # Keep recovery beside the admin-owned hosts file, never in a user-writable settings folder.
    $receiptPath = $script:HostsFilePath + '.wincommander-undo.json'
    Assert-HostsRegularPath $receiptPath
    $previous = if ([IO.File]::Exists($receiptPath)) { [IO.File]::ReadAllText($receiptPath) } else { $null }
    Write-HostsAtomicFile $receiptPath ($receipt | ConvertTo-Json -Depth 5)
    try {
        Write-HostsAtomicFile $script:HostsFilePath $After
        if (-not [string]::Equals((Read-HostsContent), $After, [StringComparison]::Ordinal)) {
            throw 'Windows did not retain the requested hosts entries. Refresh protection to inspect the result.'
        }
    } catch {
        if ([string]::Equals((Read-HostsContent), $Before, [StringComparison]::Ordinal)) {
            if ($null -ne $previous) { Write-HostsAtomicFile $receiptPath $previous }
            else { [IO.File]::Delete($receiptPath) }
        }
        throw
    }
    return $true
}

function Undo-HostsBlocklistChange {
    Assert-IsAdmin
    try {
        return Invoke-WithHostsMutex {
            $content = Read-HostsContent
            $receipt = Get-HostsUndoReceipt
            if (-not $receipt) { throw 'No previous WinCom blocking change is available.' }
            if (-not [string]::Equals((Get-HostsManagedSection $content), $receipt.after, [StringComparison]::Ordinal)) {
                throw 'The WinCom section changed after the last operation. Undo was refused to preserve those edits.'
            }
            $restored = Set-HostsManagedSection $content $receipt.before
            Write-HostsAtomicFile $script:HostsFilePath $restored
            if (-not [string]::Equals((Read-HostsContent), $restored, [StringComparison]::Ordinal)) {
                throw 'Windows did not retain the restored entries.'
            }
            [IO.File]::Delete($script:HostsFilePath + '.wincommander-undo.json')
            $dnsWarning = Clear-HostsDnsCache
            return @{ status = 'success'; verified = $true; message = 'Last WinCom blocking change undone.'; dnsWarning = $dnsWarning }
        }
    } catch { return @{ error = $true; message = $_.Exception.Message } }
}
