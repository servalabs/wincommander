# ============================================================================
# PRIVACY - CLEANUP & HYGIENE MODULE
# Automated erases, history clearing, and cache management
# ============================================================================
#
# NOTE: the per-card auto-erase scheduler (Set-AutoEraseSchedule,
# Remove-AutoEraseSchedule, Get-AutoEraseSchedules,
# Get-AutoEraseSupportedCategories, Invoke-AutoEraseMigration) is NOT
# defined here. It lives in the shared module at
# commander-shared/scripts/auto-erase.ps1 — single source of truth for
# both commander-free and commander-pro. The Free backend prepends that
# module's contents to this one when loading "privacy/cleanup", so
# calling Set-AutoEraseSchedule from this module's runtime context works
# transparently.

# --- Manual Cleaning Actions ---

function Clear-Clipboard {
    try {
        Set-Clipboard -Value $null -ErrorAction SilentlyContinue
        Restart-Service cbdhsvc_* -Force -ErrorAction SilentlyContinue
        @{ status = 'cleared' }
    }
    catch {
        @{ error = $true; message = "Failed to clear clipboard: $($_.Exception.Message)" }
    }
}

function Convert-CleanupClipboardHistoryResult {
    param($Result, [scriptblock]$ReadPreview)
    if ($null -eq $Result) { return @{ status = 'unavailable'; items = @() } }
    $status = [string]$Result.Status
    if ($status -eq 'ClipboardHistoryDisabled') { return @{ status = 'disabled'; items = @() } }
    if ($status -ne 'Success' -or $null -eq $Result.Items) { return @{ status = 'unavailable'; items = @() } }
    $items = @($Result.Items | ForEach-Object {
        $item = @{ type = 'item'; preview = '[Clipboard item]'; charCount = 0 }
        if ($ReadPreview) {
            try {
                $detail = & $ReadPreview $_
                if ($detail) { $item = $detail }
            } catch { $item = @{ type = 'item'; preview = '[Content unavailable]'; charCount = 0 } }
        }
        $item
    })
    return @{ status = 'complete'; items = $items }
}

function Convert-CleanupClipboardItemDetails {
    param($Item, [hashtable]$FormatNames, [scriptblock]$ReadTextFormat)
    $formats = @($Item.Content.AvailableFormats | ForEach-Object { [string]$_ })
    $details = @{ type = 'item'; formats = $formats; id = [string]$Item.Id; timestamp = [string]$Item.Timestamp; charCount = 0 }
    $unavailable = [Collections.Generic.List[string]]::new()
    foreach ($name in @('text', 'html', 'rtf', 'webLink', 'applicationLink', 'storageItems')) {
        $format = [string]$FormatNames[$name]
        if (-not $format -or $format -notin $formats) { continue }
        try {
            $value = & $ReadTextFormat $Item $name
            if ($null -eq $value) { throw 'Clipboard content unavailable.' }
            if ($name -eq 'storageItems') { $details[$name] = @($value) }
            else { $details[$name] = [string]$value }
            if ($name -eq 'text') { $details.type = 'text'; $details.preview = [string]$value; $details.charCount = ([string]$value).Length }
        } catch { $unavailable.Add($format) }
    }
    if ($unavailable.Count) { $details.unavailableFormats = @($unavailable.ToArray()) }
    # Do not suggest a separate viewer is needed when the returned HTML/RTF
    # is already present or the bitmap can be loaded by the local image view.
    if (-not $details.ContainsKey('preview')) {
        if ($details.ContainsKey('html') -or $details.ContainsKey('rtf')) { $details.type = 'rich text' }
        elseif ($formats -contains 'Bitmap') { $details.type = 'image' }
        elseif ($details.ContainsKey('webLink') -or $details.ContainsKey('applicationLink')) { $details.type = 'link' }
        elseif ($details.ContainsKey('storageItems')) { $details.type = 'files' }
        else { $details.contentStatus = 'Content unavailable for the available formats.' }
    }
    return $details
}

function Get-CleanupClipboardItemDetails {
    param($Item, $AsTask)
            $null = [Windows.ApplicationModel.DataTransfer.StandardDataFormats, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
            $null = [Windows.Storage.IStorageItem, Windows.Storage, ContentType=WindowsRuntime]
            $null = [Windows.Storage.StorageItemTypes, Windows.Storage, ContentType=WindowsRuntime]
                $formats = @{ text = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::Text; html = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::Html; rtf = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::Rtf; webLink = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::WebLink; applicationLink = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::ApplicationLink; storageItems = [Windows.ApplicationModel.DataTransfer.StandardDataFormats]::StorageItems }
                $readText = {
                    param($entry, $format)
                    $operation = switch ($format) { 'text' { $entry.Content.GetTextAsync() } 'html' { $entry.Content.GetHtmlFormatAsync() } 'rtf' { $entry.Content.GetRtfAsync() } 'webLink' { $entry.Content.GetWebLinkAsync() } 'applicationLink' { $entry.Content.GetApplicationLinkAsync() } 'storageItems' { $entry.Content.GetStorageItemsAsync() } }
                    $resultType = if ($format -in @('webLink', 'applicationLink')) { [Uri] } elseif ($format -eq 'storageItems') { [System.Collections.Generic.IReadOnlyList[Windows.Storage.IStorageItem]] } else { [string] }
                    $textTask = $AsTask.MakeGenericMethod($resultType).Invoke($null, @($operation))
                    if (-not $textTask.Wait(2000)) { throw 'Clipboard content timed out.' }
                    if ($format -eq 'storageItems') {
                        $entries = @($textTask.Result | ForEach-Object {
                            @{ name = [string]$_.Name; path = [string]$_.Path; type = if ($_.IsOfType([Windows.Storage.StorageItemTypes]::File)) { 'File' } elseif ($_.IsOfType([Windows.Storage.StorageItemTypes]::Folder)) { 'Folder' } else { 'Unknown' } }
                        })
                        return ,$entries
                    }
                    return [string]$textTask.Result
                }
                return (Convert-CleanupClipboardItemDetails -Item $Item -FormatNames $formats -ReadTextFormat $readText)
}

function Get-CleanupClipboardHistory {
    param([bool]$IncludePreviews = $false)
    try {
        Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
        $null = [Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $null = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
            $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.IsGenericType
        } | Select-Object -First 1
        if (-not $asTask) { return @{ status = 'unavailable'; items = @() } }
        $asyncOp = [Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()
        $resultTy = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult]
        $task = $asTask.MakeGenericMethod($resultTy).Invoke($null, @($asyncOp))
        if (-not $task.Wait(2000)) { return @{ status = 'unavailable'; items = @() } }
        $readPreview = $null
        if ($IncludePreviews) {
            # The list is metadata only. Content is fetched per item in bounded
            # chunks, preventing large history entries from exceeding IPC limits.
            $readPreview = {
                param($item)
                $metadata = @{ type = 'item'; id = [string]$item.Id; timestamp = [string]$item.Timestamp; detailAvailable = $true }
                try { $metadata.formats = @($item.Content.AvailableFormats | ForEach-Object { [string]$_ }) }
                catch { $metadata.formats = @(); $metadata.metadataStatus = 'Clipboard formats could not be read.' }
                $metadata
            }
        }
        return (Convert-CleanupClipboardHistoryResult -Result $task.Result -ReadPreview $readPreview)
    } catch {
        return @{ status = 'unavailable'; items = @() }
    }
}

# Encode one item's complete detail as UTF-8 JSON and return only a bounded
# slice. The hash binds continuation requests to the same complete value.
function Convert-CleanupClipboardCanonicalValue {
    param($Value)
    if ($null -eq $Value) { return $null }
    if ($Value -is [Collections.IDictionary]) {
        $ordered = [ordered]@{}
        foreach ($key in @($Value.Keys | Sort-Object -CaseSensitive)) { $ordered[[string]$key] = Convert-CleanupClipboardCanonicalValue $Value[$key] }
        return $ordered
    }
    if ($Value -is [array]) {
        $items = @($Value | ForEach-Object { Convert-CleanupClipboardCanonicalValue $_ })
        return ,$items
    }
    return $Value
}

function Convert-CleanupClipboardDetailChunk {
    param($Detail, [long]$Offset = 0, [string]$ExpectedHash = '')
    $chunkSize = 262144
    if ($null -eq $Detail -or $Offset -lt 0 -or ($Offset % $chunkSize) -ne 0 -or ($Offset -gt 0 -and $ExpectedHash -notmatch '^[0-9a-f]{64}$') -or ($ExpectedHash -and $ExpectedHash -notmatch '^[0-9a-f]{64}$')) { return @{ status = 'unavailable' } }
    try {
        $canonical = Convert-CleanupClipboardCanonicalValue $Detail
        $json = ConvertTo-Json -InputObject $canonical -Depth 32 -Compress -ErrorAction Stop
        $bytes = [Text.Encoding]::UTF8.GetBytes($json)
        if ($bytes.LongLength -gt 67108864) { return @{ status = 'too-large' } }
        if ($Offset -ge $bytes.LongLength) { return @{ status = 'unavailable' } }
        $sha = [Security.Cryptography.SHA256]::Create()
        try { $hash = [BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-', '').ToLowerInvariant() } finally { $sha.Dispose() }
        if ($ExpectedHash -and $hash -ne $ExpectedHash) { return @{ status = 'changed' } }
        $length = [int][Math]::Min($chunkSize, $bytes.LongLength - $Offset)
        $slice = New-Object byte[] $length
        [Array]::Copy($bytes, $Offset, $slice, 0, $length)
        $next = $Offset + $length
        return @{ status = 'complete'; detailBase64 = [Convert]::ToBase64String($slice); offset = $Offset; totalBytes = $bytes.LongLength; nextOffset = if ($next -lt $bytes.LongLength) { $next } else { $null }; snapshotHash = $hash }
    } catch { return @{ status = 'unavailable' } }
}

function Get-CleanupClipboardDetail {
    param([string]$ItemId, [long]$Offset = 0, [string]$ExpectedHash = '')
    if ([string]::IsNullOrWhiteSpace($ItemId) -or $ItemId.Length -gt 256) { return @{ status = 'unavailable' } }
    try {
        Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
        $null = [Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $resultType = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
            $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.IsGenericType
        } | Select-Object -First 1
        if (-not $asTask) { return @{ status = 'unavailable' } }
        $task = $asTask.MakeGenericMethod($resultType).Invoke($null, @([Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()))
        if (-not $task.Wait(2000) -or [string]$task.Result.Status -ne 'Success') { return @{ status = 'unavailable' } }
        $matches = @($task.Result.Items | Where-Object { [string]$_.Id -ceq $ItemId })
        if ($matches.Count -ne 1) { return @{ status = 'unavailable' } }
        $detail = Get-CleanupClipboardItemDetails -Item $matches[0] -AsTask $asTask
        return (Convert-CleanupClipboardDetailChunk -Detail $detail -Offset $Offset -ExpectedHash $ExpectedHash)
    } catch { return @{ status = 'unavailable' } }
}

# Read one selected image at a time. A bounded response avoids placing every
# clipboard image in one backend reply; no temporary file is written.
function Convert-CleanupClipboardImageStream {
    param([System.IO.Stream]$Stream, [long]$Offset = 0, [string]$ExpectedHash = '', [long]$MaxBytes = 67108864)
    $chunkSize = 262144
    if (-not $Stream -or $MaxBytes -lt 1 -or $MaxBytes -gt 67108864 -or $Offset -lt 0 -or ($Offset % $chunkSize) -ne 0 -or ($Offset -gt 0 -and $ExpectedHash -notmatch '^[0-9a-f]{64}$') -or ($ExpectedHash -and $ExpectedHash -notmatch '^[0-9a-f]{64}$')) { return @{ status = 'unavailable' } }
    $buffer = New-Object byte[] 65536
    $chunk = [IO.MemoryStream]::new(); $header = [IO.MemoryStream]::new()
    $hasher = [Security.Cryptography.IncrementalHash]::CreateHash([Security.Cryptography.HashAlgorithmName]::SHA256)
    $total = [long]0
    try {
        while ($true) {
            $read = $Stream.Read($buffer, 0, $buffer.Length)
            if ($read -le 0) { break }
            if ($total + $read -gt $MaxBytes) { return @{ status = 'too-large' } }
            $hasher.AppendData($buffer, 0, $read)
            if ($header.Length -lt 12) { $header.Write($buffer, 0, [Math]::Min($read, 12 - [int]$header.Length)) }
            $start = [Math]::Max($Offset, $total); $end = [Math]::Min($Offset + $chunkSize, $total + $read)
            if ($end -gt $start) { $chunk.Write($buffer, [int]($start - $total), [int]($end - $start)) }
            $total += $read
        }
        if ($Offset -ge $total) { return @{ status = 'unavailable' } }
        $hash = [BitConverter]::ToString($hasher.GetHashAndReset()).Replace('-', '').ToLowerInvariant()
        if ($ExpectedHash -and $hash -ne $ExpectedHash) { return @{ status = 'changed' } }
        $bytes = $header.ToArray()
        $mime = $null
        if ($bytes.Length -ge 8 -and [BitConverter]::ToString($bytes, 0, 8) -eq '89-50-4E-47-0D-0A-1A-0A') { $mime = 'image/png' }
        elseif ($bytes.Length -ge 3 -and $bytes[0] -eq 0xff -and $bytes[1] -eq 0xd8 -and $bytes[2] -eq 0xff) { $mime = 'image/jpeg' }
        elseif ($bytes.Length -ge 2 -and $bytes[0] -eq 0x42 -and $bytes[1] -eq 0x4d) { $mime = 'image/bmp' }
        elseif ($bytes.Length -ge 6 -and [Text.Encoding]::ASCII.GetString($bytes, 0, 6) -in @('GIF87a','GIF89a')) { $mime = 'image/gif' }
        elseif ($bytes.Length -ge 12 -and [Text.Encoding]::ASCII.GetString($bytes, 0, 4) -eq 'RIFF' -and [Text.Encoding]::ASCII.GetString($bytes, 8, 4) -eq 'WEBP') { $mime = 'image/webp' }
        if (-not $mime) { return @{ status = 'unsupported' } }
        $next = $Offset + $chunk.Length
        return @{ status = 'complete'; imageMime = $mime; imageBase64 = [Convert]::ToBase64String($chunk.ToArray()); offset = $Offset; totalBytes = $total; nextOffset = if ($next -lt $total) { $next } else { $null }; snapshotHash = $hash }
    } catch { return @{ status = 'unavailable' } }
    finally { $chunk.Dispose(); $header.Dispose(); $hasher.Dispose() }
}

function Get-CleanupClipboardImage {
    param([string]$ItemId, [long]$Offset = 0, [string]$ExpectedHash = '')
    if ([string]::IsNullOrWhiteSpace($ItemId) -or $ItemId.Length -gt 256) { return @{ status = 'unavailable' } }
    $stream = $null; $randomStream = $null
    try {
        Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
        $null = [Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $null = [Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $null = [Windows.ApplicationModel.DataTransfer.StandardDataFormats, Windows.ApplicationModel.DataTransfer, ContentType=WindowsRuntime]
        $referenceType = [Windows.Storage.Streams.RandomAccessStreamReference, Windows.Storage.Streams, ContentType=WindowsRuntime]
        $streamType = [Windows.Storage.Streams.IRandomAccessStreamWithContentType, Windows.Storage.Streams, ContentType=WindowsRuntime]
        $asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
            $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.IsGenericType
        } | Select-Object -First 1
        if (-not $asTask) { return @{ status = 'unavailable' } }
        $historyTask = $asTask.MakeGenericMethod([Windows.ApplicationModel.DataTransfer.ClipboardHistoryItemsResult]).Invoke($null, @([Windows.ApplicationModel.DataTransfer.Clipboard]::GetHistoryItemsAsync()))
        if (-not $historyTask.Wait(2000) -or [string]$historyTask.Result.Status -ne 'Success') { return @{ status = 'unavailable' } }
        $matches = @($historyTask.Result.Items | Where-Object { [string]$_.Id -ceq $ItemId })
        if ($matches.Count -ne 1 -or -not $matches[0].Content.Contains([Windows.ApplicationModel.DataTransfer.StandardDataFormats]::Bitmap)) { return @{ status = 'unavailable' } }
        $bitmapTask = $asTask.MakeGenericMethod($referenceType).Invoke($null, @($matches[0].Content.GetBitmapAsync()))
        if (-not $bitmapTask.Wait(2000) -or -not $bitmapTask.Result) { return @{ status = 'unavailable' } }
        $openTask = $asTask.MakeGenericMethod($streamType).Invoke($null, @($bitmapTask.Result.OpenReadAsync()))
        if (-not $openTask.Wait(2000) -or -not $openTask.Result) { return @{ status = 'unavailable' } }
        $randomStream = $openTask.Result
        if ($randomStream.Size -gt 67108864) { return @{ status = 'too-large' } }
        $stream = [System.IO.WindowsRuntimeStreamExtensions]::AsStreamForRead($randomStream)
        return (Convert-CleanupClipboardImageStream -Stream $stream -Offset $Offset -ExpectedHash $ExpectedHash)
    } catch { return @{ status = 'unavailable' } }
    finally {
        if ($stream) { try { $stream.Dispose() } catch { } }
        if ($randomStream) { try { $randomStream.Dispose() } catch { } }
    }
}

function Get-ClipboardHistoryStatus {
    param([bool]$IncludePreviews = $false, [string]$ImageItemId = '', [long]$ImageOffset = 0, [string]$ImageSnapshot = '', [string]$DetailItemId = '', [long]$DetailOffset = 0, [string]$DetailSnapshot = '')
    if ($PSBoundParameters.ContainsKey('ImageItemId')) { return (Get-CleanupClipboardImage -ItemId $ImageItemId -Offset $ImageOffset -ExpectedHash $ImageSnapshot) }
    if ($PSBoundParameters.ContainsKey('DetailItemId')) { return (Get-CleanupClipboardDetail -ItemId $DetailItemId -Offset $DetailOffset -ExpectedHash $DetailSnapshot) }
    try {
        $cb    = Get-ItemProperty -Path "HKCU:\Software\Microsoft\Clipboard" -EA SilentlyContinue
        # Also detect if an external policy (GPO/MDM we didn't set) is blocking things.
        $polH  = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "AllowClipboardHistory"    -EA SilentlyContinue
        $polC  = Get-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System" -Name "AllowCrossDeviceClipboard" -EA SilentlyContinue

        $histDisabled  = ($cb.EnableClipboardHistory -eq 0 -or $polH.AllowClipboardHistory    -eq 0)
        $cloudDisabled = ($cb.EnableCloudClipboard   -eq 0 -or $polC.AllowCrossDeviceClipboard -eq 0)

        # A disabled setting alone does not establish whether stored history is
        # empty. Only a successful inventory can establish the record count.
        $history = Get-CleanupClipboardHistory -IncludePreviews $IncludePreviews
        if ($history.status -eq 'disabled') { $histDisabled = $true }

        @{
            clipboardHistoryDisabled = $histDisabled
            cloudClipboardDisabled   = $cloudDisabled
            historyItems             = @($history.items)
            historyReadStatus        = $history.status
            partial                  = ($history.status -ne 'complete')
            message                  = if ($history.status -eq 'disabled') { 'Clipboard history is disabled and stored history could not be read.' } elseif ($history.status -ne 'complete') { 'Clipboard history could not be read.' } else { '' }
        }
    }
    catch {
        @{ error = $true; message = "Failed to read clipboard history status: $($_.Exception.Message)" }
    }
}

function Restart-ClipboardService {
    # cbdhsvc caches EnableClipboardHistory at startup; restart so it re-reads the registry.
    Get-Service -Name "cbdhsvc*" -ErrorAction SilentlyContinue | Restart-Service -Force -ErrorAction SilentlyContinue
}

function Disable-ClipboardHistory {
    Assert-IsAdmin
    try {
        # User-preference keys turn the feature off at the OS level.
        foreach ($p in @("HKCU:\Software\Microsoft\Clipboard", "HKLM:\Software\Microsoft\Clipboard")) {
            if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
            Set-ItemProperty -Path $p -Name "EnableClipboardHistory" -Value 0 -Type DWord -Force
        }
        # Policy keys (HKCU + HKLM, value 0) show "Managed by your organization"
        # in Windows Settings and lock the toggle so the user can't re-enable it.
        # Enable-ClipboardHistory removes these keys so the banner disappears on re-enable.
        foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
            Set-ItemProperty -Path $p -Name "AllowClipboardHistory" -Value 0 -Type DWord -Force
        }
        Restart-ClipboardService
        @{ status = 'disabled' }
    }
    catch {
        @{ error = $true; message = "Failed to disable clipboard history: $($_.Exception.Message)" }
    }
}

function Enable-ClipboardHistory {
    Assert-IsAdmin
    try {
        foreach ($p in @("HKCU:\Software\Microsoft\Clipboard", "HKLM:\Software\Microsoft\Clipboard")) {
            if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
            Set-ItemProperty -Path $p -Name "EnableClipboardHistory" -Value 1 -Type DWord -Force
        }
        # Remove policy keys — any policy key present (even value=1) shows "Managed by organization".
        foreach ($p in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            Remove-ItemProperty -Path $p -Name "AllowClipboardHistory" -ErrorAction SilentlyContinue
        }
        Restart-ClipboardService
        @{ status = 'enabled' }
    }
    catch {
        @{ error = $true; message = "Failed to enable clipboard history: $($_.Exception.Message)" }
    }
}

function Disable-CloudClipboardSync {
    Assert-IsAdmin
    try {
        $p = "HKCU:\Software\Microsoft\Clipboard"
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "EnableCloudClipboard" -Value 0 -Type DWord -Force
        # Policy keys lock the toggle and show "Managed by organization".
        foreach ($pp in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            if (!(Test-Path $pp)) { New-Item -Path $pp -Force | Out-Null }
            Set-ItemProperty -Path $pp -Name "AllowCrossDeviceClipboard" -Value 0 -Type DWord -Force
        }
        @{ status = 'disabled' }
    }
    catch {
        @{ error = $true; message = "Failed to disable cloud clipboard sync: $($_.Exception.Message)" }
    }
}

function Enable-CloudClipboardSync {
    Assert-IsAdmin
    try {
        $p = "HKCU:\Software\Microsoft\Clipboard"
        if (!(Test-Path $p)) { New-Item -Path $p -Force | Out-Null }
        Set-ItemProperty -Path $p -Name "EnableCloudClipboard" -Value 1 -Type DWord -Force
        foreach ($pp in @("HKCU:\SOFTWARE\Policies\Microsoft\Windows\System", "HKLM:\SOFTWARE\Policies\Microsoft\Windows\System")) {
            Remove-ItemProperty -Path $pp -Name "AllowCrossDeviceClipboard" -ErrorAction SilentlyContinue
        }
        @{ status = 'enabled' }
    }
    catch {
        @{ error = $true; message = "Failed to enable cloud clipboard sync: $($_.Exception.Message)" }
    }
}

function Get-PSHistory {
    try {
        $appData = [Environment]::GetFolderPath('ApplicationData')
        $historyPath = Join-Path $appData 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
        $historyFile = $null
        try { $historyFile = Get-Item -LiteralPath $historyPath -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        $commands = @()
        if ($null -ne $historyFile) {
            $commands = @(Get-Content -LiteralPath $historyPath -ErrorAction Stop | Where-Object { $_ -is [string] -and $_.Trim() -ne '' })
        }
        $entries = [Collections.Generic.List[object]]::new()
        for ($index = $commands.Count - 1; $index -ge 0; $index--) {
            $entries.Add(@{ id = $index; command = $commands[$index] })
        }
        @{
            entries = @($entries.ToArray()); total = $commands.Count; fileTotal = $commands.Count
            sampleCount = $entries.Count; previewLimit = $entries.Count; truncated = $false; countUnit = 'command lines'
            coverage = 'Total non-empty command lines; multi-line commands can span several lines.'
        }
    }
    catch {
        @{ error = $true; message = "Failed to read PowerShell history: $($_.Exception.Message)" }
    }
}

function Get-EventLogSummary {
    try {
        $readErrors = @()
        $channels = @(Get-WinEvent -ListLog * -ErrorAction SilentlyContinue -ErrorVariable +readErrors)
        $summaries = @()
        $unknownCounts = 0
        foreach ($channel in $channels) {
            if ($null -eq $channel.RecordCount) {
                if ($channel.IsEnabled) { $unknownCounts++ }
                continue
            }
            $count = [long]$channel.RecordCount
            if ($count -lt 0) { throw 'An event log returned an invalid record count.' }
            if ($count -gt 0) {
                $summaries += @{
                    name = $channel.LogName
                    count = $count
                    sizeMb = [math]::Round($channel.FileSize / 1MB, 2)
                }
            }
        }
        $sorted = @($summaries | Sort-Object { [long]$_.count } -Descending)
        $total = [long]0
        foreach ($summary in $sorted) { $total += $summary.count }
        $incomplete = $readErrors.Count -gt 0 -or $unknownCounts -gt 0
        @{
            logs = $sorted; total = $total; logCount = $sorted.Count
            scannedLogCount = $channels.Count; countUnit = 'event records'
            partial = $incomplete; complete = -not $incomplete
            message = if ($incomplete) { 'Some event logs could not be read. The visible channel counts cover only readable logs.' } else { 'Total event records, grouped by log channel.' }
        }
    }
    catch { @{ error = $true; message = "Failed to read event log summary: $($_.Exception.Message)" } }
}

function Get-SRUMData {
    try {
        $srumPath = "$env:SystemRoot\System32\sru\SRUDB.dat"
        $database = $null
        try { $database = Get-Item -LiteralPath $srumPath -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        $files = @()
        if ($null -ne $database) {
            $files += @{
                name = $database.Name; path = $srumPath
                sizeKB = [math]::Round($database.Length / 1KB, 1)
                modified = $database.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
            }
        }
        @{
            files = $files; total = $files.Count; countUnit = 'database files'
            srumSizeMb = if ($null -ne $database) { [math]::Round($database.Length / 1MB, 2) } else { $null }
            databaseExists = $null -ne $database
            historicalRecordsAvailable = $false
            coverage = 'Database file metadata only. Historical usage records inside SRUDB.dat are not decoded.'
        }
    }
    catch { @{ error = $true; message = "Failed to read SRUM database metadata: $($_.Exception.Message)" } }
}

function Get-ConnectivityHistory {
    $entries = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        $roots = @(
            @{
                source = 'Network Profiles'
                type = 'Profile'
                path = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Profiles'
            },
            @{
                source = 'Unmanaged Signatures'
                type = 'Signature'
                path = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Unmanaged'
            },
            @{
                source = 'Managed Signatures'
                type = 'Signature'
                path = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\NetworkList\Signatures\Managed'
            }
        )

        foreach ($root in $roots) {
            try {
            try { $rootItem = Get-Item -LiteralPath $root.path -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            Get-ChildItem -LiteralPath $root.path -ErrorAction Stop | ForEach-Object {
                try {
                    $props = Get-ItemProperty -LiteralPath $_.PSPath -ErrorAction Stop
                    $name = $props.ProfileName
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = $props.Description }
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = $props.DnsSuffix }
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = $_.PSChildName }

                    $values = @{}
                    foreach ($property in $props.PSObject.Properties) {
                        if ($property.Name -notin @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider')) { $values[$property.Name] = $property.Value }
                    }
                    $entries.Add(@{
                        source      = $root.source
                        type        = $root.type
                        name        = $name
                        key         = $_.PSChildName
                        description = $props.Description
                        dnsSuffix   = $props.DnsSuffix
                        category    = $props.Category
                        registryPath = $_.PSPath
                        registryValues = $values
                    })
                } catch { $partial = $true }
            }
            } catch { $partial = $true }
        }

        @{
            entries = @($entries.ToArray())
            total   = $entries.Count
            partial = $partial
            status = if ($partial) { 'partial' } else { 'complete' }
            message = if ($partial) { 'Some network history locations could not be checked.' } else { $null }
        }
    }
    catch {
        @{ error = $true; message = "Failed to read connectivity history: $($_.Exception.Message)" }
    }
}

function Get-USBDeviceHistory {
    try {
        $devices = @()
        $presentStorageIds = New-Object 'System.Collections.Generic.HashSet[string]' ([StringComparer]::OrdinalIgnoreCase)
        $presentPnpDevices = @(Get-PnpDevice -PresentOnly:$true -ErrorAction Stop)
        if ($presentPnpDevices.Count -eq 0) { throw 'PnP returned no present devices.' }
        foreach ($pnpDevice in $presentPnpDevices) {
            $instanceId = [string]$pnpDevice.InstanceId
            if ($instanceId.StartsWith('USBSTOR\', [StringComparison]::OrdinalIgnoreCase)) {
                [void]$presentStorageIds.Add($instanceId)
            }
        }
        $root = 'HKLM:\SYSTEM\CurrentControlSet\Enum\USBSTOR'
        if (Test-Path $root) {
            Get-ChildItem $root -ErrorAction SilentlyContinue | ForEach-Object {
                $deviceClass = $_
                Get-ChildItem $deviceClass.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                    $instanceId = "USBSTOR\$($deviceClass.PSChildName)\$($_.PSChildName)"
                    if (-not $presentStorageIds.Contains($instanceId)) {
                        $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                        $devices += @{
                            deviceId     = $_.PSChildName
                            friendlyName = $props.FriendlyName
                            manufacturer = $props.Mfg
                            className    = $props.Class
                        }
                    }
                }
            }
        }
        @{ devices = $devices }
    }
    catch {
        @{ error = $true; message = "Failed to read USB history: $($_.Exception.Message)" }
    }
}





function Get-DnsCacheEntries {
    param([switch]$FleetFiltered)
    try {
        $entries = @()
        $blocklistDomains = @{}
        $hostsPath = 'C:\Windows\System32\drivers\etc\hosts'
        if ($FleetFiltered -and (Test-Path $hostsPath)) {
            $inVaultSection = $false
            Get-Content $hostsPath -ErrorAction SilentlyContinue | ForEach-Object {
                $line = $_.Trim()
                if ($line -match '^#\s*WINCOMMANDER_BLOCKLIST_START') { $inVaultSection = $true; return }
                if ($line -match '^#\s*WINCOMMANDER_BLOCKLIST_END') { $inVaultSection = $false; return }
                if (-not $inVaultSection) { return }
                if ($line -match '^(0\.0\.0\.0|127\.0\.0\.1)\s+([^\s#]+)') {
                    $domain = $Matches[2].ToLowerInvariant().TrimEnd('.')
                    if (-not $blocklistDomains.ContainsKey($domain)) { $blocklistDomains[$domain] = $true }
                }
            }
        }

        $isBlockedDomain = {
            param([string]$Name)
            if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
            $normalized = $Name.ToLowerInvariant().TrimEnd('.')
            if ($blocklistDomains.ContainsKey($normalized)) { return $true }
            $current = $normalized
            while ($current -match '\.') {
                $current = $current.Substring($current.IndexOf('.') + 1)
                if ($blocklistDomains.ContainsKey($current)) { return $true }
            }
            return $false
        }

        # DNS record-type numeric codes → human-readable names. PowerShell's
        # Get-DnsClientCache returns these as integers; without translation
        # the detail dialog showed cryptic numbers like 1, 28, 5.
        $recordTypeMap = @{
            1 = 'A'; 2 = 'NS'; 5 = 'CNAME'; 6 = 'SOA'; 12 = 'PTR'; 13 = 'HINFO';
            15 = 'MX'; 16 = 'TXT'; 17 = 'RP'; 18 = 'AFSDB'; 24 = 'SIG'; 25 = 'KEY';
            28 = 'AAAA'; 29 = 'LOC'; 33 = 'SRV'; 35 = 'NAPTR'; 36 = 'KX';
            37 = 'CERT'; 39 = 'DNAME'; 41 = 'OPT'; 42 = 'APL'; 43 = 'DS';
            44 = 'SSHFP'; 45 = 'IPSECKEY'; 46 = 'RRSIG'; 47 = 'NSEC'; 48 = 'DNSKEY';
            49 = 'DHCID'; 50 = 'NSEC3'; 51 = 'NSEC3PARAM'; 52 = 'TLSA';
            53 = 'SMIMEA'; 55 = 'HIP'; 59 = 'CDS'; 60 = 'CDNSKEY';
            61 = 'OPENPGPKEY'; 62 = 'CSYNC'; 63 = 'ZONEMD'; 64 = 'SVCB';
            65 = 'HTTPS'; 99 = 'SPF'; 257 = 'CAA';
        }
        $statusMap = @{
            0 = 'Success'; 9003 = 'NXDomain'; 9501 = 'No Records';
            9701 = 'Bad Packet'; 9702 = 'No Packet';
        }

        $cache = Get-DnsClientCache -ErrorAction Stop
        foreach ($c in $cache) {
            $name = ([string]$c.Entry).Trim()
            if ($FleetFiltered -and (& $isBlockedDomain $name)) { continue }

            # Retain the historic Fleet scope; local cleanup displays every DNS record.
            if ($FleetFiltered -and ($name.EndsWith(".in-addr.arpa", [System.StringComparison]::InvariantCultureIgnoreCase) -or
                $name -eq "0.0.0.0" -or
                $name -eq ".")) { continue }

            $rtNum = $null
            try { $rtNum = [int]$c.RecordType } catch {}
            $rtName = if ($rtNum -ne $null -and $recordTypeMap.ContainsKey($rtNum)) {
                $recordTypeMap[$rtNum]
            } else {
                "$($c.RecordType)"
            }

            $stNum = $null
            try { $stNum = [int]$c.Status } catch {}
            $stName = if ($stNum -ne $null -and $statusMap.ContainsKey($stNum)) {
                $statusMap[$stNum]
            } else {
                "$($c.Status)"
            }

            $section = "$($c.Section)"
            $dataClean = ($c.Data | Out-String).Trim()

            $entries += @{
                name        = $name
                data        = $dataClean
                recordType  = $rtName
                recordTypeRaw = $rtNum
                status      = $stName
                ttl         = $c.TimeToLive
                section     = $section
                dataLength  = $c.DataLength
                timeStamp   = "$($c.TimeStamp)"
            }
        }
        @{ entries = $entries }
    }
    catch {
        @{ error = $true; message = "Failed to read DNS cache: $($_.Exception.Message)" }
    }
}

# Fleet may request a small, fixed projection of the same trace collectors
# that power System Cleanup.  This is deliberately not a general-purpose
# forensic export: it has a closed category list, a hard row limit, and never
# returns paths, file contents, browser history, titles, or credentials.
#
# Keep the field names stable.  Fleet renders these records as a table, while
# the desktop Cleanup viewer can keep its richer local-only output.
function New-FleetForensicProjection {
    # The Fleet projection is a deliberately small, schema-first view of a
    # local Cleanup card. It is a fixed, bounded admin evidence export: the
    # existing authenticated Pro bridge is the only caller and applies the
    # display privacy policy. Values must therefore remain in the result so a
    # privacy setting can be changed without re-collecting the device.
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Category,
        [Parameter(Mandatory)][string]$Label,
        [Parameter(Mandatory)][object[]]$Columns,
        # A successful local collector may legitimately have no artifacts.  An
        # empty table must remain a valid Fleet result (0 shown / 0 total), not
        # be rejected by PowerShell's mandatory-parameter binder.
        [Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Records,
        [Parameter(Mandatory)][int]$Total,
        [bool]$Truncated = $false,
        [bool]$Redacted = $false,
        [hashtable]$Extra = @{}
    )

    $rowLimit = 200
    $byteLimit = 512KB
    $shown = @()
    $usedBytes = 0
    foreach ($record in @($Records | Select-Object -First $rowLimit)) {
        # Values are already scalar/sanitised by the per-category branch.  A
        # serialised-size check keeps a pathological local cache from making
        # the authenticated IPC reply unbounded.
        $recordBytes = 0
        try {
            $recordBytes = [System.Text.Encoding]::UTF8.GetByteCount(($record | ConvertTo-Json -Compress -Depth 6))
        } catch { $recordBytes = $byteLimit }
        if (($usedBytes + $recordBytes) -gt $byteLimit) {
            $Truncated = $true
            break
        }
        $shown += $record
        $usedBytes += $recordBytes
    }
    if ($shown.Count -lt @($Records).Count) { $Truncated = $true }

    $projection = [ordered]@{
        # Fleet rejects earlier projection contracts rather than accepting a
        # stale packaged collector that could only return permanent redaction
        # placeholders.  This is a protocol version, not a caller-selected
        # detail level.
        projection_contract_version = 2
        source       = 'wincommander.system_cleanup'
        category     = $Category
        category_id  = $Category
        label        = $Label
        collected_at = (Get-Date).ToUniversalTime().ToString('o')
        columns      = @($Columns)
        records      = @($shown)
        total        = $Total
        shown        = $shown.Count
        truncated    = $Truncated
        # `redacted` is retained for backwards-compatible consumers, but new
        # evidence is always complete. Individual columns declare whether a
        # value is sensitive so Fleet can conceal or reveal it at display time.
        redacted     = $false
        detail_mode  = 'full'
        datasets     = @([ordered]@{
            id      = $Category
            title   = $Label
            columns = @($Columns)
            rows    = @($shown)
        })
    }
    foreach ($column in @($projection.columns)) {
        # Older category arms used `type = redacted` as a transport-time
        # placeholder. Convert that to declarative presentation metadata.
        if ($column.type -eq 'redacted') {
            $column.type = 'text'
            $column.sensitive = $true
        } elseif ($null -eq $column.sensitive) {
            $column.sensitive = $false
        }
    }
    foreach ($key in $Extra.Keys) { $projection[$key] = $Extra[$key] }
    return $projection
}

function Get-FleetForensicProjection {
    [CmdletBinding()]
    param(
        [ValidateSet(
            'shell_bags', 'usb_history', 'recycle_bin', 'dns_cache',
            'clipboard_history', 'execution_audit', 'wlan_profiles',
            'net_drives', 'event_log_summary', 'command_history',
            'recent_files', 'rdp_history', 'jump_lists', 'connectivity_history',
            'browser_footprints', 'prefetch', 'shadow_copies', 'ntfs_journals',
            'amcache', 'nt_user_traces', 'notepad_state', 'compatibility_cache',
            'crash_dumps', 'search_index', 'print_spooler',
            'resource_usage_history', 'temp_database_files', 'activity_timeline',
            'web_cache_database', 'thumbnail_icon_cache', 'notification_history',
            'peer_distribution_cache', 'diagnostics_timeline', 'timeline_cache',
            'rdp_bitmap_cache', 'servicing_logs', 'device_install_logs',
            'usage_trace_logs', 'protection_history',
            'wsl_data', 'docker_desktop_data', 'virtual_machine_artifacts', 'developer_caches',
            'credential_manager', 'network_wizard_history', 'wer_history', 'inactive_user_protection_metadata',
            'sticky_notes', 'onedrive_metadata', 'spotlight_cache', 'font_cache', 'legacy_icon_cache',
            'game_captures', 'photos_cache', 'xbox_cache', 'communication_caches', 'editor_history',
            'git_activity', 'ssh_state', 'remote_access_logs', 'password_manager_caches', 'game_launcher_logs',
            'adobe_recent', 'office_temp_files', 'firewall_log', 'neighbor_cache', 'netbios_cache',
            'geolocation_cache', 'vpn_phonebooks', 'proxy_cache', 'cloud_placeholders', 'bits_queue',
            'cellular_history', 'app_launch_history', 'office_mru', 'embedded_web_cache',
            'p2p_update_cache', 'reliability_history', 'explorer_search_history', 'search_personalization',
            'process_review'
        )]
        [string]$Category = 'dns_cache',
        # This is supplied as true only by the signed Pro-to-Free bridge. It
        # is not a caller-selected disclosure level; the complete bounded
        # projection is always collected and Fleet chooses how to display it.
        [bool]$FullDetail = $true
    )

    $rowLimit = 200
    $sanitizeText = {
        param(
            [object]$Value,
            [int]$MaxLength = 256
        )

        if ($null -eq $Value) { return '' }
        $text = ([string]$Value) -replace '[\r\n\t]+', ' '
        $text = $text.Trim()
        if ($text.Length -gt $MaxLength) {
            return $text.Substring(0, $MaxLength) + '…'
        }
        return $text
    }

    # Stable opaque labels make repeated Fleet rows distinguishable without
    # disclosing the original identifier (path, SID, host, registry key, etc.).
    $opaqueId = {
        param([object]$Value)
        if ($null -eq $Value -or [string]::IsNullOrWhiteSpace([string]$Value)) { return '' }
        $bytes = [System.Text.Encoding]::UTF8.GetBytes([string]$Value)
        # ComputeHash works in both Windows PowerShell 5.1 and PowerShell 7;
        # the newer static HashData API does not exist in the former.
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try { $hash = $sha.ComputeHash($bytes) } finally { $sha.Dispose() }
        return ([System.BitConverter]::ToString($hash).Replace('-', '').Substring(0, 16)).ToLowerInvariant()
    }

    # A number of deep-cleanup cards are local file inventories. Fleet gets
    # their bounded local viewer metadata (including the locally visible
    # artifact label), with a per-column sensitive marker for its display
    # policy. This helper remains fixed-field; it never reads arbitrary paths.
    $metadataRows = {
        param([object[]]$Items, [string]$Source)
        @($Items | ForEach-Object {
            $sizeKB = 0
            try { $sizeKB = [double]$_.sizeKB } catch {}
            $modified = ''
            foreach ($property in @('modified', 'lastModified', 'lastWriteTime')) {
                $value = $null
                try { $value = $_.$property } catch {}
                if ($value) {
                    $modified = (& $sanitizeText $value 64)
                    break
                }
            }
            $artifact = ''
            foreach ($property in @('name', 'fileName', 'displayName', 'originalPath', 'path', 'fullName')) {
                $value = $null
                try { $value = $_.$property } catch {}
                if (-not [string]::IsNullOrWhiteSpace([string]$value)) {
                    $artifact = (& $sanitizeText $value 256)
                    break
                }
            }
            @{ source = $Source; artifact = $artifact; sizeKB = $sizeKB; modified = $modified }
        })
    }

    $metadataProjection = {
        param([string]$ProjectionCategory, [string]$ProjectionLabel, [hashtable]$Result, [string]$Source)
        # Each switch arm below calls its collector literally. This helper only
        # normalises the fixed local metadata shape.
        $items = @($Result.files)
        if ($items.Count -eq 0) { $items = @($Result.entries) }
        $records = & $metadataRows $items $Source
        $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'text'; sensitive = $true }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
        return (New-FleetForensicProjection -Category $ProjectionCategory -Label $ProjectionLabel -Columns $columns -Records $records -Total $items.Count)
    }

    $empty = {
        param([string]$Message)
        return @{
            error = $true
            message = $Message
            source = 'wincommander.system_cleanup'
            category = $Category
            records = @()
            total = 0
            truncated = $false
        }
    }

    try {
        switch ($Category) {
            'dns_cache' {
                $result = Get-DnsCacheEntries -FleetFiltered
                if ($result.error) { return (& $empty 'System Cleanup could not read the DNS cache.') }

                $sourceRows = @($result.entries)
                $records = @($sourceRows | Select-Object -First $rowLimit | ForEach-Object {
                    $rawData = ([string]$_.data) -replace '[\r\n\t]+', ' '
                    $dataLength = 0
                    $ttl = 0
                    try { $dataLength = [int]$_.dataLength } catch {}
                    try { $ttl = [int]$_.ttl } catch {}
                    $data = (& $sanitizeText $_.data)
                    if ([string]::IsNullOrWhiteSpace($data)) { $data = 'Not recorded locally' }
                    # Get-DnsCacheEntries exposes the display type as a blank
                    # string on current Windows builds but retains its numeric
                    # recordTypeRaw field. Preserve that collector fact rather
                    # than letting one blank cell redact the whole Fleet column.
                    $recordType = (& $sanitizeText $_.recordType 32)
                    if ([string]::IsNullOrWhiteSpace($recordType)) {
                        $recordType = "Code $([int]$_.recordTypeRaw)"
                    }
                    @{
                        name = (& $sanitizeText $_.name)
                        dataLength = $dataLength
                        'data' = $data
                        dataTruncated = ($rawData.Trim().Length -gt 256)
                        recordType = $recordType
                        section = (& $sanitizeText $_.section 32)
                        status = (& $sanitizeText $_.status 64)
                        ttl = $ttl
                    }
                })
                $columns = @(
                        @{ key = 'name'; label = 'Name'; type = 'text' },
                        @{ key = 'dataLength'; label = 'Data length'; type = 'number' },
                        @{ key = 'data'; label = 'Data'; type = 'text'; sensitive = $true },
                        @{ key = 'recordType'; label = 'Type'; type = 'text'; sensitive = $true },
                        @{ key = 'section'; label = 'Section'; type = 'text' },
                        @{ key = 'status'; label = 'Status'; type = 'text' },
                        @{ key = 'ttl'; label = 'TTL'; type = 'number' }
                )
                return (New-FleetForensicProjection -Category 'dns_cache' -Label 'DNS Cache' -Columns $columns -Records $records -Total $sourceRows.Count -Truncated ($sourceRows.Count -gt $records.Count))
            }

            'browser_footprints' {
                $result = Get-BrowserFootprints
                if ($result.error) { return (& $empty 'System Cleanup could not read browser footprint totals.') }

                # Deliberately project browser artifact *kinds* and sizes only.
                # Profile paths, history URLs, page titles, cookie values, and
                # credential-bearing files remain local to the device.
                $sourceRows = @(
                    foreach ($browser in @($result.browsers)) {
                        foreach ($artifact in @($browser.artifacts)) {
                            $sizeKB = 0
                            try { $sizeKB = [double]$artifact.sizeKB } catch {}
                            @{
                                browser = (& $sanitizeText $browser.browser 64)
                                artifact = (& $sanitizeText $artifact.name 64)
                                sizeKB = $sizeKB
                            }
                        }
                    }
                )
                $records = @($sourceRows | Select-Object -First $rowLimit)
                $columns = @(
                        @{ key = 'browser'; label = 'Browser'; type = 'text' },
                        @{ key = 'artifact'; label = 'Artifact'; type = 'text' },
                        @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }
                )
                return (New-FleetForensicProjection -Category 'browser_footprints' -Label 'Browser footprints' -Columns $columns -Records $records -Total $sourceRows.Count -Truncated ($sourceRows.Count -gt $records.Count))
            }

            'event_log_summary' {
                $result = Get-EventLogSummary
                if ($result.error) { return (& $empty 'System Cleanup could not read event-log totals.') }

                $sourceRows = @($result.logs)
                $records = @($sourceRows | Select-Object -First $rowLimit | ForEach-Object {
                    $count = 0
                    $sizeMb = 0
                    try { $count = [int64]$_.count } catch {}
                    try { $sizeMb = [double]$_.sizeMb } catch {}
                    @{
                        name = (& $sanitizeText $_.name 128)
                        count = $count
                        newest = (& $sanitizeText $_.newest 64)
                        sizeMb = $sizeMb
                    }
                })
                $columns = @(
                        @{ key = 'name'; label = 'Log'; type = 'text' },
                        @{ key = 'count'; label = 'Records'; type = 'number' },
                        @{ key = 'newest'; label = 'Newest'; type = 'timestamp' },
                        @{ key = 'sizeMb'; label = 'Size (MB)'; type = 'number' }
                )
                return (New-FleetForensicProjection -Category 'event_log_summary' -Label 'Event log summary' -Columns $columns -Records $records -Total $sourceRows.Count -Truncated ($sourceRows.Count -gt $records.Count))
            }

            'prefetch' {
                $result = Get-PrefetchFiles
                if ($result.error) { return (& $empty 'System Cleanup could not read Prefetch summaries.') }

                $sourceRows = @($result.entries)
                $records = @($sourceRows | Select-Object -First $rowLimit | ForEach-Object {
                    $sizeKB = 0
                    try { $sizeKB = [double]$_.sizeKB } catch {}
                    @{
                        name = (& $sanitizeText $_.name 128)
                        fileName = (& $sanitizeText $_.fileName 128)
                        sizeKB = $sizeKB
                        lastRun = (& $sanitizeText $_.lastRun 64)
                        created = (& $sanitizeText $_.created 64)
                    }
                })
                $columns = @(
                        @{ key = 'name'; label = 'Application'; type = 'text' },
                        @{ key = 'fileName'; label = 'Prefetch file'; type = 'text' },
                        @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' },
                        @{ key = 'lastRun'; label = 'Last run'; type = 'timestamp' },
                        @{ key = 'created'; label = 'Created'; type = 'timestamp' }
                )
                return (New-FleetForensicProjection -Category 'prefetch' -Label 'Prefetch' -Columns $columns -Records $records -Total $sourceRows.Count -Truncated ($sourceRows.Count -gt $records.Count) -Extra @{ accessDenied = [bool]$result.accessDenied })
            }

            'shell_bags' {
                $result = Get-ShellBags
                if ($result.error) { return (& $empty 'System Cleanup could not read ShellBag summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    @{ record = (& $sanitizeText $_.path 256); lastModified = (& $sanitizeText $_.lastModified 64) }
                })
                $columns = @(
                    @{ key = 'record'; label = 'Record'; type = 'text' },
                    @{ key = 'lastModified'; label = 'Last modified'; type = 'timestamp' }
                )
                $columns[0].sensitive = $true
                return (New-FleetForensicProjection -Category 'shell_bags' -Label 'ShellBags' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'usb_history' {
                $result = Get-USBDeviceHistory
                if ($result.error) { return (& $empty 'System Cleanup could not read USB-history summaries.') }
                $sourceRows = @($result.devices)
                $records = @($sourceRows | ForEach-Object {
                    @{
                        deviceId = (& $sanitizeText $_.deviceId 256)
                        friendlyName = (& $sanitizeText $_.friendlyName 128)
                        manufacturer = (& $sanitizeText $_.manufacturer 128)
                        className = (& $sanitizeText $_.className 64)
                    }
                })
                $columns = @(
                    @{ key = 'deviceId'; label = 'Device ID'; type = 'text'; sensitive = $true },
                    @{ key = 'friendlyName'; label = 'Device'; type = 'text' },
                    @{ key = 'manufacturer'; label = 'Manufacturer'; type = 'text' },
                    @{ key = 'className'; label = 'Class'; type = 'text' }
                )
                return (New-FleetForensicProjection -Category 'usb_history' -Label 'USB history' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'recycle_bin' {
                $result = Get-RecycleBinInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read Recycle Bin summaries.') }
                $sourceRows = @($result.items)
                $records = @($sourceRows | ForEach-Object {
                    $sizeBytes = 0
                    $sizeKB = 0
                    try { $sizeBytes = [int64]$_.sizeBytes } catch {}
                    try { $sizeKB = [double]$_.sizeKB } catch {}
                    @{
                        item = (& $sanitizeText $_.originalPath 256)
                        account = (& $sanitizeText $_.account 128)
                        deletedTime = (& $sanitizeText $_.deletedTime 64)
                        sizeBytes = $sizeBytes
                        sizeKB = $sizeKB
                        drive = (& $sanitizeText $_.drive 16)
                    }
                })
                $columns = @(
                    @{ key = 'item'; label = 'Item'; type = 'text' },
                    @{ key = 'account'; label = 'Account'; type = 'text'; sensitive = $true },
                    @{ key = 'deletedTime'; label = 'Deleted'; type = 'timestamp' },
                    @{ key = 'sizeBytes'; label = 'Size (bytes)'; type = 'number' },
                    @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' },
                    @{ key = 'drive'; label = 'Drive'; type = 'text' }
                )
                $columns[0].sensitive = $true
                return (New-FleetForensicProjection -Category 'recycle_bin' -Label 'Recycle Bin' -Columns $columns -Records $records -Total $sourceRows.Count -Extra @{ totalSizeKB = [double]$result.totalSizeKB })
            }

            'clipboard_history' {
                $result = Get-ClipboardHistoryStatus
                if ($result.error) { return (& $empty 'System Cleanup could not read clipboard-history status.') }
                $sourceRows = @($result.historyItems)
                $records = @($sourceRows | ForEach-Object {
                    $count = 0
                    try { $count = [int]$_.charCount } catch {}
                    @{ itemType = (& $sanitizeText $_.type 32); preview = (& $sanitizeText $_.preview 256); charCount = $count }
                })
                $columns = @(
                    @{ key = 'itemType'; label = 'Type'; type = 'text' },
                    @{ key = 'preview'; label = 'Preview'; type = 'text'; sensitive = $true },
                    @{ key = 'charCount'; label = 'Characters'; type = 'number' }
                )
                return (New-FleetForensicProjection -Category 'clipboard_history' -Label 'Clipboard history' -Columns $columns -Records $records -Total $sourceRows.Count -Extra @{
                    clipboardHistoryDisabled = [bool]$result.clipboardHistoryDisabled
                    cloudClipboardDisabled = [bool]$result.cloudClipboardDisabled
                })
            }

            'execution_audit' {
                $result = Get-ExecutionCache
                if ($result.error) { return (& $empty 'System Cleanup could not read execution-audit summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    @{ source = (& $sanitizeText $_.source 64); record = (& $sanitizeText $_.path 256) }
                })
                $columns = @(
                    @{ key = 'source'; label = 'Source'; type = 'text' },
                    @{ key = 'record'; label = 'Record'; type = 'text'; sensitive = $true }
                )
                return (New-FleetForensicProjection -Category 'execution_audit' -Label 'Execution audit' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'wlan_profiles' {
                # Fleet needs profile evidence, never Wi-Fi credentials.
                $result = Get-WlanProfiles -IncludeSecrets:$false
                if ($result.error) { return (& $empty 'System Cleanup could not read WLAN-profile summaries.') }
                $sourceRows = @($result.profiles)
                $records = @($sourceRows | ForEach-Object {
                    $name = (& $sanitizeText $_.name 128)
                    if ([string]::IsNullOrWhiteSpace($name)) { $name = 'Not recorded locally' }
                    @{ name = $name }
                })
                $columns = @(@{ key = 'name'; label = 'Profile'; type = 'text'; sensitive = $true })
                return (New-FleetForensicProjection -Category 'wlan_profiles' -Label 'WLAN profiles' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'net_drives' {
                $result = Get-NetworkDrives
                if ($result.error) { return (& $empty 'System Cleanup could not read network-drive summaries.') }
                $sourceRows = @($result.drives)
                $records = @($sourceRows | ForEach-Object {
                    @{ drive = (& $sanitizeText $_.Name 16); remote = (& $sanitizeText $_.DisplayRoot 256) }
                })
                $columns = @(
                    @{ key = 'drive'; label = 'Drive'; type = 'text' },
                    @{ key = 'remote'; label = 'Remote location'; type = 'text'; sensitive = $true }
                )
                return (New-FleetForensicProjection -Category 'net_drives' -Label 'Network drives' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'command_history' {
                $result = Get-PSHistory
                if ($result.error) { return (& $empty 'System Cleanup could not read command-history summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object { @{ id = [int]$_.id; command = (& $sanitizeText $_.command 256) } })
                $columns = @(
                    @{ key = 'id'; label = 'Record'; type = 'number' },
                    @{ key = 'command'; label = 'Command'; type = 'text'; sensitive = $true }
                )
                return (New-FleetForensicProjection -Category 'command_history' -Label 'Command history' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'recent_files' {
                $result = Get-RecentFiles
                if ($result.error) { return (& $empty 'System Cleanup could not read recent-file summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    $sizeBytes = 0
                    try { $sizeBytes = [int64]$_.sizeBytes } catch {}
                    @{
                        item = (& $sanitizeText $_.name 256)
                        target = (& $sanitizeText $_.target 256)
                        extension = (& $sanitizeText $_.extension 32)
                        lastModified = (& $sanitizeText $_.lastModified 64)
                        sizeBytes = $sizeBytes
                    }
                })
                $columns = @(
                    @{ key = 'item'; label = 'Item'; type = 'text'; sensitive = $true },
                    @{ key = 'target'; label = 'Target'; type = 'text'; sensitive = $true },
                    @{ key = 'extension'; label = 'Extension'; type = 'text' },
                    @{ key = 'lastModified'; label = 'Last modified'; type = 'timestamp' },
                    @{ key = 'sizeBytes'; label = 'Size (bytes)'; type = 'number' }
                )
                return (New-FleetForensicProjection -Category 'recent_files' -Label 'Recent files' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'rdp_history' {
                $result = Get-RDPHistory
                if ($result.error) { return (& $empty 'System Cleanup could not read RDP-history summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    $endpoint = (& $sanitizeText $_.host 256)
                    if ([string]::IsNullOrWhiteSpace($endpoint)) { $endpoint = 'Not recorded locally' }
                    $identity = (& $sanitizeText $_.username 128)
                    if ([string]::IsNullOrWhiteSpace($identity)) { $identity = 'Not recorded locally' }
                    $lastModified = (& $sanitizeText $_.lastModified 64)
                    if ([string]::IsNullOrWhiteSpace($lastModified)) { $lastModified = 'Not recorded locally' }
                    @{ type = (& $sanitizeText $_.type 32); endpoint = $endpoint; identity = $identity; lastModified = $lastModified }
                })
                $columns = @(
                    @{ key = 'type'; label = 'Type'; type = 'text' },
                    @{ key = 'endpoint'; label = 'Endpoint'; type = 'text'; sensitive = $true },
                    @{ key = 'identity'; label = 'Identity'; type = 'text'; sensitive = $true },
                    # Windows may not retain a timestamp for every RDP entry.
                    # A text field faithfully carries either the local timestamp
                    # string or the explicit unavailable marker without making
                    # the entire column permanently redacted.
                    @{ key = 'lastModified'; label = 'Last modified'; type = 'text' }
                )
                return (New-FleetForensicProjection -Category 'rdp_history' -Label 'RDP history' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'jump_lists' {
                $result = Get-JumpLists
                if ($result.error) { return (& $empty 'System Cleanup could not read Jump List summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    $sizeKB = 0
                    try { $sizeKB = [double]$_.sizeKB } catch {}
                    @{ name = (& $sanitizeText $_.name 256); type = (& $sanitizeText $_.type 32); sizeKB = $sizeKB; lastModified = (& $sanitizeText $_.lastModified 64) }
                })
                $columns = @(
                    @{ key = 'name'; label = 'List'; type = 'text'; sensitive = $true },
                    @{ key = 'type'; label = 'Type'; type = 'text' },
                    @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' },
                    @{ key = 'lastModified'; label = 'Last modified'; type = 'timestamp' }
                )
                return (New-FleetForensicProjection -Category 'jump_lists' -Label 'Jump Lists' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'connectivity_history' {
                $result = Get-ConnectivityHistory
                if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete connectivity-history summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    @{ source = (& $sanitizeText $_.source 64); type = (& $sanitizeText $_.type 32); category = (& $sanitizeText $_.category 32); network = (& $sanitizeText $_.name 256); key = (& $sanitizeText $_.key 256); description = (& $sanitizeText $_.description 256); dnsSuffix = (& $sanitizeText $_.dnsSuffix 256) }
                })
                $columns = @(
                    @{ key = 'source'; label = 'Source'; type = 'text' },
                    @{ key = 'type'; label = 'Type'; type = 'text' },
                    @{ key = 'category'; label = 'Category'; type = 'text' },
                    @{ key = 'network'; label = 'Network'; type = 'text'; sensitive = $true },
                    @{ key = 'key'; label = 'Registry key'; type = 'text'; sensitive = $true },
                    @{ key = 'description'; label = 'Description'; type = 'text'; sensitive = $true },
                    @{ key = 'dnsSuffix'; label = 'DNS suffix'; type = 'text'; sensitive = $true }
                )
                return (New-FleetForensicProjection -Category 'connectivity_history' -Label 'Connectivity history' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'shadow_copies' {
                $result = Get-ShadowCopies
                if ($result.error) { return (& $empty 'System Cleanup could not read shadow-copy summaries.') }
                $sourceRows = @($result.copies)
                $records = @($sourceRows | ForEach-Object {
                    @{
                        id = (& $sanitizeText $_.id 256)
                        drive = (& $sanitizeText $_.drive 16)
                        created = (& $sanitizeText $_.created 64)
                        clientAccessible = [bool]$_.clientAccessible
                        persistent = [bool]$_.persistent
                        state = (& $sanitizeText $_.stateStr 64)
                    }
                })
                $columns = @(
                    @{ key = 'id'; label = 'Copy ID'; type = 'text'; sensitive = $true },
                    @{ key = 'drive'; label = 'Drive'; type = 'text' },
                    @{ key = 'created'; label = 'Created'; type = 'timestamp' },
                    @{ key = 'clientAccessible'; label = 'Client accessible'; type = 'boolean' },
                    @{ key = 'persistent'; label = 'Persistent'; type = 'boolean' },
                    @{ key = 'state'; label = 'State'; type = 'text' }
                )
                return (New-FleetForensicProjection -Category 'shadow_copies' -Label 'Shadow copies' -Columns $columns -Records $records -Total $sourceRows.Count -Extra @{ vssRunning = [bool]$result.vssRunning })
            }

            'ntfs_journals' {
                $result = Get-NTFSJournals
                if ($result.error) { return (& $empty 'System Cleanup could not read NTFS-journal summaries.') }
                $sourceRows = @($result.journals)
                $records = @($sourceRows | ForEach-Object {
                    @{ drive = (& $sanitizeText $_.drive 16); present = [bool]$_.present; journalId = (& $sanitizeText $_.journalId 128); maxSize = (& $sanitizeText $_.maxSize 64) }
                })
                $columns = @(
                    @{ key = 'drive'; label = 'Drive'; type = 'text' },
                    @{ key = 'present'; label = 'Present'; type = 'boolean' },
                    @{ key = 'journalId'; label = 'Journal ID'; type = 'text'; sensitive = $true },
                    @{ key = 'maxSize'; label = 'Maximum size'; type = 'text' }
                )
                return (New-FleetForensicProjection -Category 'ntfs_journals' -Label 'NTFS journals' -Columns $columns -Records $records -Total $sourceRows.Count)
            }

            'amcache' {
                $result = Get-AmcacheEntries -FleetProjection
                if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete Amcache summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object { @{ category = (& $sanitizeText $_.category 96); count = [int]$_.count; sample = (& $sanitizeText (($_.sample | ConvertTo-Json -Compress -Depth 3)) 256) } })
                $columns = @(@{ key = 'category'; label = 'Category'; type = 'text' }, @{ key = 'count'; label = 'Records'; type = 'number' }, @{ key = 'sample'; label = 'Sample'; type = 'text'; sensitive = $true })
                return (New-FleetForensicProjection -Category 'amcache' -Label 'Amcache' -Columns $columns -Records $records -Total ([int]$result.total) -Extra @{ hveFileExists = [bool]$result.hveFileExists; hveFileSizeMb = [double]$result.hveFileSizeMb })
            }

            'nt_user_traces' {
                $result = Get-NTUserTraces
                if ($result.error) { return (& $empty 'System Cleanup could not read NTUSER trace summaries.') }
                # The local collector already provides a small fixed set of
                # trace key/value pairs. Flatten it into rows rather than
                # discarding the values into a category-level count.
                $sourceRows = @(
                    foreach ($section in @($result.sections)) {
                        foreach ($entry in @($section.entries)) {
                            @{
                                section = (& $sanitizeText $section.name 64)
                                key = (& $sanitizeText $entry.key 64)
                                value = (& $sanitizeText $entry.value 256)
                            }
                        }
                    }
                )
                $columns = @(
                    @{ key = 'section'; label = 'Section'; type = 'text' },
                    @{ key = 'key'; label = 'Key'; type = 'text'; sensitive = $true },
                    @{ key = 'value'; label = 'Value'; type = 'text'; sensitive = $true }
                )
                return (New-FleetForensicProjection -Category 'nt_user_traces' -Label 'NTUSER traces' -Columns $columns -Records $sourceRows -Total ([int]$result.total))
            }

            'notepad_state' {
                $result = Get-NotepadStateFiles
                if ($result.error) { return (& $empty 'System Cleanup could not read Notepad-state summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Notepad state'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'notepad_state' -Label 'Notepad state' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'compatibility_cache' {
                $result = Get-PCAInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read compatibility-cache summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Compatibility cache'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'compatibility_cache' -Label 'Compatibility cache' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true -Extra @{ serviceState = (& $sanitizeText $result.pcaSvcState 32) })
            }

            'crash_dumps' {
                $result = Get-CrashDumpList
                if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read crash-dump summaries.') }
                $sourceRows = @($result.dumps)
                $records = & $metadataRows $sourceRows 'Crash dump'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'crash_dumps' -Label 'Crash dumps' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'search_index' {
                $result = Get-SearchIndexInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read search-index summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Search index'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'search_index' -Label 'Search index' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true -Extra @{ serviceState = (& $sanitizeText $result.wsearchState 32) })
            }

            'print_spooler' {
                $result = Get-PrintSpoolerInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read print-spooler summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Print spooler'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'print_spooler' -Label 'Print spooler' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true -Extra @{ serviceState = (& $sanitizeText $result.spoolerState 32) })
            }

            'resource_usage_history' {
                $result = Get-SRUMData
                if ($result.error) { return (& $empty 'System Cleanup could not read resource-usage summaries.') }
                $sourceRows = @($result.entries)
                $records = @($sourceRows | ForEach-Object {
                    @{ name = (& $sanitizeText $_.name 128); pid = [int]$_.pid; cpuTime = (& $sanitizeText $_.cpuTime 32); memoryKB = [double]$_.memoryKB; threadCount = [int]$_.threadCount; path = (& $sanitizeText $_.path 256); owner = (& $sanitizeText $_.owner 128) }
                })
                $columns = @(@{ key = 'name'; label = 'Process'; type = 'text' }, @{ key = 'pid'; label = 'PID'; type = 'number' }, @{ key = 'cpuTime'; label = 'CPU time'; type = 'text' }, @{ key = 'memoryKB'; label = 'Memory (KB)'; type = 'number' }, @{ key = 'threadCount'; label = 'Threads'; type = 'number' }, @{ key = 'path'; label = 'Path'; type = 'redacted' }, @{ key = 'owner'; label = 'Owner'; type = 'redacted' })
                return (New-FleetForensicProjection -Category 'resource_usage_history' -Label 'Resource usage history' -Columns $columns -Records $records -Total $sourceRows.Count -Extra @{ srumSizeMb = [double]$result.srumSizeMb })
            }

            'temp_database_files' {
                $result = Get-SQLiteWALList
                if ($result.error) { return (& $empty 'System Cleanup could not read temporary-database summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Temporary database'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'temp_database_files' -Label 'Temporary database files' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'activity_timeline' {
                $result = Get-RecallDatabaseInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read activity-timeline summaries.') }
                $sourceRows = @($result.databases)
                $records = & $metadataRows $sourceRows 'Activity timeline'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'activity_timeline' -Label 'Activity timeline' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'web_cache_database' {
                $result = Get-WebCacheInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read web-cache summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Web cache'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'web_cache_database' -Label 'Web cache database' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'thumbnail_icon_cache' {
                $result = Get-ThumbnailCacheInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read thumbnail-cache summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Thumbnail or icon cache'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'thumbnail_icon_cache' -Label 'Thumbnail and icon cache' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'notification_history' {
                $result = Get-NotificationDatabaseInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read notification-history summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Notification history'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'notification_history' -Label 'Notification history' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'peer_distribution_cache' {
                $result = Get-BranchCacheInfo
                if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read peer-distribution summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Peer distribution cache'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'peer_distribution_cache' -Label 'Peer distribution cache' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'diagnostics_timeline' {
                $result = Get-EventTranscriptInfo
                if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read diagnostics-timeline summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Diagnostics timeline'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'diagnostics_timeline' -Label 'Diagnostics timeline' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'timeline_cache' {
                $result = Get-ActivitiesTimelineInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read timeline-cache summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Timeline cache'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'timeline_cache' -Label 'Timeline cache' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'rdp_bitmap_cache' {
                $result = Get-RdpBitmapCacheInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read RDP bitmap-cache summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'RDP bitmap cache'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'rdp_bitmap_cache' -Label 'RDP bitmap cache' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'servicing_logs' {
                $result = Get-ServicingLogsInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read servicing-log summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Servicing log'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'servicing_logs' -Label 'Servicing logs' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'device_install_logs' {
                $result = Get-DeviceInstallLogsInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read device-install-log summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Device installation log'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'device_install_logs' -Label 'Device installation logs' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'usage_trace_logs' {
                $result = Get-UsageTraceLogsInfo
                if ($result.error) { return (& $empty 'System Cleanup could not read usage-trace-log summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Usage trace log'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'usage_trace_logs' -Label 'Usage trace logs' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'protection_history' {
                $result = Get-DefenderHistoryInfo
                if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete protection-history summaries.') }
                $sourceRows = @($result.files)
                $records = & $metadataRows $sourceRows 'Protection history'
                $columns = @(@{ key = 'source'; label = 'Source'; type = 'text' }, @{ key = 'artifact'; label = 'Artifact'; type = 'redacted' }, @{ key = 'sizeKB'; label = 'Size (KB)'; type = 'number' }, @{ key = 'modified'; label = 'Modified'; type = 'timestamp' })
                return (New-FleetForensicProjection -Category 'protection_history' -Label 'Protection history' -Columns $columns -Records $records -Total $sourceRows.Count -Redacted $true)
            }

            'wsl_data' { $result = Get-WSLDataInfo; if ($result.error) { return (& $empty 'System Cleanup could not read WSL-data summaries.') }; return (& $metadataProjection 'wsl_data' 'WSL data' $result 'WSL data') }
            'docker_desktop_data' { $result = Get-DockerDesktopDataInfo; if ($result.error) { return (& $empty 'System Cleanup could not read Docker-data summaries.') }; return (& $metadataProjection 'docker_desktop_data' 'Docker Desktop data' $result 'Docker Desktop data') }
            'virtual_machine_artifacts' { $result = Get-VirtualMachineArtifactsInfo; if ($result.error) { return (& $empty 'System Cleanup could not read virtual-machine summaries.') }; return (& $metadataProjection 'virtual_machine_artifacts' 'Virtual machine artifacts' $result 'Virtual machine artifact') }
            'developer_caches' { $result = Get-DeveloperCachesInfo; if ($result.error) { return (& $empty 'System Cleanup could not read developer-cache summaries.') }; return (& $metadataProjection 'developer_caches' 'Developer caches' $result 'Developer cache') }
            'credential_manager' { $result = Get-CredentialManagerInfo; if ($result.error) { return (& $empty 'System Cleanup could not read credential-manager summaries.') }; return (& $metadataProjection 'credential_manager' 'Credential Manager' $result 'Credential metadata') }
            'network_wizard_history' { $result = Get-NetworkWizardHistoryInfo; if ($result.error) { return (& $empty 'System Cleanup could not read network-wizard summaries.') }; return (& $metadataProjection 'network_wizard_history' 'Network wizard history' $result 'Network wizard metadata') }
            'wer_history' { $result = Get-WERHistoryInfo; if ($result.error) { return (& $empty 'System Cleanup could not read WER-history summaries.') }; return (& $metadataProjection 'wer_history' 'Windows Error Reporting history' $result 'WER metadata') }
            'inactive_user_protection_metadata' { $result = Get-InactiveUserProtectionMetadataInfo; if ($result.error) { return (& $empty 'System Cleanup could not read inactive-user protection summaries.') }; return (& $metadataProjection 'inactive_user_protection_metadata' 'Inactive-user protection metadata' $result 'Protection metadata') }
            'sticky_notes' { $result = Get-StickyNotesInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete Sticky Notes summaries.') }; return (& $metadataProjection 'sticky_notes' 'Sticky Notes' $result 'Sticky Notes metadata') }
            'onedrive_metadata' { $result = Get-OneDriveMetadataInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read OneDrive metadata.') }; return (& $metadataProjection 'onedrive_metadata' 'OneDrive metadata' $result 'OneDrive metadata') }
            'spotlight_cache' { $result = Get-SpotlightCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read Spotlight-cache summaries.') }; return (& $metadataProjection 'spotlight_cache' 'Spotlight cache' $result 'Spotlight cache') }
            'font_cache' { $result = Get-FontCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read font-cache summaries.') }; return (& $metadataProjection 'font_cache' 'Font cache' $result 'Font cache') }
            'legacy_icon_cache' { $result = Get-LegacyIconCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read icon-cache summaries.') }; return (& $metadataProjection 'legacy_icon_cache' 'Legacy icon cache' $result 'Icon cache') }
            'game_captures' { $result = Get-GameCapturesInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete game-capture summaries.') }; return (& $metadataProjection 'game_captures' 'Game captures' $result 'Game capture metadata') }
            'photos_cache' { $result = Get-PhotosCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read photo-cache summaries.') }; return (& $metadataProjection 'photos_cache' 'Photos cache' $result 'Photos cache') }
            'xbox_cache' { $result = Get-XboxCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read Xbox-cache summaries.') }; return (& $metadataProjection 'xbox_cache' 'Xbox cache' $result 'Xbox cache') }
            'communication_caches' { $result = Get-CommunicationCachesInfo; if ($result.error) { return (& $empty 'System Cleanup could not read communication-cache summaries.') }; return (& $metadataProjection 'communication_caches' 'Communication caches' $result 'Communication cache') }
            'editor_history' { $result = Get-EditorHistoryInfo; if ($result.error) { return (& $empty 'System Cleanup could not read editor-history summaries.') }; return (& $metadataProjection 'editor_history' 'Editor history' $result 'Editor history metadata') }
            'git_activity' { $result = Get-GitActivityInfo; if ($result.error) { return (& $empty 'System Cleanup could not read Git-activity summaries.') }; return (& $metadataProjection 'git_activity' 'Git activity' $result 'Git activity metadata') }
            'ssh_state' { $result = Get-SSHStateInfo; if ($result.error) { return (& $empty 'System Cleanup could not read SSH-state summaries.') }; return (& $metadataProjection 'ssh_state' 'SSH state' $result 'SSH metadata') }
            'remote_access_logs' { $result = Get-RemoteAccessLogsInfo; if ($result.error) { return (& $empty 'System Cleanup could not read remote-access summaries.') }; return (& $metadataProjection 'remote_access_logs' 'Remote access logs' $result 'Remote access metadata') }
            'password_manager_caches' { $result = Get-PasswordManagerCachesInfo; if ($result.error) { return (& $empty 'System Cleanup could not read password-manager summaries.') }; return (& $metadataProjection 'password_manager_caches' 'Password-manager caches' $result 'Password-manager metadata') }
            'game_launcher_logs' { $result = Get-GameLauncherLogsInfo; if ($result.error) { return (& $empty 'System Cleanup could not read game-launcher summaries.') }; return (& $metadataProjection 'game_launcher_logs' 'Game launcher logs' $result 'Game launcher log') }
            'adobe_recent' { $result = Get-AdobeRecentInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read Adobe-recent summaries.') }; return (& $metadataProjection 'adobe_recent' 'Adobe recent items' $result 'Adobe recent metadata') }
            'office_temp_files' { $result = Get-OfficeTempFilesInfo; if ($result.error) { return (& $empty 'System Cleanup could not read Office-temporary-file summaries.') }; return (& $metadataProjection 'office_temp_files' 'Office temporary files' $result 'Office temporary metadata') }
            'firewall_log' { $result = Get-FirewallLogInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read firewall-log summaries.') }; return (& $metadataProjection 'firewall_log' 'Firewall log' $result 'Firewall log metadata') }
            'neighbor_cache' { $result = Get-NeighborCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read neighbor-cache summaries.') }; return (& $metadataProjection 'neighbor_cache' 'Neighbor cache' $result 'Neighbor cache metadata') }
            'netbios_cache' { $result = Get-NetBIOSCacheInfo; if ($result.error) { return (& $empty 'System Cleanup could not read NetBIOS-cache summaries.') }; return (& $metadataProjection 'netbios_cache' 'NetBIOS cache' $result 'NetBIOS cache metadata') }
            'geolocation_cache' { $result = Get-GeolocationCacheInfo; if ($result.error) { return (& $empty 'System Cleanup could not read geolocation-cache summaries.') }; return (& $metadataProjection 'geolocation_cache' 'Geolocation cache' $result 'Geolocation metadata') }
            'vpn_phonebooks' { $result = Get-VPNPhonebooksInfo; if ($result.error) { return (& $empty 'System Cleanup could not read VPN-phonebook summaries.') }; return (& $metadataProjection 'vpn_phonebooks' 'VPN phonebooks' $result 'VPN metadata') }
            'proxy_cache' { $result = Get-ProxyCacheInfo; if ($result.error) { return (& $empty 'System Cleanup could not read proxy-cache summaries.') }; return (& $metadataProjection 'proxy_cache' 'Proxy cache' $result 'Proxy metadata') }
            'cloud_placeholders' { $result = Get-CloudPlaceholdersInfo; if ($result.error) { return (& $empty 'System Cleanup could not read cloud-placeholder summaries.') }; return (& $metadataProjection 'cloud_placeholders' 'Cloud placeholders' $result 'Cloud placeholder metadata') }
            'bits_queue' { $result = Get-BITSQueueInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete BITS-queue summaries.') }; return (& $metadataProjection 'bits_queue' 'BITS queue' $result 'BITS metadata') }
            'cellular_history' { $result = Get-CellularHistoryInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read cellular-history summaries.') }; return (& $metadataProjection 'cellular_history' 'Cellular history' $result 'Cellular metadata') }
            'app_launch_history' { $result = Get-AppLaunchHistoryInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete app-launch-history summaries.') }; return (& $metadataProjection 'app_launch_history' 'App launch history' $result 'App launch metadata') }
            'office_mru' { $result = Get-OfficeMruInfo; if ($result.error) { return (& $empty 'System Cleanup could not read Office-MRU summaries.') }; return (& $metadataProjection 'office_mru' 'Office MRU' $result 'Office MRU metadata') }
            'embedded_web_cache' { $result = Get-EmbeddedWebCacheInfo; if ($result.error) { return (& $empty 'System Cleanup could not read embedded-web-cache summaries.') }; return (& $metadataProjection 'embedded_web_cache' 'Embedded web cache' $result 'Embedded web metadata') }
            'p2p_update_cache' { $result = Get-P2PUpdateCacheInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete peer-update-cache summaries.') }; return (& $metadataProjection 'p2p_update_cache' 'Peer update cache' $result 'Peer update metadata') }
            'reliability_history' { $result = Get-ReliabilityHistoryInfo; if ($result.error -or $result.partial) { return (& $empty 'System Cleanup could not read complete reliability-history summaries.') }; return (& $metadataProjection 'reliability_history' 'Reliability history' $result 'Reliability metadata') }
            'explorer_search_history' { $result = Get-ExplorerSearchHistoryInfo; if ($result.error) { return (& $empty 'System Cleanup could not read Explorer-search-history summaries.') }; return (& $metadataProjection 'explorer_search_history' 'Explorer search metadata' $result 'Explorer search metadata') }
            'search_personalization' { $result = Get-SearchPersonalizationInfo; if ($result.error) { return (& $empty 'System Cleanup could not read search-personalisation summaries.') }; return (& $metadataProjection 'search_personalization' 'Search personalisation' $result 'Search personalisation metadata') }

            'process_review' {
                $result = Get-ProcessIntelligence
                if ($result.error) { return (& $empty 'System Cleanup could not read process summaries.') }
                $sourceRows = @($result.processes)
                $records = @($sourceRows | ForEach-Object { @{ name = (& $sanitizeText $_.name 128); pid = [int]$_.pid; signed = (& $sanitizeText $_.signed 32); signer = (& $sanitizeText $_.signer 128); elevated = (& $sanitizeText $_.elevated 16); path = (& $sanitizeText $_.path 256) } })
                $columns = @(@{ key = 'name'; label = 'Process'; type = 'text' }, @{ key = 'pid'; label = 'PID'; type = 'number' }, @{ key = 'signed'; label = 'Signature'; type = 'text' }, @{ key = 'signer'; label = 'Signer'; type = 'text' }, @{ key = 'elevated'; label = 'Elevated'; type = 'text' }, @{ key = 'path'; label = 'Path'; type = 'redacted' })
                return (New-FleetForensicProjection -Category 'process_review' -Label 'Process review' -Columns $columns -Records $records -Total $sourceRows.Count)
            }
        }
    }
    catch {
        # Fleet receives a category-level failure only.  Do not export raw
        # exception text because it can include paths or account names.
        return (& $empty 'System Cleanup could not prepare the requested forensic records.')
    }
}

function Get-CleanupProcessElevation {
    param([int]$ProcessId)
    try {
        if (-not ('WcCleanup.ProcessElevation' -as [type])) {
            Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace WcCleanup {
    public static class ProcessElevation {
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern IntPtr OpenProcess(uint access, bool inheritHandle, int processId);
        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
        [DllImport("advapi32.dll", SetLastError = true)]
        private static extern bool GetTokenInformation(IntPtr token, int infoClass, out int elevation, int length, out int returnedLength);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);
        public static int Read(int processId) {
            IntPtr process = OpenProcess(0x1000, false, processId); // PROCESS_QUERY_LIMITED_INFORMATION
            if (process == IntPtr.Zero) return -1;
            IntPtr token = IntPtr.Zero;
            try {
                if (!OpenProcessToken(process, 0x0008, out token)) return -1; // TOKEN_QUERY
                int elevation, returned;
                if (!GetTokenInformation(token, 20, out elevation, sizeof(int), out returned) || returned < sizeof(int)) return -1;
                return elevation == 0 ? 0 : 1; // TokenElevation
            } finally {
                if (token != IntPtr.Zero) CloseHandle(token);
                CloseHandle(process);
            }
        }
    }
}
'@ -ErrorAction Stop
        }
        $elevation = [WcCleanup.ProcessElevation]::Read($ProcessId)
        if ($elevation -eq 1) { return 'Yes' }
        if ($elevation -eq 0) { return 'No' }
    } catch { }
    return 'Unknown'
}

function Get-ProcessIntelligence {
    $entries = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        $procs = @(Get-Process -ErrorAction Stop)
        foreach ($p in $procs) {
            try {
            $processId = $p.Id
            if ($processId -le 0) { $partial = $true; continue }
            $processName = $p.ProcessName
            if ([string]::IsNullOrWhiteSpace([string]$processName)) { $partial = $true; continue }
            $path = $null
            $sigStatus = "UnknownError"
            $signer = $null
            try {
                $path = $p.MainModule.FileName
            }
            catch { $partial = $true }
            if ([string]::IsNullOrWhiteSpace([string]$path)) { $partial = $true }
            if ($path -and (Test-Path -LiteralPath $path -ErrorAction SilentlyContinue)) {
                try {
                    $sig = Get-AuthenticodeSignature -LiteralPath $path -ErrorAction Stop
                    if ($sig) {
                        $sigStatus = $sig.Status.ToString()
                        if ($sig.Status -eq 'Valid' -and $sig.SignerCertificate) {
                            $signer = $sig.SignerCertificate.Subject
                        }
                        if ($sigStatus -in @('UnknownError', 'NotSupportedFileFormat', 'Incompatible') -or
                            ($sigStatus -eq 'Valid' -and [string]::IsNullOrWhiteSpace([string]$signer))) { $partial = $true }
                    } else {
                        $partial = $true
                    }
                }
                catch {
                    $sigStatus = "UnknownError"
                    $partial = $true
                }
            } elseif ($path) { $partial = $true }
            try { $elevated = Get-CleanupProcessElevation -ProcessId $processId }
            catch { $elevated = 'Unknown'; $partial = $true }
            if ($elevated -notin @('Yes', 'No')) { $partial = $true }
            $entries.Add(@{
                name     = $processName
                pid      = $processId
                path     = if ($path) { $path } else { $null }
                pathStatus = if ($path) { 'available' } else { 'unavailable' }
                signed   = $sigStatus
                signer   = $signer
                elevated = $elevated
            })
            } catch { $partial = $true }
        }
        @{ processes = @($entries.ToArray()); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some processes could not be inspected.' } else { $null } }
    }
    catch {
        @{ error = $true; message = "Failed to get process intelligence: $($_.Exception.Message)"; processes = @() }
    }
}

function Get-ExecutionCache {
    $entries = @()
    $seen = @{}
    $partial = $false
    try {
        $storePath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store'
        $persistPath = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted'
        $userAssistRoot = 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\UserAssist'
        $muiCachePath = 'HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache'
        $recentAppsPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps'

        $addEntry = {
            param([string]$Path, [string]$Source)
            if ([string]::IsNullOrWhiteSpace($Path)) { return }
            if (-not $seen.ContainsKey($Path)) {
                $seen[$Path] = $true
                $entries += @{
                    path   = $Path
                    source = $Source
                }
            }
        }

        foreach ($path in @($storePath, $persistPath)) {
            try {
            if (Test-Path -LiteralPath $path -ErrorAction Stop) {
                # Add logic to handle different registry structures if needed
                $props = Get-ItemProperty -LiteralPath $path -ErrorAction Stop
                if ($props) {
                    $props.PSObject.Properties | ForEach-Object {
                        if ($_.Name -notin @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider')) {
                            . $addEntry $_.Name (Split-Path -Leaf $path)
                        }
                    }
                }
            }
            } catch { $partial = $true }
        }

        # BAM omitted: kernel-protected, cannot be cleared; only clearable sources shown so Clear leaves list empty
        # if (Test-Path $bamRoot) { ... }

        try {
        if (Test-Path -LiteralPath $userAssistRoot -ErrorAction Stop) {
            Get-ChildItem -LiteralPath $userAssistRoot -ErrorAction Stop | ForEach-Object {
                try {
                $countPath = Join-Path $_.PSPath 'Count'
                if (Test-Path -LiteralPath $countPath -ErrorAction Stop) {
                    $props = Get-ItemProperty -LiteralPath $countPath -ErrorAction Stop | Select-Object -Property *
                    $props.PSObject.Properties | ForEach-Object {
                        if ($_.Name -notmatch '^PS(.*)') {
                            $decoded = ($_.Name.ToCharArray() | ForEach-Object {
                                    $c = [int][char]$_
                                    if ($c -ge 65 -and $c -le 90) { [char](((($c - 65) + 13) % 26) + 65) }
                                    elseif ($c -ge 97 -and $c -le 122) { [char](((($c - 97) + 13) % 26) + 97) }
                                    else { [char]$c }
                                }) -join ''
                            $decoded = $decoded -replace '^\\\?\\', ''
                            . $addEntry $decoded 'UserAssist'
                        }
                    }
                }
                } catch { $partial = $true }
            }
        }
        } catch { $partial = $true }

        try {
        if (Test-Path -LiteralPath $muiCachePath -ErrorAction Stop) {
            $props = Get-ItemProperty -LiteralPath $muiCachePath -ErrorAction Stop | Select-Object -Property *
            $props.PSObject.Properties | ForEach-Object {
                if ($_.Name -notmatch '^PS(.*)' -and $_.Name -ne 'LangID') {
                    . $addEntry $_.Name 'MuiCache'
                }
            }
        }
        } catch { $partial = $true }

        try {
        if (Test-Path -LiteralPath $recentAppsPath -ErrorAction Stop) {
            Get-ChildItem -LiteralPath $recentAppsPath -ErrorAction Stop | ForEach-Object {
                try {
                $appPath = $_.GetValue('AppPath')
                if ($appPath) { . $addEntry $appPath 'RecentApps' }
                } catch { $partial = $true }
            }
        }
        } catch { $partial = $true }
        
        # Check ShimCache (AppCompatCache) existence (just counting as one entry if present, or listing size)
        $shimCachePath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\AppCompatCache'
        try {
        if (Test-Path -LiteralPath $shimCachePath -ErrorAction Stop) {
            $shimData = Get-ItemProperty -LiteralPath $shimCachePath -ErrorAction Stop
            if ($shimData -and $shimData.AppCompatCache) {
                . $addEntry "ShimCache (Binary Blob - $($shimData.AppCompatCache.Length) bytes)" "ShimCache"
            }
        }
        } catch { $partial = $true }

        @{ entries = $entries; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some execution history sources could not be checked.' } else { $null } }
    }
    catch {
        @{ error = $true; message = "Failed to read execution cache: $($_.Exception.Message)" }
    }
}

# --- Privacy Protection Logic ---

function Get-PrivacyProtectionStatus {
    try {
        $memPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management'
        $explorerPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
        $prefetchPath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
        
        $pagefile = (Get-ItemProperty -Path $memPath -Name 'PagingFiles' -ErrorAction SilentlyContinue).PagingFiles
        $pagefileDisabled = [string]::IsNullOrWhiteSpace($pagefile)
        
        $prefetch = (Get-ItemProperty -Path $prefetchPath -Name 'EnablePrefetcher' -ErrorAction SilentlyContinue).EnablePrefetcher
        $prefetchDisabled = ($prefetch -eq 0)
        
        $trackDocs = (Get-ItemProperty -Path $explorerPath -Name 'Start_TrackDocs' -ErrorAction SilentlyContinue).Start_TrackDocs
        $recentFilesDisabled = ($trackDocs -eq 0)
        
        $trackProgs = (Get-ItemProperty -Path $explorerPath -Name 'Start_TrackProgs' -ErrorAction SilentlyContinue).Start_TrackProgs
        $jumpListsDisabled = ($trackProgs -eq 0)
        
        $thumbCache = (Get-ItemProperty -Path $explorerPath -Name 'DisableThumbnailCache' -ErrorAction SilentlyContinue).DisableThumbnailCache
        $thumbCacheDisabled = ($thumbCache -eq 1)
        
        @{
            enabled        = (@($pagefileDisabled, $prefetchDisabled, $recentFilesDisabled, $jumpListsDisabled, $thumbCacheDisabled) | Where-Object { $_ } | Measure-Object).Count -ge 3
            pagefile       = $pagefileDisabled
            prefetch       = $prefetchDisabled
            recentFiles    = $recentFilesDisabled
            jumpLists      = $jumpListsDisabled
            thumbnailCache = $thumbCacheDisabled
        }
    }
    catch {
        @{ enabled = $false; error = $_.Exception.Message }
    }
}

function Enable-PrivacyProtection {
    try {
        Disable-Pagefile | Out-Null
        Disable-Prefetch | Out-Null
        Disable-RecentFilesTracking | Out-Null
        Disable-JumpLists | Out-Null
        Disable-ThumbnailCache | Out-Null
        Get-PrivacyProtectionStatus
    }
    catch {
        @{ error = $true; message = "Failed to enable privacy protection: $($_.Exception.Message)" }
    }
}

function Disable-PrivacyProtection {
    try {
        Enable-Pagefile | Out-Null
        Enable-Prefetch | Out-Null
        Enable-RecentFilesTracking | Out-Null
        Enable-JumpLists | Out-Null
        Enable-ThumbnailCache | Out-Null
        Get-PrivacyProtectionStatus
    }
    catch {
        @{ error = $true; message = "Failed to disable privacy protection: $($_.Exception.Message)" }
    }
}

function Disable-Pagefile {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name 'PagingFiles' -Value '' -Type MultiString -Force
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name 'ClearPageFileAtShutdown' -Value 1 -Type DWord -Force
    return @{ status = 'disabled' }
}

function Enable-Pagefile {
    Assert-IsAdmin
    Set-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' -Name 'PagingFiles' -Value '?:\pagefile.sys' -Type MultiString -Force
    return @{ status = 'enabled' }
}

function Disable-Prefetch {
    Assert-IsAdmin
    $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'EnablePrefetcher' -Value 0 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'EnableSuperfetch' -Value 0 -Type DWord -Force -ErrorAction SilentlyContinue
    return @{ status = 'disabled' }
}

function Enable-Prefetch {
    Assert-IsAdmin
    $path = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters'
    Set-ItemProperty -Path $path -Name 'EnablePrefetcher' -Value 3 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'EnableSuperfetch' -Value 3 -Type DWord -Force -ErrorAction SilentlyContinue
    return @{ status = 'enabled' }
}

function Disable-RecentFilesTracking {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackDocs' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}

function Enable-RecentFilesTracking {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackDocs' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}

function Disable-JumpLists {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackProgs' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}

function Enable-JumpLists {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name 'Start_TrackProgs' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}

# ────────────────────────────────────────────────────────────────────
# Phase E hide-recent toggles
# ────────────────────────────────────────────────────────────────────
# All HKCU writes — no admin required. Each toggle's "disabled" sense
# means "history surface is hidden / not recorded". Convention:
#   Disable-X → set the registry value that hides the history
#   Enable-X  → restore default (history visible / recorded)

# --- Quick Access "Recent files" group in File Explorer Home view
function Disable-QuickAccessRecent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowRecent' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}
function Enable-QuickAccessRecent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowRecent' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}
function Get-QuickAccessRecentStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowRecent' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.ShowRecent -eq 0) }
}

# --- Quick Access "Frequent folders" group in File Explorer Home view
function Disable-QuickAccessFrequent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowFrequent' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}
function Enable-QuickAccessFrequent {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowFrequent' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}
function Get-QuickAccessFrequentStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer' -Name 'ShowFrequent' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.ShowFrequent -eq 0) }
}

# --- Win+R "Run" dialog history (the dropdown of recently typed cmds)
function Disable-RunMRU {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer'
    if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'NoRunMRU' -Value 1 -Type DWord -Force
    # Also erase the existing RunMRU keys so historical entries vanish.
    Remove-Item -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU' -Recurse -Force -ErrorAction SilentlyContinue
    return @{ status = 'disabled' }
}
function Enable-RunMRU {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer'
    Remove-ItemProperty -Path $path -Name 'NoRunMRU' -ErrorAction SilentlyContinue
    return @{ status = 'enabled' }
}
function Get-RunMRUStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer' -Name 'NoRunMRU' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.NoRunMRU -eq 1) }
}

# --- Windows Search box history ("recent searches")
function Disable-SearchHistory {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SearchSettings'
    if (-not (Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
    Set-ItemProperty -Path $path -Name 'IsDeviceSearchHistoryEnabled' -Value 0 -Type DWord -Force
    return @{ status = 'disabled' }
}
function Enable-SearchHistory {
    Set-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SearchSettings' -Name 'IsDeviceSearchHistoryEnabled' -Value 1 -Type DWord -Force
    return @{ status = 'enabled' }
}
function Get-SearchHistoryStatus {
    $v = Get-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\SearchSettings' -Name 'IsDeviceSearchHistoryEnabled' -ErrorAction SilentlyContinue
    return @{ disabled = ($v.IsDeviceSearchHistoryEnabled -eq 0) }
}

# --- PowerShell terminal command history
# Windows Terminal does not retain command history itself; PSReadLine does. A
# profile block covers both Windows PowerShell and PowerShell 7 while leaving
# every user-owned profile line intact when the control is later turned off.
$script:WcTerminalHistoryStart = '# >>> WinCommander terminal history >>>'
$script:WcTerminalHistoryEnd = '# <<< WinCommander terminal history <<<'

function Get-WcTerminalHistoryProfiles {
    $documents = [Environment]::GetFolderPath('MyDocuments')
    @(
        (Join-Path $documents 'WindowsPowerShell\profile.ps1'),
        (Join-Path $documents 'PowerShell\profile.ps1')
    )
}

function Get-WcTerminalHistoryBlock {
    @"
$script:WcTerminalHistoryStart
if (Get-Module -ListAvailable -Name PSReadLine) {
    Import-Module PSReadLine -ErrorAction SilentlyContinue
    Set-PSReadLineOption -HistorySaveStyle SaveNothing -ErrorAction SilentlyContinue
}
$script:WcTerminalHistoryEnd
"@.Trim()
}

function Test-WcTerminalHistoryProfile {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    try {
        $content = [System.IO.File]::ReadAllText($Path)
        return $content.Contains($script:WcTerminalHistoryStart) -and $content.Contains($script:WcTerminalHistoryEnd)
    }
    catch { return $false }
}

function Disable-TerminalHistory {
    try {
        $block = Get-WcTerminalHistoryBlock
        foreach ($profile in Get-WcTerminalHistoryProfiles) {
            $directory = Split-Path -Parent $profile
            if (-not (Test-Path -LiteralPath $directory)) {
                [System.IO.Directory]::CreateDirectory($directory) | Out-Null
            }
            if (Test-WcTerminalHistoryProfile -Path $profile) { continue }

            $existing = if (Test-Path -LiteralPath $profile -PathType Leaf) {
                [System.IO.File]::ReadAllText($profile)
            } else { '' }
            $separator = if ([string]::IsNullOrWhiteSpace($existing)) { '' } else { [Environment]::NewLine + [Environment]::NewLine }
            [System.IO.File]::WriteAllText($profile, $existing + $separator + $block + [Environment]::NewLine)
        }
        @{ status = 'disabled' }
    }
    catch {
        @{ error = $true; message = "Failed to disable terminal history: $($_.Exception.Message)" }
    }
}

function Enable-TerminalHistory {
    try {
        $pattern = '(?ms)^# >>> WinCommander terminal history >>>\r?\n.*?^# <<< WinCommander terminal history <<<\r?\n?'
        foreach ($profile in Get-WcTerminalHistoryProfiles) {
            if (-not (Test-Path -LiteralPath $profile -PathType Leaf)) { continue }
            $existing = [System.IO.File]::ReadAllText($profile)
            $updated = [regex]::Replace($existing, $pattern, '')
            if ($updated -eq $existing) { continue }
            if ([string]::IsNullOrWhiteSpace($updated)) {
                Remove-Item -LiteralPath $profile -Force
            } else {
                [System.IO.File]::WriteAllText($profile, $updated.TrimEnd() + [Environment]::NewLine)
            }
        }
        @{ status = 'enabled' }
    }
    catch {
        @{ error = $true; message = "Failed to enable terminal history: $($_.Exception.Message)" }
    }
}

function Get-TerminalHistoryStatus {
    $profiles = @(Get-WcTerminalHistoryProfiles)
    $protected = @($profiles | Where-Object { Test-WcTerminalHistoryProfile -Path $_ })
    @{ disabled = ($protected.Count -eq $profiles.Count); configuredProfiles = $protected.Count; totalProfiles = $profiles.Count }
}

function Disable-ThumbnailCache {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    Set-ItemProperty -Path $path -Name 'DisableThumbnailCache' -Value 1 -Type DWord -Force
    Set-ItemProperty -Path $path -Name 'DisableThumbsDBOnNetworkFolders' -Value 1 -Type DWord -Force
    # Best-effort shred of any existing thumbnail-cache DBs. Expand the wildcard
    # HERE: Invoke-7Erase resolves with -LiteralPath (so it never expands globs),
    # and the files may not exist at all (fresh profile / already cleared). A
    # missing cache is NOT an error — the registry keys above are the real
    # disable, so we iterate the matched files and skip cleanly when there are none.
    Get-ChildItem -Path "$env:LOCALAPPDATA\Microsoft\Windows\Explorer\thumbcache_*.db" -Force -ErrorAction SilentlyContinue |
        ForEach-Object { Invoke-7Erase -Path $_.FullName -Type File }
    return @{ status = 'disabled' }
}

function Enable-ThumbnailCache {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
    Set-ItemProperty -Path $path -Name 'DisableThumbnailCache' -Value 0 -Type DWord -Force
    Remove-ItemSecure -Path $path -Name 'DisableThumbsDBOnNetworkFolders' -ErrorAction SilentlyContinue
    return @{ status = 'enabled' }
}

# --- High-Intensity Privacy Clean ---

function Get-ShellBags {
    try {
        $entries = @()
        $seen = @{}
        # Use only primary BagMRU (Local Settings) - the second path often duplicates or is empty
        $mruRoots = @("HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU")
        foreach ($root in $mruRoots) {
            if (-not (Test-Path $root)) { continue }
            $rootItem = Get-Item $root -ErrorAction SilentlyContinue
            if (-not $rootItem) { continue }
            $stack = @(@{ key = $rootItem; path = "" })
            while ($stack.Count -gt 0) {
                $cur = $stack[-1]
                if ($stack.Count -eq 1) { $stack = @() } else { $stack = $stack[0..($stack.Count - 2)] }
                $key = $cur.key
                $relPath = $cur.path
                $fullPath = if ($relPath) { $relPath } else { "(root)" }
                $lastMod = if ($key.LastWriteTime) { $key.LastWriteTime.ToString("o") } else { "" }
                $id = $key.PSPath
                if (-not $seen.ContainsKey($id)) {
                    $seen[$id] = $true
                    $pathHint = $fullPath
                    try {
                        $props = Get-ItemProperty -Path $key.PSPath -ErrorAction SilentlyContinue
                        if ($props -and $props.PSObject.Properties["NodeSlot"]) {
                            $nodeSlot = $props.NodeSlot
                            $bagsPaths = @(
                                "HKCU:\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
                                "HKCU:\Software\Microsoft\Windows\Shell\Bags"
                            )
                            foreach ($bp in $bagsPaths) {
                                if (Test-Path $bp) {
                                    $bagKeys = Get-ChildItem $bp -Recurse -ErrorAction SilentlyContinue | Where-Object { [string]$_.PSChildName -eq [string]$nodeSlot }
                                    foreach ($bk in $bagKeys) {
                                        $itemPos = (Get-ItemProperty -Path $bk.PSPath -Name "ItemPos" -ErrorAction SilentlyContinue).ItemPos
                                        if ($itemPos -and [string]::IsNullOrWhiteSpace([string]$itemPos) -eq $false) {
                                            $pathHint = [string]$itemPos
                                            break
                                        }
                                    }
                                    if ($pathHint -ne $fullPath) { break }
                                }
                            }
                        }
                    }
                    catch {}
                    # Use friendly labels for internal registry structure
                    $displayPath = if ($pathHint -eq "(root)") { "ShellBag root" } elseif ($pathHint -match '^\d+(\\\d+)*$') { "Folder view ($pathHint)" } else { $pathHint }
                    $entries += @{ path = $displayPath; lastModified = $lastMod; source = "ShellBags" }
                }
                Get-ChildItem $key.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                    $childPath = if ($relPath) { "$relPath\$($_.PSChildName)" } else { $_.PSChildName }
                    $stack += @{ key = $_; path = $childPath }
                }
            }
        }
        @{ entries = $entries }
    }
    catch {
        @{ error = $true; message = "Failed to get ShellBags: $($_.Exception.Message)" }
    }
}

function Get-WlanProfiles {
    param(
        # Only the local viewer explicitly requests credentials. Fleet and
        # normal scans pass false and must never collect them.
        [bool]$IncludeSecrets = $false
    )
    try {
        $snapshot = Get-WCWlanSnapshot
        Assert-WCWlanSnapshot $snapshot
        $allProfiles = @($snapshot.Profiles | ForEach-Object {
            $flags = [int64]$_.Flags
            $profile = @{ name = [string]$_.Name; interfaceId = [string]$_.InterfaceId; flags = $flags; profileType = if ($flags -eq 0) { 'All users' } elseif ($flags -band 1) { 'Group policy' } elseif ($flags -band 2) { 'Current user' } else { 'Other' } }
            if ($IncludeSecrets) {
                $profile.password = $null
                $profile.passwordStatus = 'unavailable'
                try {
                    $key = Get-WCWlanKey -InterfaceId $profile.interfaceId -Name $profile.name
                    if ($null -ne $key -and $key -ne '') { $profile.password = [string]$key; $profile.passwordStatus = 'available' }
                } catch { }
            }
            $profile
        })
        $profiles = @($allProfiles | Where-Object { $_.flags -eq 0 })
        $additionalProfiles = @($allProfiles | Where-Object { $_.flags -ne 0 })
        @{ profiles = $profiles; additionalProfiles = $additionalProfiles; total = $profiles.Count; adapterCount = [int]$snapshot.InterfaceCount;
           coverage = 'limited'; coverageNote = 'The cleanup count includes all-user profiles on available Wi-Fi adapters. Current-user and policy profiles are displayed separately and are not removed by this cleanup.' }
    }
    catch {
        @{ error = $true; message = 'Wi-Fi profiles could not be inspected. Check the WLAN service and adapter.' }
    }
}

function Get-BluetoothDevices {
    $debugLog = @()
    $devices = @()
    $seenNames = @{}

    # PHASE 1: Direct Read (User Context)
    $debugLog += "Phase 1: Direct Scan"
    try {
        # Helper to check if a name is a generic system component
        $isSystem = {
            param($n)
            $generic = @("Microsoft Bluetooth", "RFCOMM", "Personal Area Network", "BTHBRB", "BTHLE", "Generic Bluetooth", "Bluetooth LE")
            foreach ($g in $generic) { if ($n -match $g) { return $true } }
            return $false
        }

        # Helper to extract MAC or clean ID
        $cleanId = {
            param($id)
            if ($id -match 'BluetoothDevice_([0-9A-Fa-f]{12})') { return $Matches[1].ToUpper() }
            if ($id -match '_([0-9A-Fa-f]{12})_') { return $Matches[1].ToUpper() }
            if ($id -match '^([0-9A-Fa-f]{12})$') { return $Matches[1].ToUpper() } # Direct MAC
            return $id
        }

        # 1. Check BTHPORT (Best Names for Paired Devices)
        $btPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices'
        if (Test-Path $btPath) {
            Get-ChildItem $btPath -ErrorAction SilentlyContinue | ForEach-Object {
                $id = $_.PSChildName
                $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                $name = if ($props.Name) { try { [System.Text.Encoding]::ASCII.GetString($props.Name).Trim([char]0) } catch { "Unknown Device" } } else { "Unknown Device" }
                
                if (-not (&$isSystem $name) -and -not $seenNames[$name]) {
                    $lastSeen = if ($props.LastSeen) { try { [DateTime]::FromFileTime($props.LastSeen).ToString("yyyy-MM-dd HH:mm:ss") } catch { "Registry Traces" } } else { "Registry Traces" }
                    $devices += @{ id = (&$cleanId $id); name = $name; lastSeen = $lastSeen }
                    $seenNames[$name] = $true
                }
            }
        }

        # 2. Check Enum History (Traces)
        $enumPaths = @('HKLM:\SYSTEM\CurrentControlSet\Enum\BTHENUM', 'HKLM:\SYSTEM\CurrentControlSet\Enum\BTH')
        foreach ($p in $enumPaths) {
            if (Test-Path $p) {
                Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {
                    $devClass = $_
                    Get-ChildItem $devClass.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                        $sub = $_
                        $props = Get-ItemProperty $sub.PSPath -ErrorAction SilentlyContinue
                        $name = $props.FriendlyName
                        if (-not $name) { $name = $props.DeviceDesc }
                        if ($name -match ';') { $name = $name.Split(';')[-1] }
                        if (-not $name -or $name -eq $sub.PSChildName) { return } # Skip if no meaningful name

                        if (-not (&$isSystem $name) -and -not $seenNames[$name]) {
                            $devices += @{ id = (&$cleanId $sub.PSChildName); name = $name; lastSeen = "Registry Trace" }
                            $seenNames[$name] = $true
                        }
                    }
                }
            }
        }
    }
    catch {
        $debugLog += "Phase 1 Error: $($_.Exception.Message)"
    }

    # PHASE 2: SYSTEM Task (If Phase 1 found nothing or for escalation)
    if ($devices.Count -eq 0) {
        $debugLog += "Phase 2: SYSTEM Task Escalation"
        try {
            $scriptContent = @'
$res = @()
$seenN = @{}
$isSys = { param($n); $gen = @("Microsoft Bluetooth", "RFCOMM", "Personal Area Network", "BTHBRB", "BTHLE", "Generic Bluetooth", "Bluetooth LE"); foreach ($g in $gen) { if ($n -match $g) { return $true } }; return $false }
$btPath = 'HKLM:\SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices'
if (Test-Path $btPath) {
    Get-ChildItem $btPath -ErrorAction SilentlyContinue | ForEach-Object {
        $id = $_.PSChildName; $pr = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
        $nm = if ($pr.Name) { try { [System.Text.Encoding]::ASCII.GetString($pr.Name).Trim([char]0) } catch { "Unknown Device" } } else { "Unknown Device" }
        if (-not (&$isSys $nm) -and -not $seenN[$nm]) {
            $ls = if ($pr.LastSeen) { try { [DateTime]::FromFileTime($pr.LastSeen).ToString("yyyy-MM-dd HH:mm:ss") } catch { "Registry Traces" } } else { "Registry Traces" }
            $res += @{ id = $id; name = $nm; lastSeen = $ls }; $seenN[$nm] = $true
        }
    }
}
$enumPaths = @('HKLM:\SYSTEM\CurrentControlSet\Enum\BTHENUM', 'HKLM:\SYSTEM\CurrentControlSet\Enum\BTH')
foreach ($p in $enumPaths) {
    if (Test-Path $p) {
        Get-ChildItem $p -ErrorAction SilentlyContinue | ForEach-Object {
            $c = $_; Get-ChildItem $c.PSPath -ErrorAction SilentlyContinue | ForEach-Object {
                $sub = $_; $pr = Get-ItemProperty $sub.PSPath -ErrorAction SilentlyContinue
                $nm = $pr.FriendlyName; if (-not $nm) { $nm = $pr.DeviceDesc }; if ($nm -match ';') { $nm = $nm.Split(';')[-1] }
                if ($nm -and -not (&$isSys $nm) -and -not $seenN[$nm]) {
                    $res += @{ id = $sub.PSChildName; name = $nm; lastSeen = "Registry Trace" }; $seenN[$nm] = $true
                }
            }
        }
    }
}
$res | ConvertTo-Json -Compress
'@
            $tempScript = "$env:TEMP\System_BtGet.ps1"
            $outputFile = "$env:TEMP\System_BtOut.json"
            $escapedScript = $scriptContent.Replace("'", "''")
            $wrapper = "[scriptblock]::Create('$escapedScript').Invoke() | Out-File -FilePath '$outputFile' -Encoding UTF8 -Force"
            $wrapper | Out-File -FilePath $tempScript -Encoding UTF8 -Force

            $taskName = 'SM-BS'
            Initialize-SystemMaintenanceTaskFolder
            Unregister-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
            $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-ExecutionPolicy Bypass -WindowStyle Hidden -File `"$tempScript`""
            $principal = New-ScheduledTaskPrincipal -UserId "NT AUTHORITY\SYSTEM" -LogonType ServiceAccount -RunLevel Highest
            Register-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
            Start-ScheduledTask -TaskPath $script:SystemMaintenanceTaskPath -TaskName $taskName
            
            $timeout = 10
            while ($timeout -gt 0) {
                if (Test-Path $outputFile) { break }
                Start-Sleep -Seconds 1
                $timeout--
            }

            if (Test-Path $outputFile) {
                $json = Get-Content $outputFile -Raw
                if ($json) {
                    $sysDevices = $json | ConvertFrom-Json
                    if ($sysDevices -is [PSCustomObject]) { $sysDevices = @($sysDevices) }
                    foreach ($d in $sysDevices) {
                        if (-not $seenNames[$d.name]) {
                            $d.id = (&$cleanId $d.id) # Apply cleanId to the ID from the SYSTEM task
                            $devices += $d
                            $seenNames[$d.name] = $true
                        }
                    }
                    $debugLog += "Phase 2 Success: Found $($sysDevices.Count) devices"
                }
                else {
                    $debugLog += "Phase 2 Error: Output file was empty"
                }
                Remove-ItemSecure $outputFile -ErrorAction SilentlyContinue
            }
            else {
                $debugLog += "Phase 2 Error: Task timeout or output file missing"
            }
            Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
            Invoke-7Erase -Path $tempScript -Type File
        }
        catch {
            $debugLog += "Phase 2 Error: $($_.Exception.Message)"
        }
    }

    if ($devices -is [PSCustomObject]) { $devices = @($devices) }
    @{ 
        devices   = $devices
        debugInfo = $debugLog -join "`n"
        error     = if ($devices.Count -eq 0) { $true } else { $false }
        message   = if ($devices.Count -eq 0) { "No user devices found." } else { $null }
    }
}

function Get-NetworkDrives {
    try {
        $drives = Get-PSDrive -PSProvider FileSystem | Where-Object { $null -ne $_.DisplayRoot } | Select-Object Name, DisplayRoot
        @{ drives = $drives }
    }
    catch {
        @{ error = $true; message = "Failed to get network drives: $($_.Exception.Message)" }
    }
}

function Get-RecentFiles {
    try {
        $recentPath = [Environment]::GetFolderPath('Recent')
        $entries = @()
        if (Test-Path $recentPath) {
            Get-ChildItem "$recentPath\*" -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $target = $null
                try {
                    $shell = New-Object -ComObject WScript.Shell
                    $lnk = $shell.CreateShortcut($_.FullName)
                    $target = $lnk.TargetPath
                } catch {}
                $entries += @{
                    name         = $_.BaseName
                    extension    = $_.Extension
                    target       = $target
                    lastModified = $_.LastWriteTime.ToString("o")
                    sizeBytes    = $_.Length
                }
            }
        }
        @{ entries = $entries; total = $entries.Count; path = $recentPath }
    }
    catch {
        @{ error = $true; message = "Failed to get recent files: $($_.Exception.Message)" }
    }
}

function Get-RDPHistory {
    try {
        $entries = @()

        # MRU entries (most recently used hosts)
        $defaultPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Default'
        if (Test-Path $defaultPath) {
            $props = Get-ItemProperty -Path $defaultPath -ErrorAction SilentlyContinue
            if ($props) {
                $mruKeys = $props.PSObject.Properties | Where-Object { $_.Name -match '^MRU\d+$' }
                foreach ($k in $mruKeys) {
                    $entries += @{ type = 'MRU'; host = $k.Value; key = $k.Name }
                }
            }
        }

        # Per-server entries
        $serversPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Servers'
        if (Test-Path $serversPath) {
            Get-ChildItem $serversPath -ErrorAction SilentlyContinue | ForEach-Object {
                $hostname = $_.PSChildName
                $props = Get-ItemProperty -Path $_.PSPath -ErrorAction SilentlyContinue
                $username = if ($props -and $props.UsernameHint) { $props.UsernameHint } else { $null }
                $entries += @{ type = 'Server'; host = $hostname; username = $username; key = $_.PSPath }
            }
        }

        # Saved credentials (cmdkey)
        $cmdkeyOutput = cmdkey /list 2>&1
        foreach ($line in $cmdkeyOutput) {
            if ($line -like "*target=TERMSRV/*") {
                if ($line -match 'target=(\S+)') {
                    $entries += @{ type = 'SavedCredential'; host = $Matches[1]; key = $Matches[1] }
                }
            }
        }

        # Default.rdp file presence
        $docPath = [Environment]::GetFolderPath('MyDocuments')
        $rdpFile = "$docPath\Default.rdp"
        if (Test-Path $rdpFile) {
            $fi = Get-Item $rdpFile -Force -ErrorAction SilentlyContinue
            $entries += @{ type = 'DefaultRDP'; host = 'Default.rdp'; key = $rdpFile; lastModified = $fi.LastWriteTime.ToString("o") }
        }

        @{ entries = $entries; total = $entries.Count }
    }
    catch {
        @{ error = $true; message = "Failed to get RDP history: $($_.Exception.Message)" }
    }
}

function Clear-RDPHistory {
    # Clears all four locations that Windows stores outgoing RDP connection history:
    #   1. Terminal Server Client Default MRU keys (MRU0–MRUn) — the dropdown list in mstsc
    #   2. Per-server subkeys under Servers\ — stores UsernameHint per host
    #   3. Default.rdp in Documents — persists last-used connection settings
    # Saved credentials (TERMSRV/) are a separate store — use Clear-RDPPasswords for those.
    try {
        $cleared = @()

        # 1. MRU entries
        $defaultPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Default'
        if (Test-Path $defaultPath) {
            $props = Get-ItemProperty -Path $defaultPath -ErrorAction SilentlyContinue
            if ($props) {
                $mruKeys = $props.PSObject.Properties |
                    Where-Object { $_.Name -match '^MRU\d+$' } |
                    Select-Object -ExpandProperty Name
                foreach ($k in $mruKeys) {
                    Remove-ItemProperty -Path $defaultPath -Name $k -Force -ErrorAction SilentlyContinue
                    $cleared += "MRU:$k"
                }
            }
        }

        # 2. Per-server subkeys (UsernameHint stored per hostname)
        $serversPath = 'HKCU:\Software\Microsoft\Terminal Server Client\Servers'
        if (Test-Path $serversPath) {
            Get-ChildItem $serversPath -ErrorAction SilentlyContinue | ForEach-Object {
                Remove-Item -Path $_.PSPath -Recurse -Force -ErrorAction SilentlyContinue
                $cleared += "Server:$($_.PSChildName)"
            }
        }

        # 3. Default.rdp file in Documents
        $docPath = [Environment]::GetFolderPath('MyDocuments')
        $rdpFile = "$docPath\Default.rdp"
        if (Test-Path $rdpFile) {
            Remove-Item -Path $rdpFile -Force -ErrorAction SilentlyContinue
            $cleared += "File:Default.rdp"
        }

        @{ success = $true; cleared = $cleared; total = $cleared.Count }
    }
    catch {
        @{ success = $false; error = $_.Exception.Message }
    }
}

function Clear-RDPPasswords {
    # Removes all saved RDP credentials (TERMSRV/* entries) from Windows
    # Credential Manager. Uses cmdkey /delete for each entry found.
    try {
        $removed = @()
        $errors  = @()

        $cmdkeyOutput = cmdkey /list 2>&1
        $targets = @()
        foreach ($line in $cmdkeyOutput) {
            if ($line -like '*target=TERMSRV/*') {
                if ($line -match 'target=(\S+)') { $targets += $Matches[1] }
            }
        }

        foreach ($target in $targets) {
            $result = cmdkey /delete:$target 2>&1
            if ($LASTEXITCODE -eq 0) {
                $removed += $target
            } else {
                $errors += $target
            }
        }

        @{ success = $true; removed = $removed; errors = $errors; total = $removed.Count }
    }
    catch {
        @{ success = $false; error = $_.Exception.Message }
    }
}

function Get-JumpLists {
    try {
        $entries = @()
        $autoPath = "$env:APPDATA\Microsoft\Windows\Recent\AutomaticDestinations"
        $customPath = "$env:APPDATA\Microsoft\Windows\Recent\CustomDestinations"

        if (Test-Path $autoPath) {
            Get-ChildItem $autoPath -ErrorAction SilentlyContinue | ForEach-Object {
                $entries += @{
                    name         = $_.Name
                    type         = 'Automatic'
                    sizeKB       = [math]::Round($_.Length / 1KB, 1)
                    lastModified = $_.LastWriteTime.ToString("o")
                }
            }
        }
        if (Test-Path $customPath) {
            Get-ChildItem $customPath -ErrorAction SilentlyContinue | ForEach-Object {
                $entries += @{
                    name         = $_.Name
                    type         = 'Custom'
                    sizeKB       = [math]::Round($_.Length / 1KB, 1)
                    lastModified = $_.LastWriteTime.ToString("o")
                }
            }
        }

        @{ entries = $entries; total = $entries.Count }
    }
    catch {
        @{ error = $true; message = "Failed to get jump lists: $($_.Exception.Message)" }
    }
}

function Get-CleanupChromiumProfiles {
    param($Browser)
    $root = [string]$Browser.UserDataDir
    $rootItem = Get-Item -LiteralPath $root -Force -ErrorAction Stop
    if ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A browser profile root is a reparse point.' }
    if ($Browser.Name -in @('Opera', 'Opera GX')) { return @($root) }
    @(Get-ChildItem -LiteralPath $root -Directory -Force -ErrorAction Stop | Where-Object {
        $_.Name -eq 'Default' -or $_.Name -match '^Profile [0-9]+$'
    } | ForEach-Object {
        if ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A browser profile is a reparse point.' }
        $_.FullName
    })
}

function Get-CleanupSummaryBrowserInventory {
    param([Parameter(Mandatory)][string]$ProfilePath)
    if (-not $script:BrowserMap) { throw 'Browser inventory is unavailable.' }
    $targetLocal = Join-Path $ProfilePath 'AppData\Local'
    $targetRoaming = Join-Path $ProfilePath 'AppData\Roaming'
    $roots = @(
        @{ source = $env:LOCALAPPDATA; target = $targetLocal },
        @{ source = $env:APPDATA; target = $targetRoaming },
        @{ source = $env:USERPROFILE; target = $ProfilePath }
    ) | Where-Object { $_.source } | Sort-Object { $_.source.Length } -Descending
    $mapPath = {
        param([string]$Path)
        foreach ($root in $roots) {
            $prefix = ([string]$root.source).TrimEnd('\')
            if ($Path -ieq $prefix) { return [string]$root.target }
            if ($Path.StartsWith($prefix + '\', [StringComparison]::OrdinalIgnoreCase)) {
                return (Join-Path $root.target $Path.Substring($prefix.Length + 1))
            }
        }
        return $Path
    }
    foreach ($browser in $script:BrowserMap) {
        $dataDir = & $mapPath ([string]$browser.UserDataDir)
        if (-not $dataDir) { continue }
        foreach ($exe in @($browser.ExePaths)) {
            $path = & $mapPath ([string]$exe)
            if ($path -and (Test-Path -LiteralPath $path -PathType Leaf -ErrorAction Stop)) {
                @{ Name = $browser.Name; Engine = $browser.Engine; UserDataDir = $dataDir }
                break
            }
        }
    }
}

function Get-CleanupBrowserFootprints {
    param([Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Browsers)
    # Uses Get-InstalledBrowsers from the security module (loaded as additional dependency)
    try {
        $chromiumArtifacts = @('History','Cookies','Login Data','Top Sites','Favicons','Web Data','Visited Links','Shortcuts','Cache','Code Cache','IndexedDB','Local Storage','Session Storage','Network','Download Metadata')
        $geckoArtifacts    = @('places.sqlite','cookies.sqlite','logins.json','favicons.sqlite','formhistory.sqlite','cache2','storage','sessionstore.jsonlz4')

        $helper_measurePath = {
            param($p)
            $sz = 0
            try {
                if ((Get-Item $p -Force -ErrorAction Stop).PSIsContainer) {
                    $sz = (Get-ChildItem -LiteralPath $p -Recurse -File -Force -ErrorAction Stop | Measure-Object -Property Length -Sum -ErrorAction Stop).Sum
                } else {
                    $sz = (Get-Item $p -Force -ErrorAction Stop).Length
                }
            } catch { return $null }
            if (-not $sz) { $sz = 0 }
            return $sz
        }

        $installed = @($Browsers)
        $browsers  = @()
        $partial = $false

        foreach ($b in $installed) {
            $udd = $b.UserDataDir
            $artifacts  = @()
            $totalBytes = 0

            # A single inaccessible profile must not discard browser records
            # already collected from this or an earlier browser.
            try {
            if (-not $udd -or -not (Test-Path -LiteralPath $udd -ErrorAction Stop)) { continue }
            if ($b.Engine -eq 'Chromium') {
                foreach ($profileDir in @(Get-CleanupChromiumProfiles $b)) {
                    foreach ($art in $chromiumArtifacts) {
                        $p = Join-Path $profileDir $art
                        if (Test-Path $p) {
                            $sz = & $helper_measurePath $p
                            if ($null -eq $sz) { $partial = $true; $sizeKB = $null } else { $totalBytes += $sz; $sizeKB = [math]::Round($sz / 1KB, 1) }
                            $artifacts += @{ name = $art; sizeKB = $sizeKB; profilePath = $profileDir }
                        }
                    }
                }
            } else {
                # Gecko: each profile is a subdirectory under UserDataDir\Profiles
                # Read profiles.ini to find active profiles, fallback to scanning Profiles dir
                $profileDirs = @()
                $profilesIni = Join-Path $udd 'profiles.ini'
                if (Test-Path $profilesIni) {
                    try { $ini = Get-Content -LiteralPath $profilesIni -ErrorAction Stop }
                    catch { $partial = $true; $ini = @() }
                    $currentPath = $null
                    $isRelative  = $true
                    foreach ($line in $ini) {
                        if ($line -match '^\[Profile') { $currentPath = $null; $isRelative = $true }
                        elseif ($line -match '^Path=(.+)$') { $currentPath = $Matches[1].Trim() }
                        elseif ($line -match '^IsRelative=(\d)') { $isRelative = $Matches[1] -eq '1' }
                        elseif ($line -match '^\[' -and $currentPath) {
                            $resolved = if ($isRelative) { Join-Path $udd ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                            if (Test-Path $resolved) { $profileDirs += $resolved }
                            $currentPath = $null
                        }
                    }
                    # Flush last entry
                    if ($currentPath) {
                        $resolved = if ($isRelative) { Join-Path $udd ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                        if (Test-Path $resolved) { $profileDirs += $resolved }
                    }
                }
                # Fallback: scan Profiles subdir
                if ($profileDirs.Count -eq 0) {
                    $pfDir = Join-Path $udd 'Profiles'
                    if (Test-Path $pfDir) {
                        $profileDirs = @(Get-ChildItem -LiteralPath $pfDir -Directory -ErrorAction Stop | Select-Object -ExpandProperty FullName)
                    }
                }
                if ($profileDirs.Count -eq 0) { continue }

                foreach ($profDir in $profileDirs) {
                    foreach ($art in $geckoArtifacts) {
                        $p = Join-Path $profDir $art
                        if (Test-Path $p) {
                            $sz = & $helper_measurePath $p
                            if ($null -eq $sz) { $partial = $true; $sizeKB = $null } else { $totalBytes += $sz; $sizeKB = [math]::Round($sz / 1KB, 1) }
                            $artifacts += @{ name = $art; sizeKB = $sizeKB; profilePath = $profDir }
                        }
                    }
                }
            }
            } catch { $partial = $true }

            if ($artifacts.Count -gt 0) {
                $browsers += @{
                    browser     = $b.Name
                    profilePath = $udd
                    artifacts   = $artifacts
                    totalSizeKB = [math]::Round($totalBytes / 1KB, 1)
                }
            }
        }

        @{ browsers = $browsers; totalBrowsers = $browsers.Count; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some browser profiles or data sizes could not be checked.' } else { $null } }
    }
    catch {
        @{ error = $true; message = "Failed to get browser footprints: $($_.Exception.Message)" }
    }
}

function Get-BrowserFootprints {
    Get-CleanupBrowserFootprints -Browsers @(Get-InstalledBrowsers)
}

function Clear-BrowserFootprints {
    [CmdletBinding()]
    param(
        [string]$Browser = '',
        [string]$ProfilePath = '',
        [object]$AllowDefaultExcluded = $false
    )
    Assert-CleanupCategoriesAllowed @('browserFootprints') -AllowDefaultExcluded:($AllowDefaultExcluded -is [bool] -and $AllowDefaultExcluded)
    try {
        $chromiumArtifacts = @('History','Cookies','Login Data','Top Sites','Favicons','Web Data','Visited Links','Shortcuts','Cache','Code Cache','IndexedDB','Local Storage','Session Storage','Network','Download Metadata')
        $geckoArtifacts    = @('places.sqlite','cookies.sqlite','logins.json','favicons.sqlite','formhistory.sqlite','cache2','storage','sessionstore.jsonlz4')
        $removed = @()
        $errors  = @()

        $resolveGeckoProfileDirs = {
            param($UserDataDir)
            $profileDirs = @()
            $profilesIni = Join-Path $UserDataDir 'profiles.ini'
            if (Test-Path $profilesIni) {
                $ini = Get-Content $profilesIni -ErrorAction SilentlyContinue
                $currentPath = $null
                $isRelative  = $true
                foreach ($line in $ini) {
                    if ($line -match '^\[Profile') {
                        $currentPath = $null
                        $isRelative = $true
                    }
                    elseif ($line -match '^Path=(.+)$') {
                        $currentPath = $Matches[1].Trim()
                    }
                    elseif ($line -match '^IsRelative=(\d)') {
                        $isRelative = $Matches[1] -eq '1'
                    }
                    elseif ($line -match '^\[' -and $currentPath) {
                        $resolved = if ($isRelative) { Join-Path $UserDataDir ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                        if (Test-Path $resolved) { $profileDirs += $resolved }
                        $currentPath = $null
                    }
                }
                if ($currentPath) {
                    $resolved = if ($isRelative) { Join-Path $UserDataDir ($currentPath -replace '/', '\') } else { $currentPath -replace '/', '\' }
                    if (Test-Path $resolved) { $profileDirs += $resolved }
                }
            }

            if ($profileDirs.Count -eq 0) {
                $pfDir = Join-Path $UserDataDir 'Profiles'
                if (Test-Path $pfDir) {
                    $profileDirs = Get-ChildItem $pfDir -Directory -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName
                }
            }

            return $profileDirs
        }

        $installed = Get-InstalledBrowsers
        if (-not $installed -or @($installed).Count -eq 0) {
            return @{ success = $true; removed = @(); errors = @(); total = 0 }
        }

        $targets = $installed
        if ($Browser) {
            $targets = $targets | Where-Object { $_.Name -ieq $Browser }
        }
        if ($ProfilePath) {
            $targets = $targets | Where-Object { $_.UserDataDir -ieq $ProfilePath }
        }

        foreach ($b in $targets) {
            $udd = $b.UserDataDir
            if (-not $udd -or -not (Test-Path $udd)) { continue }

            if ($b.Engine -eq 'Chromium') {
                foreach ($profileDir in @(Get-CleanupChromiumProfiles $b)) {
                    foreach ($art in $chromiumArtifacts) {
                        $p = Join-Path $profileDir $art
                        if (-not (Test-Path $p)) { continue }
                        try {
                            Remove-ItemSecure -Path $p
                            $removed += "$($b.Name):$art"
                        }
                        catch {
                            $errors += "$($b.Name):$art"
                        }
                    }
                }
            }
            else {
                $profileDirs = & $resolveGeckoProfileDirs $udd
                if ($profileDirs.Count -eq 0) { continue }

                foreach ($profDir in $profileDirs) {
                    foreach ($art in $geckoArtifacts) {
                        $p = Join-Path $profDir $art
                        if (-not (Test-Path $p)) { continue }
                        try {
                            Remove-ItemSecure -Path $p
                            $removed += "$($b.Name):$art"
                        }
                        catch {
                            $errors += "$($b.Name):$art"
                        }
                    }
                }
            }
        }

        @{ success = $true; removed = $removed; errors = $errors; total = $removed.Count }
    }
    catch {
        @{ success = $false; error = $_.Exception.Message }
    }
}

function Get-PrefetchFiles {
    try {
        $prefetchPath = "$env:SystemRoot\Prefetch"
        $entries = @()
        $accessDenied = $false
        if (Test-Path $prefetchPath) {
            $files = $null
            # Attempt 1: native PS listing
            try {
                $files = Get-ChildItem "$prefetchPath\*.pf" -Force -ErrorAction Stop
            } catch [System.UnauthorizedAccessException] {
                $accessDenied = $true
            } catch {}
            # Attempt 2: .NET IO (sometimes bypasses PS provider restrictions)
            if (-not $files -and -not $accessDenied) {
                try {
                    $netFiles = [System.IO.Directory]::GetFiles($prefetchPath, '*.pf')
                    $files = $netFiles | ForEach-Object { try { [System.IO.FileInfo]::new($_) } catch { $null } } | Where-Object { $_ }
                } catch [System.UnauthorizedAccessException] {
                    $accessDenied = $true
                } catch {}
            }
            # Attempt 3: cmd /c dir (last resort)
            if (-not $files -and -not $accessDenied) {
                $cmdOut = cmd /c "dir /b `"$prefetchPath\*.pf`"" 2>&1
                if (($cmdOut -join ' ') -match 'Access is denied|Zugriff verweigert|access.denied') {
                    $accessDenied = $true
                } else {
                    $files = $cmdOut | Where-Object { $_ -match '\.pf$' } | ForEach-Object {
                        [PSCustomObject]@{
                            Name = $_
                            BaseName = [System.IO.Path]::GetFileNameWithoutExtension($_)
                            Length = 0
                            LastWriteTime = [datetime]::MinValue
                            CreationTime  = [datetime]::MinValue
                        }
                    }
                }
            }
            if ($files) {
                $files | Sort-Object LastWriteTime -Descending | ForEach-Object {
                    $exeName = $_.BaseName -replace '-[0-9A-Fa-f]{8}$', ''
                    $entries += @{
                        name         = $exeName
                        fileName     = $_.Name
                        sizeKB       = if ($_.Length -gt 0) { [math]::Round($_.Length / 1KB, 1) } else { 0 }
                        lastRun      = if ($_.LastWriteTime -ne [datetime]::MinValue) { $_.LastWriteTime.ToString("o") } else { $null }
                        created      = if ($_.CreationTime -ne [datetime]::MinValue) { $_.CreationTime.ToString("o") } else { $null }
                    }
                }
            }
        }
        # Check EnablePrefetcher registry value
        $enablePrefetcher = 3  # default = enabled
        try {
            $pfParams = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management\PrefetchParameters' -Name EnablePrefetcher -ErrorAction Stop
            $enablePrefetcher = [int]$pfParams.EnablePrefetcher
        } catch {}
        @{ entries = $entries; total = $entries.Count; path = $prefetchPath; accessDenied = $accessDenied; enablePrefetcher = $enablePrefetcher }
    }
    catch {
        @{ error = $true; message = "Failed to get prefetch files: $($_.Exception.Message)" }
    }
}

function Get-ShadowCopies {
    try {
        $copies = @()
        # Try CIM first (preferred), fall back to WMI
        $shadows = $null
        try { $shadows = Get-CimInstance -ClassName Win32_ShadowCopy -ErrorAction Stop } catch {}
        if (-not $shadows) {
            try { $shadows = Get-WmiObject Win32_ShadowCopy -ErrorAction Stop } catch {}
        }
        # Also try vssadmin to count even if CIM/WMI fails
        if (-not $shadows) {
            $vssOut = & vssadmin list shadows 2>&1
            $vssStr = ($vssOut | ForEach-Object { $_.ToString() }) -join "`n"
            if ($vssStr -match 'Shadow Copy ID') {
                $ids = [regex]::Matches($vssStr, 'Shadow Copy ID:\s*\{([^}]+)\}')
                foreach ($m in $ids) {
                    $copies += @{ id = $m.Groups[1].Value; drive = '?'; deviceObject = ''; created = ''; stateStr = 'Listed'; persistent = $false; clientAccessible = $false; originatingMachine = ''; serviceMachine = '' }
                }
            }
        } else {
            $shadowList = if ($shadows -is [System.Array]) { $shadows } else { @($shadows) }
            foreach ($s in $shadowList) {
                $stateVal = try { [int]$s.State } catch { 0 }
                $copies += @{
                    id           = [string]$s.ID
                    drive        = [string]$s.VolumeName
                    deviceObject = [string]$s.DeviceObject
                    created      = [string]$s.InstallDate
                    originatingMachine = [string]$s.OriginatingMachine
                    serviceMachine     = [string]$s.ServiceMachine
                    clientAccessible   = [bool]$s.ClientAccessible
                    persistent         = [bool]$s.Persistent
                    stateStr           = switch ($stateVal) {
                        1 { 'Prepare' } 2 { 'Processing Prepare' } 3 { 'Prepared' }
                        4 { 'Processing Precopy' } 5 { 'Precopy' } 6 { 'Processing Create' }
                        8 { 'Processing Commit' } 9 { 'Processing Postcommit' }
                        10 { 'Committed' } 12 { 'Deleted' } 13 { 'Count' }
                        default { 'Unknown' }
                    }
                }
            }
        }
        # Check VSS service status
        $vssRunning = $false
        try {
            $vssSvc = Get-Service -Name VSS -ErrorAction Stop
            $vssRunning = ($vssSvc.Status -eq 'Running')
        } catch {}
        @{ copies = $copies; total = $copies.Count; vssRunning = $vssRunning }
    }
    catch {
        @{ error = $true; message = "Failed to get shadow copies: $($_.Exception.Message)" }
    }
}

function Get-NTFSJournals {
    try {
        $journals = @()
        $inactiveCount = 0
        $volumes = @(Get-CimInstance -ClassName Win32_Volume -Filter "FileSystem='NTFS' AND DriveType=3" -ErrorAction Stop |
            Where-Object { $_.DriveLetter } | Sort-Object -Property DriveLetter -Unique)
        foreach ($volume in $volumes) {
            $driveLetter = [string]$volume.DriveLetter
            $raw = & fsutil usn queryjournal $driveLetter 2>&1
            $exitCode = $LASTEXITCODE
            $output = ($raw | ForEach-Object { $_.ToString() }) -join "`n"
            if ($exitCode -eq 1179 -or ($exitCode -ne 0 -and $output -match '(?i)volume change journal is not active')) {
                $inactiveCount++
                continue
            }
            if ($exitCode -ne 0) { throw "Cannot query the NTFS journal on $driveLetter (exit $exitCode)." }
            if ($output -notmatch '(?i)Journal\s*ID\s*[=:]\s*(0x[0-9a-f]+|[0-9a-f]+)') {
                throw "Cannot decode the NTFS journal identity on $driveLetter. The fsutil output format is unsupported."
            }
            $journalId = $Matches[1]
            $maxSize = $null
            if ($output -match '(?i)Maximum\s*Size\s*[=:]\s*(0x[0-9a-f]+|[0-9]+(?:\s*(?:bytes|kb|mb|gb))?)') { $maxSize = $Matches[1] }
            $journals += @{ drive = $driveLetter; journalId = $journalId; maxSize = $maxSize; present = $true }
        }
        @{
            journals = $journals; total = $journals.Count; countUnit = 'active journals'
            scannedVolumeCount = $volumes.Count; inactiveVolumeCount = $inactiveCount
            coverage = 'Fixed NTFS volumes with drive letters. A reset creates a new journal identity; the number of active journals can stay the same.'
        }
    }
    catch { @{ error = $true; message = "Failed to get NTFS journals: $($_.Exception.Message)" } }
}

# ============================================================================
# ADVANCED PRIVACY CLEAN - GROUP I-A: Hardcore Privacy Clean
# ============================================================================

function Get-VirtualMemoryStatus {
    try {
        $hiberStatus = & powercfg /query SCHEME_CURRENT SUB_SLEEP HIBERNATE 2>&1
        $hiberEnabled = $hiberStatus -notmatch 'hibernate.*off'
        try {
            $pfParam = Get-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management' `
                -Name 'ClearPageFileAtShutdown' -ErrorAction Stop
            $clearOnShutdown = [int]$pfParam.ClearPageFileAtShutdown -eq 1
        } catch { $clearOnShutdown = $false }
        $hiberFile = Test-Path "$env:SystemDrive\hiberfil.sys"
        $swapFile = Test-Path "$env:SystemDrive\swapfile.sys"
        @{ hiberEnabled = $hiberEnabled; hiberFileExists = $hiberFile; swapFileExists = $swapFile; clearPageFileAtShutdown = $clearOnShutdown }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# GROUP I-A: STATUS VIEWERS (Get-* companions for each clear function)
# ============================================================================

function Get-AmcacheEntries {
    param([switch]$FleetProjection)
    # Returns Amcache hive file info + live registry entry counts per category
    try {
        $entries = @()
        $hveFile = "$env:SystemRoot\AppCompat\Programs\Amcache.hve"
        # A denied read must fail the scan, not masquerade as an absent hive.
        try {
            $hveItem = Get-Item -LiteralPath $hveFile -ErrorAction Stop
            $hveSize = [math]::Round($hveItem.Length / 1MB, 2)
            $hveExists = $true
        } catch [System.Management.Automation.ItemNotFoundException] {
            $hveSize = 0
            $hveExists = $false
        }

        # An existing hive file is not itself a live registry inventory. If
        # the expected mount is absent, category zeros do not prove it empty.
        $mountAvailable = $true
        try { $null = Get-Item -Path 'HKLM:\AppCompatCache\Amcache' -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $mountAvailable = $false }

        # Categories accessible as live registry (mounted hive)
        $cats = @(
            @{ name = 'InventoryApplication';        path = 'HKLM:\AppCompatCache\Amcache\InventoryApplication' },
            @{ name = 'InventoryApplicationFile';    path = 'HKLM:\AppCompatCache\Amcache\InventoryApplicationFile' },
            @{ name = 'InventoryApplicationShortcut';path = 'HKLM:\AppCompatCache\Amcache\InventoryApplicationShortcut' },
            @{ name = 'InventoryDeviceContainer';    path = 'HKLM:\AppCompatCache\Amcache\InventoryDeviceContainer' },
            @{ name = 'InventoryDriverBinary';       path = 'HKLM:\AppCompatCache\Amcache\InventoryDriverBinary' }
        )
        foreach ($cat in $cats) {
            $count = 0
            $sample = @()
            try {
                $null = Get-Item -Path $cat.path -ErrorAction Stop
                $children = @(Get-ChildItem -Path $cat.path -ErrorAction Stop)
                $count = ($children | Measure-Object).Count
                $sample = $children | ForEach-Object {
                    $child = $_
                    $props = Get-ItemProperty $child.PSPath -ErrorAction Stop
                    $name = @($props.Name, $props.ProductName, $props.BinFileVersion) | Where-Object { $_ -is [string] -and $_.Trim() } | Select-Object -First 1
                    $path = @($props.LowerCaseLongPath, $props.LongPathHash) | Where-Object { $_ -is [string] -and $_.Trim() } | Select-Object -First 1
                    $entry = @{ id = $child.PSChildName; name = if ($name) { $name } else { '' }; path = if ($path) { $path } else { '' } }
                    foreach ($pair in @(@('productName','ProductName'), @('binaryVersion','BinFileVersion'), @('longPath','LowerCaseLongPath'), @('longPathHash','LongPathHash'))) {
                        $value = $props.($pair[1])
                        if ($value -is [string] -and $value.Trim()) { $entry[$pair[0]] = $value }
                    }
                    # Keep every registry value the scan obtained, including
                    # fields that differ between Windows versions.
                    if (-not $FleetProjection) {
                        $registryValues = @{}
                        foreach ($property in $props.PSObject.Properties) {
                            if ($property.Name -notin @('PSPath','PSParentPath','PSChildName','PSDrive','PSProvider')) { $registryValues[$property.Name] = $property.Value }
                        }
                        if ($registryValues.Count) { $entry.registryValues = $registryValues }
                    }
                    $entry
                }
            } catch [System.Management.Automation.ItemNotFoundException] {
                # This registry category is absent on this Windows build.
            }
            $entries += @{ category = $cat.name; count = $count; sample = $sample }
        }
        # Windows PowerShell 5.1 resolves a hashtable's Count as its number
        # of keys. Read the record-count key explicitly instead.
        $totalCount = 0
        foreach ($entry in $entries) { $totalCount += [int]$entry['count'] }
        $partial = $hveExists -and -not $mountAvailable
        @{ entries = $entries; hveFileSizeMb = $hveSize; hveFileExists = $hveExists; total = $totalCount; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'The Amcache file exists, but its registry inventory is not available.' } else { $null } }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RecycleBinInfo {
    # Read-only viewer. Each deleted item in a volume's $Recycle.Bin has a
    # paired metadata file ($I...) recording the ORIGINAL path, deletion
    # time and size; the $R... file holds the data. This parses every $I
    # file across all fixed and removable volumes (and every per-account SID folder we can
    # read) so the operator can see what was deleted without recovering it.
    # Removes nothing — emptying is the paid Clear-RecycleBinMetadata clearer.
    try {
        $items = @()
        $totalSizeKB = 0.0
        $sidNameCache = @{}
        $partial = $false

        $drives = [System.IO.DriveInfo]::GetDrives() |
            Where-Object { $_.DriveType -in @('Fixed','Removable') -and $_.IsReady }
        foreach ($drv in $drives) {
            $binRoot = Join-Path $drv.RootDirectory.FullName '$Recycle.Bin'
            try {
                $rootItem = Get-Item -LiteralPath $binRoot -Force -ErrorAction Stop
                if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Recycle Bin root unavailable.' }
                $sidDirs = @(Get-ChildItem -LiteralPath $binRoot -Directory -Force -ErrorAction Stop)
            } catch [System.Management.Automation.ItemNotFoundException] { continue }
            catch { $partial = $true; continue }
            foreach ($sidDir in $sidDirs) {
                if ($sidDir.Attributes -band [IO.FileAttributes]::ReparsePoint) { $partial = $true; continue }
                $sid = $sidDir.Name
                if (-not $sidNameCache.ContainsKey($sid)) {
                    $resolved = $sid
                    try {
                        $resolved = (New-Object System.Security.Principal.SecurityIdentifier($sid)).Translate(
                            [System.Security.Principal.NTAccount]).Value
                    } catch {}
                    $sidNameCache[$sid] = $resolved
                }
                $account = $sidNameCache[$sid]

                try { $iFiles = @(Get-ChildItem -LiteralPath $sidDir.FullName -Filter '$I*' -File -Force -ErrorAction Stop) }
                catch { $partial = $true; continue }
                foreach ($iFile in $iFiles) {
                    try {
                        if (($iFile.Attributes -band [IO.FileAttributes]::ReparsePoint) -or $iFile.Length -gt 1MB) { throw 'Recycle Bin metadata unavailable.' }
                        $bytes = [System.IO.File]::ReadAllBytes($iFile.FullName)
                        if ($bytes.Length -lt 24) { throw 'Recycle Bin metadata is incomplete.' }
                        $version   = [System.BitConverter]::ToInt64($bytes, 0)
                        if ($version -notin @(1, 2)) { throw 'Recycle Bin metadata has an unknown format.' }
                        $sizeBytes = [System.BitConverter]::ToInt64($bytes, 8)
                        if ($sizeBytes -lt 0) { throw 'Recycle Bin metadata has an invalid size.' }
                        $ft        = [System.BitConverter]::ToInt64($bytes, 16)
                        $deleted   = [System.DateTime]::FromFileTime($ft).ToString('yyyy-MM-dd HH:mm:ss')

                        $origPath = ''
                        if ($version -eq 2) {
                            if ($bytes.Length -lt 28) { throw 'Recycle Bin path metadata is incomplete.' }
                            # Win10+: int32 path length (wchars incl. NUL) at 24, UTF-16 path at 28
                            $pathChars = [System.BitConverter]::ToInt32($bytes, 24)
                            $byteLen   = [Math]::Max(0, ($pathChars - 1) * 2)
                            if ($pathChars -lt 1 -or $bytes.Length -lt (28 + $byteLen)) { throw 'Recycle Bin path metadata is incomplete.' }
                            $origPath = [System.Text.Encoding]::Unicode.GetString($bytes, 28, $byteLen)
                        } else {
                            # Legacy: fixed 520-byte (260 wchar) UTF-16 path at offset 24
                            if ($bytes.Length -lt 544) { throw 'Recycle Bin path metadata is incomplete.' }
                            $avail = [Math]::Min(520, $bytes.Length - 24)
                            if ($avail -gt 0) {
                                $raw = [System.Text.Encoding]::Unicode.GetString($bytes, 24, $avail)
                                $nul = $raw.IndexOf([char]0)
                                $origPath = if ($nul -ge 0) { $raw.Substring(0, $nul) } else { $raw }
                            }
                        }

                        $sizeKB = [Math]::Round($sizeBytes / 1KB, 1)
                        $totalSizeKB += $sizeKB
                        $items += @{
                            originalPath = $origPath
                            deletedTime  = $deleted
                            sizeBytes    = $sizeBytes
                            sizeKB       = $sizeKB
                            account      = $account
                            sid          = $sid
                            drive        = $drv.Name
                        }
                    } catch { $partial = $true }
                }
            }
        }

        @{
            items       = $items
            total       = $items.Count
            totalSizeKB = [Math]::Round($totalSizeKB, 1)
            partial     = $partial
            status      = if ($partial) { 'partial' } else { 'complete' }
            message     = if ($partial) { 'Some Recycle Bin records could not be read.' } else { $null }
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-NTUserTraces {
    # Returns all RunMRU, TypedPaths, TypedURLs, WordWheelQuery values
    $sections = @()
    try {

        # RunMRU
        $runMruPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU'
        $runMru = @()
        $runMruExists = $true
        try { $null = Get-Item -Path $runMruPath -ErrorAction Stop } catch [System.Management.Automation.ItemNotFoundException] { $runMruExists = $false }
        if ($runMruExists) {
            $props = Get-ItemProperty -Path $runMruPath -ErrorAction Stop
            $order = $props.MRUList
            if ($props) {
                $props.PSObject.Properties | Where-Object { $_.Name -notmatch '^(PS|MRUList)' } | ForEach-Object {
                    $runMru += @{ key = $_.Name; value = ($_.Value -replace '\\1$',''); rawValue = $_.Value }
                }
            }
        }
        $sections += @{ name = 'RunMRU'; count = $runMru.Count; entries = $runMru }

        # TypedPaths
        $tpPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths'
        $typedPaths = @()
        $typedPathsExist = $true
        try { $null = Get-Item -Path $tpPath -ErrorAction Stop } catch [System.Management.Automation.ItemNotFoundException] { $typedPathsExist = $false }
        if ($typedPathsExist) {
            Get-ItemProperty -Path $tpPath -ErrorAction Stop | ForEach-Object {
                $_.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                    $typedPaths += @{ key = $_.Name; value = $_.Value }
                }
            }
        }
        $sections += @{ name = 'TypedPaths'; count = $typedPaths.Count; entries = $typedPaths }

        # TypedURLs (IE/Edge legacy)
        $tuPath = 'HKCU:\Software\Microsoft\Internet Explorer\TypedURLs'
        $typedURLs = @()
        $typedUrlsExist = $true
        try { $null = Get-Item -Path $tuPath -ErrorAction Stop } catch [System.Management.Automation.ItemNotFoundException] { $typedUrlsExist = $false }
        if ($typedUrlsExist) {
            Get-ItemProperty -Path $tuPath -ErrorAction Stop | ForEach-Object {
                $_.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                    $typedURLs += @{ key = $_.Name; value = $_.Value }
                }
            }
        }
        $sections += @{ name = 'TypedURLs'; count = $typedURLs.Count; entries = $typedURLs }

        # WordWheelQuery (Start/Explorer search MRU)
        $wwqPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery'
        $wwq = @()
        $searchHistoryExists = $true
        try { $null = Get-Item -Path $wwqPath -ErrorAction Stop } catch [System.Management.Automation.ItemNotFoundException] { $searchHistoryExists = $false }
        if ($searchHistoryExists) {
            Get-ItemProperty -Path $wwqPath -ErrorAction Stop | ForEach-Object {
                $_.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                    $wwq += @{ key = $_.Name; value = $_.Value }
                }
            }
        }
        $sections += @{ name = 'WordWheelQuery'; count = $wwq.Count; entries = $wwq }

        $totalCount = 0
        foreach ($section in $sections) { $totalCount += [int]$section['count'] }
        @{ sections = $sections; total = $totalCount }
    }
    catch { @{ error = $true; message = $_.Exception.Message; sections = $sections } }
}

function Get-NotepadStateFiles {
    try {
        $files = @()
        $packageRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        $packages = @()
        try {
            $rootItem = Get-Item -LiteralPath $packageRoot -Force -ErrorAction Stop
            if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Notepad package coverage is unavailable.' }
            $packages = @(Get-ChildItem -LiteralPath $packageRoot -Filter 'Microsoft.WindowsNotepad_*' -Directory -Force -ErrorAction Stop)
        } catch [System.Management.Automation.ItemNotFoundException] { }
        foreach ($package in $packages) {
            if ($package.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Notepad package coverage is unavailable.' }
            $tabDir = Join-Path $package.FullName 'LocalState\TabState'
            try {
                $tabItem = Get-Item -LiteralPath $tabDir -Force -ErrorAction Stop
                if (-not $tabItem.PSIsContainer -or ($tabItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Notepad tab-state coverage is unavailable.' }
                Get-ChildItem -LiteralPath $tabDir -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                    $files += @{
                        name    = $_.Name
                        path    = $_.FullName
                        sizeKB  = [math]::Round($_.Length / 1KB, 1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    }
                }
            } catch [System.Management.Automation.ItemNotFoundException] { }
        }
        @{ files = $files; total = $files.Count; packageFound = ($packages.Count -gt 0); totalSizeKB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum, 1) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-PCAInfo {
    # Read the same PCA files targeted by Clear-PCADatabase.
    try {
        $pcaDir = "$env:SystemRoot\appcompat\pca"
        $files = @()
        try {
            $null = Get-Item -LiteralPath $pcaDir -ErrorAction Stop
            Get-ChildItem -LiteralPath $pcaDir -File -Force -ErrorAction Stop | ForEach-Object {
                $files += @{
                    name     = $_.Name
                    sizeKB   = [math]::Round($_.Length / 1KB, 1)
                    modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    type     = $_.Extension
                }
            }
        } catch [System.Management.Automation.ItemNotFoundException] {}
        # Check if PcaSvc service is running
        $pcaSvcState = try { (Get-Service PcaSvc -ErrorAction Stop).Status.ToString() } catch { 'Unknown' }
        @{ files = $files; total = $files.Count; totalSizeMB = [math]::Round(($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2); pcaSvcState = $pcaSvcState }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-CrashDumpList {
    # Returns all crash dump files with size, name, and age
    $dumps = [Collections.Generic.List[object]]::new()
    $partial = $false
    $sources = @(
            @{ label = 'Minidump';           path = "$env:SystemRoot\Minidump" },
            @{ label = 'CrashDumps';         path = "$env:LOCALAPPDATA\CrashDumps" },
            @{ label = 'WER ReportArchive';  path = "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportArchive" },
            @{ label = 'WER ReportQueue';    path = "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportQueue" }
        )
    $memDmpPath = "$env:SystemRoot\MEMORY.DMP"
    try {
        try { $f = Get-Item -LiteralPath $memDmpPath -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $f = $null }
        if ($f) {
            if ($f.PSIsContainer) { throw 'Crash dump path is not a file.' }
            $dumps.Add(@{ source = 'FullDump'; name = $f.Name; path = $f.FullName; sizeKB = [math]::Round($f.Length/1KB,1); modified = $f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
        }
    } catch { $partial = $true }
    foreach ($src in $sources) {
        try {
            try { $root = Get-Item -LiteralPath $src.path -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
            if ($root) {
                if (-not $root.PSIsContainer) { throw 'Crash dump location is not a directory.' }
                Get-ChildItem -LiteralPath $src.path -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                    $dumps.Add(@{
                        source   = $src.label
                        name     = $_.Name
                        path     = $_.FullName
                        sizeKB   = [math]::Round($_.Length/1KB,1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    })
                }
            }
        } catch { $partial = $true }
    }
    @{ dumps = @($dumps.ToArray()); total = $dumps.Count; totalSizeMB = [math]::Round(($dumps | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some crash dump locations could not be checked.' } else { $null } }
}

function Get-SQLiteWALList {
    # Returns all .wal/.shm files found in the stated app-data scope.
    # Uses -Depth 5 to avoid extremely deep recursive scans that can time out
    try {
        $found = [Collections.Generic.List[object]]::new()
        $roots = @($env:LOCALAPPDATA, $env:APPDATA)
        foreach ($root in $roots) {
            try { $null = Get-Item -LiteralPath $root -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            Get-ChildItem -Path $root -Include '*.wal','*.shm' -Recurse -Depth 5 -File -Force -ErrorAction Stop |
                ForEach-Object {
                    $found.Add(@{
                        name     = $_.Name
                        path     = $_.FullName
                        sizeKB   = [math]::Round($_.Length/1KB,1)
                        dir      = Split-Path $_.FullName -Parent | Split-Path -Leaf
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    })
                }
        }
        @{
            files = @($found.ToArray()); total = $found.Count
            totalSizeMB = [math]::Round(($found | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2)
            coverage = 'limited'; coverageNote = 'Scans two app-data roots to depth 5.'
            partial = $false; truncated = $false
            message = 'This count covers the stated depth-limited scope.'
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-RecallDatabaseInfo {
    # Returns Recall/Timeline/notification DB file listing
    try {
        $dbs = [Collections.Generic.List[object]]::new()
        $sources = @(
            @{ label = 'Recall';    path = "$env:LOCALAPPDATA\CoreAIPlatform.00" },
            @{ label = 'Recall';    path = "$env:LOCALAPPDATA\Microsoft\Windows\Recall" },
            @{ label = 'Timeline';  path = "$env:LOCALAPPDATA\ConnectedDevicesPlatform" },
            @{ label = 'ActionCenter'; path = "$env:LOCALAPPDATA\Microsoft\Windows\ActionCenter" }
        )
        foreach ($src in $sources) {
            try { $null = Get-Item -LiteralPath $src.path -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            Get-ChildItem -Path $src.path -Include '*.db','*.db-wal','*.db-shm','*.db-journal','*.edb' -Recurse -File -Force -ErrorAction Stop |
                ForEach-Object {
                    $dbs.Add(@{
                        source   = $src.label
                        name     = $_.Name
                        path     = $_.FullName
                        sizeKB   = [math]::Round($_.Length/1KB,1)
                        modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                    })
                }
        }
        @{
            databases = @($dbs.ToArray()); total = $dbs.Count
            totalSizeMB = [math]::Round(($dbs | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum / 1KB, 2)
            coverage = 'limited'; coverageNote = 'Scans four specified Recall, Timeline and notification locations.'
            partial = $false; truncated = $false
            message = 'This count covers the stated locations only.'
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-SearchIndexInfo {
    try {
        $svcState = try { [string](Get-Service -Name WSearch -ErrorAction Stop).Status } catch { 'Unknown' }
        $indexRoot = "$env:ProgramData\Microsoft\Search\Data\Applications\Windows"
        $files = @()
        $root = $null
        try { $root = Get-Item -LiteralPath $indexRoot -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        $targets = @()
        if ($null -ne $root) {
            $edb = $null
            try { $edb = Get-Item -LiteralPath (Join-Path $indexRoot 'Windows.edb') -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { }
            if ($null -ne $edb) { $targets += $edb }
            $targets += @(Get-ChildItem -LiteralPath $indexRoot -File -Filter 'Windows*.db*' -Force -ErrorAction Stop)
            foreach ($sub in @('Projects', 'GatherLogs')) {
                $subPath = Join-Path $indexRoot $sub
                $subRoot = $null
                try { $subRoot = Get-Item -LiteralPath $subPath -Force -ErrorAction Stop }
                catch [System.Management.Automation.ItemNotFoundException] { }
                if ($null -ne $subRoot) {
                    $targets += @(Get-ChildItem -LiteralPath $subPath -Recurse -File -Force -ErrorAction Stop)
                }
            }
        }
        foreach ($file in $targets) {
            $files += @{
                name = $file.Name; path = $file.FullName
                sizeKB = [math]::Round($file.Length / 1KB, 1)
                modified = $file.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
            }
        }
        @{
            files = $files; total = $files.Count; countUnit = 'files'
            totalSizeMB = [math]::Round(($targets | Measure-Object -Property Length -Sum).Sum / 1MB, 2)
            wsearchState = $svcState
            coverage = 'Windows.edb, Windows*.db* and files in Projects and GatherLogs. Other Search data and Temp are outside this cleanup target.'
        }
    }
    catch { @{ error = $true; message = "Failed to read Search index files: $($_.Exception.Message)" } }
}

function Get-PrintSpoolerInfo {
    # Read the same spool files targeted by Clear-PrintSpooler.
    try {
        $svcState = try { (Get-Service -Name Spooler -ErrorAction Stop).Status } catch { 'Unknown' }
        $files = @()
        $sources = @(
            @{ label = 'Spool Queue'; path = "$env:SystemRoot\System32\spool\PRINTERS"; filter = '*' },
            @{ label = 'User Spool'; path = "$env:LOCALAPPDATA\Microsoft\Windows\Spool"; filter = '*.spl' }
        )
        $totalSizeKB = 0
        foreach ($src in $sources) {
            try {
                $null = Get-Item -LiteralPath $src.path -ErrorAction Stop
                Get-ChildItem -LiteralPath $src.path -File -Filter $src.filter -Force -ErrorAction Stop |
                    ForEach-Object {
                        $kb = [math]::Round($_.Length/1KB, 1)
                        $totalSizeKB += $kb
                        $files += @{
                            source   = $src.label
                            name     = $_.Name
                            sizeKB   = $kb
                            modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss')
                        }
                    }
            } catch [System.Management.Automation.ItemNotFoundException] {}
        }
        @{
            files         = $files
            total         = $files.Count
            totalSizeMB   = [math]::Round($totalSizeKB / 1KB, 2)
            spoolerState  = [string]$svcState
        }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

function Get-WSLDataInfo {
    # Store WSL distributions retain their virtual disks below the package LocalState directory.
    # Registered distro names are included without opening the distributions or their disks.
    try {
        $files = @()
        $totalBytes = [int64]0
        $partial = $false
        $registeredDistros = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
        $distrosWithDisk = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
        $seenVhdx = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
        $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
        try {
        if (Test-Path $packagesRoot -ErrorAction Stop) {
            Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction Stop | ForEach-Object {
                try {
                $vhdx = Join-Path $_.FullName 'LocalState\ext4.vhdx'
                if (Test-Path $vhdx -PathType Leaf -ErrorAction Stop) {
                    $item = Get-Item -LiteralPath $vhdx -Force -ErrorAction Stop
                    if ($item -and $seenVhdx.Add($item.FullName)) {
                        $files += @{ name = "Store WSL: $($_.Name)\\LocalState\\ext4.vhdx"; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        $totalBytes += $item.Length
                    }
                }
                } catch { $partial = $true }
            }
        }
        } catch { $partial = $true }
        # Imported and Microsoft Store distributions use different locations.
        # The former is recorded in Lxss\<GUID>\BasePath, so only walking the
        # Store package folder made a real distribution look like a zero-size
        # registration in the cleanup card.
        $lxssRoot = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss'
        try {
        if (Test-Path $lxssRoot -ErrorAction Stop) {
            Get-ChildItem -Path $lxssRoot -ErrorAction Stop | ForEach-Object {
                try {
                $distro = Get-ItemProperty -Path $_.PSPath -ErrorAction Stop
                $name = ([string]$distro.DistributionName).Trim()
                $basePath = ([string]$distro.BasePath).Trim()
                if ($basePath.StartsWith('\\?\')) { $basePath = $basePath.Substring(4) }
                if (-not $basePath) { return }
                $vhdx = Join-Path $basePath 'ext4.vhdx'
                if (-not (Test-Path $vhdx -PathType Leaf -ErrorAction Stop)) { return }
                $item = Get-Item -LiteralPath $vhdx -Force -ErrorAction Stop
                if (-not $item) { return }
                if ($name) {
                    $null = $registeredDistros.Add($name)
                    $null = $distrosWithDisk.Add($name)
                }
                if ($seenVhdx.Add($item.FullName)) {
                    $label = if ($name) { "WSL: $name\\ext4.vhdx" } else { "WSL: $($_.PSChildName)\\ext4.vhdx" }
                    $files += @{ name = $label; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                    $totalBytes += $item.Length
                }
                } catch { $partial = $true }
            }
        }
        } catch { $partial = $true }
        $wsl = Get-Command wsl.exe -ErrorAction SilentlyContinue
        if ($wsl) {
            try {
            # `--list --quiet` reports imported distros as well as Store-backed
            # ones. They can live outside LocalAppData, so treating the VHDX
            # walk as the only source incorrectly reports a real WSL setup as
            # empty. This query is read-only and does not start a distro.
            $quietResult = @(& $wsl.Source -l -q 2>$null)
            if ($LASTEXITCODE -ne 0) { $partial = $true }
            foreach ($distro in $quietResult) {
                # wsl.exe can emit UTF-16LE through the legacy PowerShell host;
                # strip its NUL separators before comparing it to Lxss names.
                $name = ([string]$distro).Replace([string][char]0, '').Trim()
                if ($name -and $registeredDistros.Add($name) -and -not $distrosWithDisk.Contains($name)) {
                    $files += @{ name = "Registered WSL distro: $name"; sizeKB = 0; modified = '' }
                }
            }
            # Some older WSL builds don't support the quiet listing reliably
            # in a non-interactive PowerShell host. Parse the verbose listing
            # as a read-only fallback so the card does not show a false zero.
            if ($registeredDistros.Count -eq 0) {
                $verboseResult = @(& $wsl.Source -l -v 2>$null)
                if ($LASTEXITCODE -ne 0) { $partial = $true }
                foreach ($line in $verboseResult) {
                    $name = ([string]$line).Replace([string][char]0, '').Trim() -replace '^\*\s*', '' -replace '\s{2,}.*$', ''
                    if ($name -and $name -notmatch '^(NAME|There are no installed distributions)') {
                        if ($registeredDistros.Add($name) -and -not $distrosWithDisk.Contains($name)) {
                            $files += @{ name = "Registered WSL distro: $name"; sizeKB = 0; modified = '' }
                        }
                    }
                }
            }
            } catch { $partial = $true }
        } else { $partial = $true }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some WSL sources could not be checked.' } else { $null } }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-DockerDesktopDataInfo {
    # Docker documents these default paths; customized disk locations require separate verification.
        $files = @()
        $totalBytes = [int64]0
        $partial = $false
        $dockerWsl = Join-Path $env:LOCALAPPDATA 'Docker\wsl'
        $root = $null
        try {
        try { $root = Get-Item -LiteralPath $dockerWsl -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        if ($root) {
            if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Docker WSL disk inventory is unavailable.' }
            Get-ChildItem -LiteralPath $dockerWsl -Recurse -Force -ErrorAction Stop | ForEach-Object {
                $item = $_
                if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Docker WSL disk inventory contains a reparse point.' }
                if ($item.PSIsContainer -or $item.Name -notlike '*.vhdx') { return }
                $files += @{ name = "Docker WSL: $($item.Name)"; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $item.Length
            }
        }
        } catch { $partial = $true }
        foreach ($disk in @(
            @{ name = 'Docker Hyper-V disk'; path = (Join-Path $env:ProgramData 'DockerDesktop\vm-data\DockerDesktop.vhdx') },
            @{ name = 'Docker VMM disk'; path = (Join-Path $env:LOCALAPPDATA 'Docker\vm-data\DockerDesktop.vhdx') }
        )) {
            try {
            $item = $null
            try { $item = Get-Item -LiteralPath $disk.path -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { }
            if (-not $item) { continue }
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Docker disk inventory is unavailable.' }
            $files += @{ name = $disk.name; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
            $totalBytes += $item.Length
            } catch { $partial = $true }
        }
        $dockerLog = Join-Path $env:APPDATA 'Docker\log.txt'
        $item = $null
        try {
        try { $item = Get-Item -LiteralPath $dockerLog -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        if ($item) {
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Docker log inventory is unavailable.' }
            $files += @{ name = 'Docker: log.txt'; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
            $totalBytes += $item.Length
        }
        } catch { $partial = $true }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); coverage = 'limited'; coverageNote = 'Checks documented default Docker disk and log locations. Customized disk locations are not included.'; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some Docker locations could not be checked.' } else { $null } }
}

function Get-VirtualMachineArtifactsInfo {
    # Inventory each source independently so an inaccessible directory does not
    # erase records already found or turn a partial scan into a false zero.
    $files = [System.Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    $vmwareRoot = Join-Path $env:USERPROFILE 'Documents\Virtual Machines'
    $virtualBoxRoot = Join-Path $env:USERPROFILE 'VirtualBox VMs'
    foreach ($source in @(
        @{ root=$vmwareRoot; patterns=@('*-0000*.vmdk','*.vmsn','vmware*.log'); label='VMware' },
        @{ root=$virtualBoxRoot; patterns=@('VBox.log','VBoxSVC.log'); label='VirtualBox log' }
    )) {
        try {
            try { $root = Get-Item -LiteralPath $source.root -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The VM inventory root is unavailable.' }
            foreach ($pattern in $source.patterns) {
                try {
                    Get-ChildItem -LiteralPath $source.root -Recurse -File -Filter $pattern -Force -ErrorAction Stop | ForEach-Object {
                        $files.Add(@{ name = "$($source.label): $($_.FullName.Substring($source.root.Length).TrimStart('\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                        $totalBytes += $_.Length
                    }
                } catch { $partial = $true }
            }
        } catch { $partial = $true }
    }
    try {
        try { $root = Get-Item -LiteralPath $virtualBoxRoot -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The VirtualBox inventory root is unavailable.' }
            Get-ChildItem -LiteralPath $virtualBoxRoot -Directory -Recurse -Force -ErrorAction Stop | Where-Object { $_.Name -eq 'Snapshots' } | ForEach-Object {
                if ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) { $partial = $true; return }
                $snapshotPath = $_.FullName
                foreach ($pattern in @('*.vdi','*.vbox-prev')) {
                    try {
                        Get-ChildItem -LiteralPath $snapshotPath -Recurse -File -Filter $pattern -Force -ErrorAction Stop | ForEach-Object {
                            $files.Add(@{ name = "VirtualBox snapshot: $($_.FullName.Substring($virtualBoxRoot.Length).TrimStart('\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                            $totalBytes += $_.Length
                        }
                    } catch { $partial = $true }
                }
            }
        }
    } catch { $partial = $true }
    foreach ($hostLog in @(
        (Join-Path $env:ProgramData 'VMware\vmnetdhcp.log'),
        (Join-Path $env:ProgramData 'VMware\vmnetnat.log'),
        (Join-Path $env:USERPROFILE '.VirtualBox\VirtualBox.xml')
    )) {
        try {
            try { $item = Get-Item -LiteralPath $hostLog -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'A VM configuration or log is unavailable.' }
            $files.Add(@{ name = "VM configuration/log: $($item.FullName)"; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            $totalBytes += $item.Length
        } catch { $partial = $true }
    }
    try {
        try { $mru = Get-Item -LiteralPath 'HKCU:\Software\VMware, Inc.\VMware Workstation' -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $mru = $null }
        if ($mru) {
            foreach ($property in @($mru.Property | Where-Object { $_ -match '(?i)mru|recent' })) {
                $files.Add(@{ name = "VMware MRU: $property"; sizeKB = 0; modified = '' })
            }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some virtual-machine locations could not be checked.' } else { $null } }
}

function Assert-CleanupDeveloperCacheAncestors {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Base)
    $basePath = [IO.Path]::GetFullPath($Base).TrimEnd('\')
    $fullPath = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    if (-not $fullPath.StartsWith($basePath + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Developer cache path is outside its selected profile.' }
    $parent = [IO.Path]::GetDirectoryName($fullPath)
    while ($parent -and $parent.Length -ge $basePath.Length) {
        $item = Get-Item -LiteralPath $parent -Force -ErrorAction Stop
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Developer cache path contains an unsupported reparse point.' }
        if ($parent -ieq $basePath) { return }
        $parent = [IO.Path]::GetDirectoryName($parent)
    }
    throw 'Developer cache path could not be verified.'
}

function Get-CleanupDeveloperCachesInfo {
    param([string]$AppDataPath, [string]$LocalAppDataPath, [string]$UserProfilePath, [int]$previewLimit)
    # Configuration files are surfaced by presence only; token contents are never read or returned.
    # A developer cache can contain millions of files. Keep complete count/size
    # aggregation while returning only a fixed preview so the IPC payload and
    # trace-detail dialog cannot allocate one UI row per file.
    try {
        $files = [System.Collections.Generic.List[object]]::new()
        $total = [int64]0
        $totalBytes = [int64]0
        $cacheDirs = @(
            @{ label = 'npm cache'; path = (Join-Path $AppDataPath 'npm-cache'); base = $AppDataPath },
            @{ label = 'npm cache'; path = (Join-Path $LocalAppDataPath 'npm-cache'); base = $LocalAppDataPath },
            @{ label = 'pip cache'; path = (Join-Path $LocalAppDataPath 'pip\Cache'); base = $LocalAppDataPath },
            @{ label = 'cargo registry cache'; path = (Join-Path $UserProfilePath '.cargo\registry\cache'); base = $UserProfilePath },
            @{ label = 'cargo registry source'; path = (Join-Path $UserProfilePath '.cargo\registry\src'); base = $UserProfilePath }
        )
        foreach ($cache in $cacheDirs) {
            try { $root = Get-Item -LiteralPath $cache.path -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Developer cache root is not a regular directory.' }
            Assert-CleanupDeveloperCacheAncestors -Path $cache.path -Base $cache.base
            Get-ChildItem -LiteralPath $cache.path -Recurse -Force -ErrorAction Stop | ForEach-Object {
                if ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Developer cache inventory contains a reparse point.' }
                if ($_.PSIsContainer) { return }
                $total++
                if ($files.Count -lt $previewLimit) {
                    [void]$files.Add(@{ name = "$($cache.label): $($_.FullName.Substring($cache.path.Length).TrimStart('\\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                }
                $totalBytes += $_.Length
            }
        }
        foreach ($config in @(
            @{ label = 'npm token configuration present'; path = (Join-Path $UserProfilePath '.npmrc') },
            @{ label = 'cargo configuration present'; path = (Join-Path $UserProfilePath '.cargo\config.toml') },
            @{ label = 'cargo credentials present'; path = (Join-Path $UserProfilePath '.cargo\credentials.toml') }
        )) {
            try { $item = Get-Item -LiteralPath $config.path -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Developer configuration path is not a regular file.' }
            Assert-CleanupDeveloperCacheAncestors -Path $config.path -Base $UserProfilePath
            $total++
            if ($files.Count -lt $previewLimit) {
                [void]$files.Add(@{ name = $config.label; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            }
            $totalBytes += $item.Length
        }
        @{ files = @($files); total = $total; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); previewLimit = $previewLimit; truncated = ($total -gt $files.Count) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0; previewLimit = $previewLimit; truncated = $false } }
}

function Get-DeveloperCachesInfo {
    $previewLimit = 200
    Get-CleanupDeveloperCachesInfo -AppDataPath $env:APPDATA -LocalAppDataPath $env:LOCALAPPDATA -UserProfilePath $env:USERPROFILE -previewLimit $previewLimit
}

function Get-CredentialManagerInfo {
    # cmdkey exposes target names only; it never returns credential secret values here.
    try {
        $files = @()
        $targets = @{}
        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)target=(.+)$') {
                $target = $Matches[1].Trim()
                if ($target) { $targets[$target] = $true }
            }
        }
        foreach ($target in $targets.Keys | Sort-Object) {
            $files += @{ name = "Saved credential target: $target"; sizeKB = 0; modified = '' }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-NetworkWizardHistoryInfo {
    try {
        $files = @()
        $networkWizard = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Network\Network Connection Wizard'
        $exists = $true
        try { $item = Get-Item -LiteralPath $networkWizard -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $exists = $false }
        if ($exists) {
            foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                $files += @{ name = "Network Connection Wizard value: $property"; value = $item.GetValue($property, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); valueType = [string]$item.GetValueKind($property); sizeKB = 0; modified = '' }
            }
            Get-ChildItem -LiteralPath $networkWizard -Force -ErrorAction Stop | ForEach-Object {
                $files += @{ name = "Network Connection Wizard subkey: $($_.PSChildName)"; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-WERHistoryInfo {
    # Return the locally available value and its type alongside its registry name.
    try {
        $files = @()
        foreach ($root in @('HKCU:\Software\Microsoft\Windows\Windows Error Reporting', 'HKLM:\SOFTWARE\Microsoft\Windows\Windows Error Reporting')) {
            foreach ($suffix in @('Consent', 'ExclusionList')) {
                $path = Join-Path $root $suffix
                try { $item = Get-Item -LiteralPath $path -ErrorAction Stop }
                catch [System.Management.Automation.ItemNotFoundException] { continue }
                foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                    $files += @{ name = "$root\\$suffix value: $property"; value = $item.GetValue($property, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); valueType = [string]$item.GetValueKind($property); sizeKB = 0; modified = '' }
                }
                Get-ChildItem -LiteralPath $path -Force -ErrorAction Stop | ForEach-Object {
                    $files += @{ name = "$root\\$suffix subkey: $($_.PSChildName)"; sizeKB = 0; modified = '' }
                }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-InactiveUserProtectionMetadataInfo {
    # DPAPI metadata can reveal which protected material exists; only other users' profiles are inspected.
    try {
        $files = @()
        $totalBytes = [int64]0
        $currentSid = ''
        $currentProfile = [Environment]::GetFolderPath('UserProfile')
        try { $currentSid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}
        if (-not $currentSid -and -not $currentProfile) { throw 'Current profile identity unavailable; inactive profile scan skipped.' }
        $profiles = @(Get-CimInstance -ClassName Win32_UserProfile -ErrorAction SilentlyContinue |
            Where-Object {
                -not $_.Special -and -not $_.Loaded -and $_.SID -match '^S-1-5-21-' -and
                $_.SID -ne $currentSid -and $_.LocalPath -and
                $_.LocalPath.TrimEnd('\\') -ne $currentProfile.TrimEnd('\\')
            })
        foreach ($profile in $profiles) {
            $protectRoot = Join-Path $profile.LocalPath "AppData\Roaming\Microsoft\Protect\$($profile.SID)"
            if (-not (Test-Path $protectRoot -ErrorAction SilentlyContinue)) { continue }
            Get-ChildItem -Path $protectRoot -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $relative = $_.FullName.Substring($protectRoot.Length).TrimStart('\\')
                $files += @{ name = "Inactive user DPAPI metadata: $($profile.SID)\\$relative"; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $_.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-StickyNotesInfo {
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
    try {
        try { $root = Get-Item -LiteralPath $packagesRoot -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'The packages location is not a directory.' }
            Get-ChildItem -LiteralPath $packagesRoot -Directory -Filter 'Microsoft.MicrosoftStickyNotes_*' -Force -ErrorAction Stop | ForEach-Object {
                $package = $_
                foreach ($name in @('plum.sqlite','plum.sqlite-wal','plum.sqlite-shm')) {
                    $path = Join-Path $package.FullName "LocalState\$name"
                    try {
                        try { $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop }
                        catch [System.Management.Automation.ItemNotFoundException] { continue }
                        if ($item.PSIsContainer) { throw 'The Sticky Notes database changed type.' }
                        $files.Add(@{ name = "Sticky Notes: $name"; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                        $totalBytes += $item.Length
                    } catch { $partial = $true }
                }
            }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some Sticky Notes locations could not be checked.' } else { $null } }
}

function Get-OneDriveMetadataInfo {
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    $settingsRoot = Join-Path $env:LOCALAPPDATA 'Microsoft\OneDrive\settings'
    try {
        try { $root = Get-Item -LiteralPath $settingsRoot -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'OneDrive metadata root is not a directory.' }
            $settingsRoot = $root.FullName
            Get-ChildItem -LiteralPath $settingsRoot -Directory -Force -ErrorAction Stop | ForEach-Object {
                $folder = $_
                foreach ($pattern in @('*.dat','*.db')) {
                    try {
                        Get-ChildItem -LiteralPath $folder.FullName -Recurse -File -Filter $pattern -Force -ErrorAction Stop | ForEach-Object {
                            $files.Add(@{ name = "OneDrive metadata: $($_.FullName.Substring($settingsRoot.Length).TrimStart('\\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                            $totalBytes += $_.Length
                        }
                    } catch { $partial = $true }
                }
            }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some OneDrive metadata locations could not be checked.' } else { $null } }
}

function Get-CleanupPackageCacheInventory {
    param([string[]]$PackagePatterns, [string]$RelativeRoot, [string]$Label, [switch]$NameOnly)
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [long]0
    $partial = $false
    $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
    try {
        try { $root = Get-Item -LiteralPath $packagesRoot -Force -ErrorAction Stop }
        catch [Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'Package inventory root is not a directory.' }
            Get-ChildItem -LiteralPath $packagesRoot -Directory -Force -ErrorAction Stop | ForEach-Object {
                $package = $_
                if (-not @($PackagePatterns | Where-Object { $package.Name -like $_ }).Count) { return }
                $cacheRoot = Join-Path $package.FullName $RelativeRoot
                try {
                    try { $cache = Get-Item -LiteralPath $cacheRoot -Force -ErrorAction Stop }
                    catch [Management.Automation.ItemNotFoundException] { return }
                    if (-not $cache.PSIsContainer) { throw 'Package cache root is not a directory.' }
                    Get-ChildItem -LiteralPath $cacheRoot -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                        $name = if ($NameOnly) { $_.Name } else { $_.FullName.Substring($cacheRoot.Length).TrimStart('\\') }
                        $files.Add(@{ name = "${Label}: $name"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                        $totalBytes += $_.Length
                    }
                } catch { $partial = $true }
            }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some package cache locations could not be checked.' } else { $null } }
}

function Get-SpotlightCacheInfo {
    Get-CleanupPackageCacheInventory -PackagePatterns @('Microsoft.Windows.ContentDeliveryManager_*') -RelativeRoot 'LocalState\Assets' -Label 'Spotlight asset' -NameOnly
}

function Get-FontCacheInfo {
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    try {
        $root = Join-Path $env:WINDIR 'ServiceProfiles\LocalService\AppData\Local\FontCache'
        try { $null = Get-Item -LiteralPath $root -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { return @{ files = @(); total = 0; totalSizeMB = 0 } }
        Get-ChildItem -LiteralPath $root -Recurse -File -Filter '*.dat' -Force -ErrorAction Stop | ForEach-Object {
            $files.Add(@{ name = "Font cache: $($_.FullName.Substring($root.Length).TrimStart('\\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            $totalBytes += $_.Length
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some font cache locations could not be checked.' } else { $null } }
}

function Get-LegacyIconCacheInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        $path = Join-Path $env:LOCALAPPDATA 'IconCache.db'
        $item = $null
        try { $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        if ($item) {
            if ($item.PSIsContainer) { throw 'Icon cache path is not a file.' }
            if ($item) {
                $files += @{ name = 'IconCache.db'; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                $totalBytes += $item.Length
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GameCapturesInfo {
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    $roots = @((Join-Path $env:USERPROFILE 'Videos\Captures'))
    $packagesRoot = Join-Path $env:LOCALAPPDATA 'Packages'
    try {
        try { $packages = Get-Item -LiteralPath $packagesRoot -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $packages = $null }
        if ($packages) {
            if (-not $packages.PSIsContainer) { throw 'Package location is not a directory.' }
            Get-ChildItem -LiteralPath $packagesRoot -Directory -Force -ErrorAction Stop |
                Where-Object { $_.Name -like 'Microsoft.XboxGamingOverlay_*' -or $_.Name -like 'Microsoft.GamingApp_*' } |
                ForEach-Object { $roots += Join-Path $_.FullName 'LocalState' }
        }
    } catch { $partial = $true }
    foreach ($root in $roots) {
        try {
            try { $rootItem = Get-Item -LiteralPath $root -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if (-not $rootItem.PSIsContainer) { throw 'Capture location is not a directory.' }
            Get-ChildItem -LiteralPath $root -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ name = "Game capture: $($_.FullName.Substring($root.Length).TrimStart('\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                $totalBytes += $_.Length
            }
        } catch { $partial = $true }
    }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some game capture locations could not be checked.' } else { $null } }
}

function Get-PhotosCacheInfo {
    Get-CleanupPackageCacheInventory -PackagePatterns @('Microsoft.Windows.Photos_*') -RelativeRoot 'LocalState' -Label 'Photos cache'
}

function Get-XboxCacheInfo {
    Get-CleanupPackageCacheInventory -PackagePatterns @('Microsoft.GamingApp_*', 'Microsoft.XboxApp_*') -RelativeRoot 'LocalState' -Label 'Xbox cache'
}

function Get-CleanupArtifactFiles {
    param([Parameter(Mandatory)] [array]$Sources)
    $files = @()
    foreach ($source in $Sources) {
        try { $sourcePath = (Get-Item -LiteralPath $source.path -Force -ErrorAction Stop).FullName }
        catch [System.Management.Automation.ItemNotFoundException] { continue }
        foreach ($pattern in $source.patterns) {
            Get-ChildItem -LiteralPath $sourcePath -Recurse:($source.recurse -ne $false) -File -Filter $pattern -Force -ErrorAction Stop | ForEach-Object {
                $relative = $_.FullName.Substring($sourcePath.Length).TrimStart('\\')
                $files += @{ name = "$($source.label): $relative"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
            }
        }
    }
    @($files)
}

function Get-CommunicationCachesInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'Teams'; path = (Join-Path $env:APPDATA 'Microsoft\\Teams\\Cache'); patterns = @('*') },
            @{ label = 'Teams'; path = (Join-Path $env:APPDATA 'Microsoft\\Teams\\IndexedDB'); patterns = @('*') },
            @{ label = 'Slack'; path = (Join-Path $env:APPDATA 'Slack\\Cache'); patterns = @('*') },
            @{ label = 'Slack'; path = (Join-Path $env:APPDATA 'Slack\\IndexedDB'); patterns = @('*') },
            @{ label = 'Slack'; path = (Join-Path $env:APPDATA 'Slack\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'Discord'; path = (Join-Path $env:APPDATA 'discord\\Cache'); patterns = @('*') },
            @{ label = 'Discord'; path = (Join-Path $env:APPDATA 'discord\\IndexedDB'); patterns = @('*') },
            @{ label = 'Discord'; path = (Join-Path $env:APPDATA 'discord\\Local Storage\\leveldb'); patterns = @('*') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-EditorHistoryInfo {
    try {
        $sources = @(
            @{ label = 'VS Code'; path = (Join-Path $env:APPDATA 'Code\User\globalStorage'); patterns = @('storage.json','state.vscdb'); recurse = $false },
            @{ label = 'VS Code local history'; path = (Join-Path $env:APPDATA 'Code\\User\\History'); patterns = @('*') }
        )
        $jetBrainsRoot = Join-Path $env:APPDATA 'JetBrains'
        if (Test-Path $jetBrainsRoot -ErrorAction SilentlyContinue) {
            Get-ChildItem -Path $jetBrainsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                $sources += @{ label = 'JetBrains recent projects'; path = (Join-Path $_.FullName 'options'); patterns = @('recentProjects.xml') }
                $sources += @{ label = 'JetBrains local history'; path = (Join-Path $env:LOCALAPPDATA ("JetBrains\\{0}\\LocalHistory" -f $_.Name)); patterns = @('*') }
            }
        }
        $files = @(Get-CleanupArtifactFiles -Sources $sources)
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GitActivityInfo {
    try {
        $files = @()
        $targets = @{}
        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)target=(.+)$') {
                $target = $Matches[1].Trim()
                if ($target -match '(?i)git:') { $targets[$target] = $true }
            }
        }
        foreach ($target in $targets.Keys | Sort-Object) { $files += @{ name = "Git credential target: $target"; sizeKB = 0; modified = '' } }
        $gitConfig = Join-Path $env:USERPROFILE '.gitconfig'
        if (Test-Path $gitConfig -PathType Leaf -ErrorAction SilentlyContinue) {
            $safeDirectories = @(& git.exe config --global --get-all safe.directory 2>$null)
            foreach ($directory in $safeDirectories) {
                $files += @{ name = 'Global Git safe.directory'; value = [string]$directory; sizeKB = 0; modified = '' }
            }
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-SSHStateInfo {
    try {
        $files = @()
        $agentVerified = $true
        $knownHosts = Join-Path $env:USERPROFILE '.ssh\\known_hosts'
        if (Test-Path $knownHosts -PathType Leaf -ErrorAction SilentlyContinue) {
            $item = Get-Item -LiteralPath $knownHosts -Force -ErrorAction SilentlyContinue
            if ($item) { $files += @{ name = 'SSH known_hosts'; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } }
        }
        try {
            $agentStatus = Invoke-WCSshAdd -Option '-l'
            if ($agentStatus -eq 0) {
                $files += @{ name = 'SSH agent identities loaded'; sizeKB = 0; modified = '' }
            } elseif ($agentStatus -ne 1) { $agentVerified = $false }
        } catch { $agentVerified = $false }
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = (-not $agentVerified); status = if ($agentVerified) { 'complete' } else { 'partial' }; message = if ($agentVerified) { $null } else { 'SSH agent state could not be checked.' } }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-RemoteAccessLogsInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'TeamViewer'; path = (Join-Path $env:APPDATA 'TeamViewer'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'AnyDesk'; path = (Join-Path $env:APPDATA 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'AnyDesk'; path = (Join-Path $env:ProgramData 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'VNC'; path = (Join-Path $env:APPDATA 'RealVNC'); patterns = @('Connections*.txt','*.trace','*.log') },
            @{ label = 'Chrome Remote Desktop'; path = (Join-Path $env:LOCALAPPDATA 'Google\\Chrome Remote Desktop'); patterns = @('Connections*.txt','*.trace','*.log') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-PasswordManagerCachesInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = '1Password'; path = (Join-Path $env:LOCALAPPDATA '1Password\\Cache'); patterns = @('*') },
            @{ label = '1Password'; path = (Join-Path $env:LOCALAPPDATA '1Password\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'Bitwarden'; path = (Join-Path $env:APPDATA 'Bitwarden\\Cache'); patterns = @('*') },
            @{ label = 'Bitwarden'; path = (Join-Path $env:APPDATA 'Bitwarden\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'Bitwarden'; path = (Join-Path $env:APPDATA 'Bitwarden\\Session Storage'); patterns = @('*') },
            @{ label = 'LastPass'; path = (Join-Path $env:APPDATA 'LastPass\\Cache'); patterns = @('*') },
            @{ label = 'LastPass'; path = (Join-Path $env:APPDATA 'LastPass\\Local Storage\\leveldb'); patterns = @('*') },
            @{ label = 'LastPass'; path = (Join-Path $env:APPDATA 'LastPass\\Session Storage'); patterns = @('*') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-GameLauncherLogsInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'Steam log'; path = (Join-Path ${env:ProgramFiles(x86)} 'Steam\\logs'); patterns = @('*') },
            @{ label = 'Epic Games Launcher'; path = (Join-Path $env:LOCALAPPDATA 'EpicGamesLauncher\\Saved\\Logs'); patterns = @('*') },
            @{ label = 'Battle.net'; path = (Join-Path $env:APPDATA 'Battle.net\\Logs'); patterns = @('*') },
            @{ label = 'Battle.net'; path = (Join-Path $env:ProgramData 'Battle.net\\Logs'); patterns = @('*') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-AdobeRecentInfo {
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    $readerRoot = 'HKCU:\Software\Adobe\Acrobat Reader'
    try {
        try { $root = Get-Item -LiteralPath $readerRoot -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            Get-ChildItem -LiteralPath $readerRoot -Force -ErrorAction Stop | ForEach-Object {
                $version = $_
                foreach ($suffix in @('AVGeneral\cRecentFiles','AVGeneral\cDigitalSignatures')) {
                    $path = Join-Path $version.PSPath $suffix
                    try {
                        try { $item = Get-Item -LiteralPath $path -ErrorAction Stop }
                        catch [System.Management.Automation.ItemNotFoundException] { continue }
                        foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                            try {
                                $files.Add(@{ name = "Adobe $suffix value: $property"; path = $path; value = $item.GetValue($property, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); valueType = [string]$item.GetValueKind($property); sizeKB = 0; modified = '' })
                            } catch { $partial = $true }
                        }
                    } catch { $partial = $true }
                }
            }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = 0; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some Adobe recent history could not be checked.' } else { $null } }
}

function Get-OfficeTempFilesInfo {
    try {
        $files = [Collections.Generic.List[object]]::new()
        $totalBytes = [int64]0
        $roots = @(
            (Join-Path $env:USERPROFILE 'Documents'),
            (Join-Path $env:USERPROFILE 'Desktop'),
            (Join-Path $env:USERPROFILE 'Downloads')
        )
        foreach ($root in $roots) {
            try { $item = Get-Item -LiteralPath $root -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Office temporary-file coverage is unavailable.' }
            Get-ChildItem -Path $root -Recurse -Depth 2 -File -Include '~$*.doc*','~$*.xls*','~$*.ppt*' -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ name = "Office temporary file: $($_.FullName.Substring($root.Length).TrimStart('\\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                $totalBytes += $_.Length
            }
        }
        $tempRoot = $env:TEMP
        $tempItem = $null
        try { $tempItem = Get-Item -LiteralPath $tempRoot -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        if ($tempItem) {
            if (-not $tempItem.PSIsContainer -or ($tempItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Office temporary-file coverage is unavailable.' }
            Get-ChildItem -Path $tempRoot -Recurse -Depth 2 -File -Filter '*.tmp' -Force -ErrorAction Stop |
                Sort-Object LastWriteTime -Descending | ForEach-Object {
                    $files.Add(@{ name = "Temporary file: $($_.FullName.Substring($tempRoot.Length).TrimStart('\\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                    $totalBytes += $_.Length
                }
        }
        @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Test-WCCloudProviderInactive {
    param([Parameter(Mandatory)] [ValidateSet('OneDrive','Dropbox','GoogleDrive')] [string]$Provider)
    $processNames = @{ OneDrive = @('OneDrive'); Dropbox = @('Dropbox'); GoogleDrive = @('GoogleDriveFS','GoogleDrive') }
    $installPaths = @{
        OneDrive = @((Join-Path $env:LOCALAPPDATA 'Microsoft\\OneDrive\\OneDrive.exe'), (Join-Path $env:ProgramFiles 'Microsoft OneDrive\\OneDrive.exe'))
        Dropbox = @((Join-Path $env:LOCALAPPDATA 'Dropbox\\bin\\Dropbox.exe'), (Join-Path $env:ProgramFiles 'Dropbox\\Client\\Dropbox.exe'))
        GoogleDrive = @((Join-Path $env:LOCALAPPDATA 'Google\\DriveFS\\GoogleDriveFS.exe'), (Join-Path $env:ProgramFiles 'Google\\Drive File Stream\\GoogleDriveFS.exe'))
    }
    if (@(Get-Process -Name $processNames[$Provider] -ErrorAction SilentlyContinue).Count -gt 0) { return $false }
    foreach ($path in $installPaths[$Provider]) {
        if (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue) { return $false }
    }
    return $true
}

function Get-WCCloudPlaceholderFiles {
    param([Parameter(Mandatory)] [string]$UserProfilePath)
    $placeholders = @()
    if (-not (Test-Path $UserProfilePath -ErrorAction SilentlyContinue)) { return @() }
    $roots = @()
    Get-ChildItem -Path $UserProfilePath -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
        if ($_.Name -like 'OneDrive*') { $roots += @{ provider = 'OneDrive'; path = $_.FullName } }
        elseif ($_.Name -eq 'Dropbox') { $roots += @{ provider = 'Dropbox'; path = $_.FullName } }
        elseif ($_.Name -eq 'Google Drive') { $roots += @{ provider = 'GoogleDrive'; path = $_.FullName } }
    }
    foreach ($root in $roots) {
        if (-not (Test-WCCloudProviderInactive -Provider $root.provider)) { continue }
        if (-not (Test-Path $root.path -ErrorAction SilentlyContinue)) { continue }
        Get-ChildItem -Path $root.path -Recurse -Force -ErrorAction SilentlyContinue | Where-Object {
            ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0
        } | ForEach-Object { $placeholders += @{ provider = $root.provider; item = $_ } }
    }
    @($placeholders)
}

function Get-FirewallLogInfo {
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    try {
        $seenPaths = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($profile in @(Get-NetFirewallProfile -ErrorAction Stop)) {
            try {
            $path = [Environment]::ExpandEnvironmentVariables([string]$profile.LogFileName)
            if ($path -match '%[^%]+%') { throw 'Firewall log path contains an unresolved environment variable.' }
            if (-not $path -or -not $seenPaths.Add($path)) { continue }
            $item = $null
            try { $item = Get-Item -LiteralPath $path -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if ($item.PSIsContainer) { throw 'Firewall log configuration points to a folder instead of a file.' }
            if ($item) {
                $files.Add(@{ name = "Firewall $($profile.Name) log: $($item.Name)"; path = $item.FullName; sizeKB = [math]::Round($item.Length / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                $totalBytes += $item.Length
            }
            } catch { $partial = $true }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some firewall logs could not be checked.' } else { $null } }
}

function Get-NeighborCacheInfo {
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        Get-NetNeighbor -ErrorAction Stop | Where-Object { $_.State -ne 'Permanent' } | ForEach-Object {
            $files.Add(@{ name = "Neighbor: $($_.InterfaceAlias) $($_.IPAddress) ($($_.State))"; interfaceAlias = [string]$_.InterfaceAlias; interfaceIndex = $_.InterfaceIndex; address = [string]$_.IPAddress; linkLayerAddress = [string]$_.LinkLayerAddress; state = [string]$_.State; addressFamily = [string]$_.AddressFamily; sizeKB = 0; modified = '' })
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = 0; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some neighbor-cache entries could not be checked.' } else { $null } }
}

function Get-NetBIOSCacheInfo {
    $files = @()
    try {
        $LASTEXITCODE = 0
        $output = @(& nbtstat.exe -c 2>$null)
        $exitCode = $LASTEXITCODE
        foreach ($line in $output) {
            $text = ([string]$line).Trim()
            if ($text -match '<[0-9A-F]{2}>' -and $text -notmatch 'NetBIOS Remote Cache') {
                $files += @{ name = "NetBIOS cache: $text"; sizeKB = 0; modified = '' }
            }
        }
        if ($exitCode -ne 0) { throw 'NetBIOS cache could not be checked.' }
        @{ files = @($files); total = @($files).Count; totalSizeMB = 0 }
    }
    catch { @{ error = $true; message = 'NetBIOS cache could not be checked.'; files = @($files); total = @($files).Count; totalSizeMB = 0 } }
}

function Get-GeolocationCacheInfo {
    $files = [Collections.Generic.List[object]]::new()
    $totalBytes = [int64]0
    $partial = $false
    $lfSvc = 'C:\ProgramData\Microsoft\Windows\LfSvc'
    try {
        try { $root = Get-Item -LiteralPath $lfSvc -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'Location cache is not a directory.' }
            Get-ChildItem -LiteralPath $lfSvc -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ name = "Geolocation cache: $($_.FullName.Substring($lfSvc.Length).TrimStart('\'))"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                $totalBytes += $_.Length
            }
        }
    } catch { $partial = $true }
    $locationConsent = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\location'
    try {
        try { $root = Get-Item -LiteralPath $locationConsent -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            foreach ($name in @($root.Property | Where-Object { $_ -notmatch '^PS' })) {
                try {
                    $files.Add(@{ name = "Location consent metadata: $name"; path = $locationConsent; value = $root.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); valueType = [string]$root.GetValueKind($name); sizeKB = 0; modified = '' })
                } catch { $partial = $true }
            }
            try {
                Get-ChildItem -LiteralPath $locationConsent -Recurse -Force -ErrorAction Stop | ForEach-Object {
                    $key = $_
                    $values = @{}
                    foreach ($name in @($key.Property | Where-Object { $_ -notmatch '^PS' })) {
                        try { $values[$name] = @{ value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); type = [string]$key.GetValueKind($name) } }
                        catch { $partial = $true }
                    }
                    $files.Add(@{ name = "Location consent usage metadata: $($key.PSChildName)"; path = $key.PSPath; values = $values; sizeKB = 0; modified = '' })
                }
            } catch { $partial = $true }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some geolocation sources could not be checked.' } else { $null } }
}

function Get-VPNPhonebooksInfo {
    try {
        $files = @(Get-CleanupArtifactFiles -Sources @(
            @{ label = 'User VPN'; path = (Join-Path $env:APPDATA 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') },
            @{ label = 'User VPN'; path = (Join-Path $env:LOCALAPPDATA 'Microsoft\\Network\\Connections'); patterns = @('rasphone.pbk','*.log','*.etl') },
            @{ label = 'System VPN'; path = (Join-Path $env:ProgramData 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') }
        ))
        $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-ProxyCacheInfo {
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    $connections = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections'
    try {
        try { $item = Get-Item -LiteralPath $connections -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $item = $null }
        if ($item) {
            foreach ($property in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                try {
                    $files.Add(@{ name = "Proxy connection metadata: $property"; value = $item.GetValue($property, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); valueType = [string]$item.GetValueKind($property); sizeKB = 0; modified = '' })
                } catch { $partial = $true }
            }
        }
    } catch { $partial = $true }
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = 0; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some proxy connection metadata could not be checked.' } else { $null } }
}

function Get-CloudPlaceholdersInfo {
    try {
        $files = @()
        $totalBytes = [int64]0
        foreach ($entry in @(Get-WCCloudPlaceholderFiles -UserProfilePath $env:USERPROFILE)) {
            $item = $entry.item
            $size = if ($item.PSIsContainer) { 0 } else { [int64]$item.Length }
            $files += @{ name = "Orphaned $($entry.provider) placeholder: $($item.FullName)"; sizeKB = [math]::Round($size / 1KB, 1); modified = $item.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
            $totalBytes += $size
        }
        @{ files = @($files); total = @($files).Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2) }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-BITSQueueInfo {
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        Get-BitsTransfer -AllUsers -ErrorAction Stop | ForEach-Object {
            $transfer = $_
            $files.Add(@{ name = "BITS job: $($transfer.DisplayName) ($($transfer.JobState))"; jobId = [string]$transfer.JobId; state = [string]$transfer.JobState; owner = [string]$transfer.OwnerAccount; bytesTransferred = $transfer.BytesTransferred; bytesTotal = $transfer.BytesTotal; sizeKB = 0; modified = '' })
        }
    } catch { $partial = $true }
    $downloader = Join-Path $env:ProgramData 'Microsoft\\Network\\Downloader'
    try {
        try { $root = Get-Item -LiteralPath $downloader -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'BITS queue location is not a directory.' }
            Get-ChildItem -LiteralPath $downloader -File -Filter 'qmgr*.dat' -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ name = "BITS queue database: $($_.Name)"; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            }
        }
    } catch { $partial = $true }
    $totalBytes = [int64](($files | ForEach-Object { [int64]($_.sizeKB * 1KB) } | Measure-Object -Sum).Sum)
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($totalBytes / 1MB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some BITS queue sources could not be checked.' } else { $null } }
}

function Get-CellularHistoryInfo {
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        $interfaces = @(& netsh.exe mbn show interfaces 2>$null)
        if ($LASTEXITCODE -ne 0) { throw 'Cellular interfaces could not be read.' }
        $interfaceNames = @()
        foreach ($line in $interfaces) {
            $text = ([string]$line).Trim()
            if ($text -match '(?i)^(Name|Interface Name)\s*:\s*(.+)$') {
                $interfaceNames += $Matches[2].Trim()
            }
        }
        foreach ($interface in $interfaceNames) {
            try {
                $profiles = @(& netsh.exe mbn show profiles interface="$interface" 2>$null)
                if ($LASTEXITCODE -ne 0) { throw 'Cellular profiles could not be read.' }
                foreach ($line in $profiles) {
                    $text = ([string]$line).Trim()
                    if ($text -match '(?i)^(All User Profile|Profile|Profile Name)\s*:\s*(.+)$') {
                        $profileName = $Matches[2].Trim()
                        $files.Add(@{ name = "Cellular profile ($interface): $profileName"; profileName = $profileName; interface = $interface; source = 'Cellular profile'; sizeKB = 0; modified = '' })
                    }
                }
            } catch { $partial = $true }
        }
        @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = 0; interfaceCount = $interfaceNames.Count; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some cellular profiles could not be checked.' } else { $null } }
    }
    catch { @{ error = $true; message = $_.Exception.Message; files = @(); total = 0; totalSizeMB = 0 } }
}

function Get-CleanupCacheFileInventory {
    param([string]$Root, [string[]]$Patterns, [string]$ExtraDirectory = '')
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        try { $rootItem = Get-Item -LiteralPath $Root -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $rootItem = $null }
        if ($rootItem) {
            if (-not $rootItem.PSIsContainer) { throw 'Cache location is not a directory.' }
            foreach ($pattern in $Patterns) {
                try {
                    Get-ChildItem -LiteralPath $Root -File -Filter $pattern -Force -ErrorAction Stop | ForEach-Object {
                        $files.Add(@{ name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                    }
                } catch { $partial = $true }
            }
            if ($ExtraDirectory) {
                try {
                    $extra = Join-Path $Root $ExtraDirectory
                    try { $extraItem = Get-Item -LiteralPath $extra -Force -ErrorAction Stop }
                    catch [System.Management.Automation.ItemNotFoundException] { $extraItem = $null }
                    if ($extraItem) {
                        if (-not $extraItem.PSIsContainer) { throw 'Cache location is not a directory.' }
                        Get-ChildItem -LiteralPath $extra -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                            $files.Add(@{ source=$ExtraDirectory; name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                        }
                    }
                } catch { $partial = $true }
            }
        }
    } catch { $partial = $true }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some cache locations could not be checked.' } else { $null } }
}

function Get-WebCacheInfo {
    Get-CleanupCacheFileInventory -Root (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\WebCache') -Patterns @('WebCacheV*.dat', 'V01*.log')
}

function Get-ThumbnailCacheInfo {
    Get-CleanupCacheFileInventory -Root (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Explorer') -Patterns @('thumbcache_*.db', 'iconcache_*.db')
}

function Get-NotificationDatabaseInfo {
    Get-CleanupCacheFileInventory -Root (Join-Path $env:LOCALAPPDATA 'Microsoft\Windows\Notifications') -Patterns @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm') -ExtraDirectory 'wpnidm'
}

function Get-BranchCacheInfo {
    # Each source is independent: a denied NetworkService folder must not
    # make the device look empty or erase records already found elsewhere.
    $sources = @(
        @{ label = 'PeerDistRep'; path = "$env:SystemRoot\System32\PeerDistRep" },
        @{ label = 'PeerDistPub'; path = "$env:SystemRoot\ServiceProfiles\NetworkService\AppData\Local\PeerDistPub" }
    )
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    foreach ($src in $sources) {
        try {
            try { $root = Get-Item -LiteralPath $src.path -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            if (-not $root.PSIsContainer) { throw 'BranchCache location is not a directory.' }
            Get-ChildItem -LiteralPath $src.path -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ source=$src.label; name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            }
        } catch { $partial = $true }
    }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some BranchCache locations could not be checked.' } else { $null } }
}

# --- Phase E: system diagnostic / servicing / defender-history viewers (read-only) ---

function Get-EventTranscriptInfo {
    # Returns the Diagnostic Data Viewer's EventTranscript.db + its rotating
    # ESE transaction logs (*.jrs / *.rbs). System-wide (ProgramData), not per-user.
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    foreach ($src in @(
        @{ label = 'EventTranscript'; root = (Join-Path $env:ProgramData 'Microsoft\Diagnosis\EventTranscript'); patterns = @('EventTranscript.db', '*.jrs', '*.rbs'); recursive = $false },
        @{ label = 'ETLLogs'; root = (Join-Path $env:ProgramData 'Microsoft\Diagnosis\ETLLogs'); patterns = @('*.etl'); recursive = $true }
    )) {
        try {
            try { $root = Get-Item -LiteralPath $src.root -Force -ErrorAction Stop }
            catch [Management.Automation.ItemNotFoundException] { continue }
            if (-not $root.PSIsContainer) { throw 'Diagnostics location is not a directory.' }
            foreach ($pattern in $src.patterns) {
                try {
                    Get-ChildItem -LiteralPath $src.root -Filter $pattern -Recurse:$src.recursive -File -Force -ErrorAction Stop | ForEach-Object {
                        $files.Add(@{ source=$src.label; name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                    }
                } catch { $partial = $true }
            }
        } catch { $partial = $true }
    }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some diagnostics locations could not be checked.' } else { $null } }
}

function Get-ActivitiesTimelineInfo {
    # Returns Windows Timeline / Activities History database (ActivitiesCache.db)
    # + its WAL/SHM sidecars, recursed across every per-identity subfolder
    # under ConnectedDevicesPlatform.
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    $cdpDir = Join-Path $env:LOCALAPPDATA 'ConnectedDevicesPlatform'
    try {
        try { $root = Get-Item -LiteralPath $cdpDir -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'Activity location is not a directory.' }
            Get-ChildItem -LiteralPath $cdpDir -Include 'ActivitiesCache.db','*.db-wal','*.db-shm' -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ source='ConnectedDevicesPlatform'; name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            }
        }
    } catch { $partial = $true }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some activity locations could not be checked.' } else { $null } }
}

function Get-RdpBitmapCacheInfo {
    # Returns RDP client bitmap-cache tile files (thumbnails cached from past
    # outgoing RDP sessions).
    $result = Get-CleanupCacheFileInventory -Root (Join-Path $env:LOCALAPPDATA 'Microsoft\Terminal Server Client\Cache') -Patterns @('*.bin', '*.bmc')
    foreach ($file in $result.files) { $file.source = 'Terminal Server Client\Cache' }
    $result
}

function Get-CleanupMultiSourceFileInventory {
    param([object[]]$Sources)
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    foreach ($source in $Sources) {
        $result = Get-CleanupCacheFileInventory -Root $source.path -Patterns $source.patterns
        if ($result.partial) { $partial = $true }
        foreach ($file in $result.files) { $file.source = $source.label; $files.Add($file) }
    }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some log locations could not be checked.' } else { $null } }
}

function Get-ServicingLogsInfo {
    Get-CleanupMultiSourceFileInventory -Sources @(
        @{ label='CBS'; path=(Join-Path $env:SystemRoot 'Logs\CBS'); patterns=@('*') },
        @{ label='DISM'; path=(Join-Path $env:SystemRoot 'Logs\DISM'); patterns=@('*') }
    )
}

function Get-DeviceInstallLogsInfo {
    Get-CleanupMultiSourceFileInventory -Sources @(
        @{ label='INF'; path=(Join-Path $env:SystemRoot 'INF'); patterns=@('setupapi.*.log') }
    )
}

function Get-UsageTraceLogsInfo {
    Get-CleanupMultiSourceFileInventory -Sources @(
        @{ label='SleepStudy'; path=(Join-Path $env:SystemRoot 'System32\SleepStudy'); patterns=@('*.etl') },
        @{ label='WDI'; path=(Join-Path $env:SystemRoot 'System32\WDI\LogFiles'); patterns=@('*.etl') },
        @{ label='WMI'; path=(Join-Path $env:SystemRoot 'System32\LogFiles\WMI'); patterns=@('*.etl') }
    )
}

function Get-DefenderHistoryInfo {
    # Returns Windows Defender scan-history records + the MpCmdRun.log support log.
    # System path (ProgramData) — ACL-guarded like the other Phase E system viewers.
    $history = Get-CleanupRecursiveFileInventory -Root (Join-Path $env:ProgramData 'Microsoft\Windows Defender\Scans\History\Service') -Source 'ScanHistory'
    $files = [Collections.Generic.List[object]]::new()
    foreach ($file in $history.files) { $files.Add($file) }
    $partial = [bool]$history.partial
    $mpLog = Join-Path $env:ProgramData 'Microsoft\Windows Defender\Support\MpCmdRun.log'
    try {
        try { $f = Get-Item -LiteralPath $mpLog -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $f = $null }
        if ($f) {
            if ($f.PSIsContainer) { throw 'The Defender support log is not a file.' }
            $files.Add(@{ source='Support'; name=$f.Name; path=$f.FullName; sizeKB=[math]::Round($f.Length / 1KB, 1); modified=$f.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
        }
    } catch { $partial = $true }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some protection history locations could not be checked.' } else { $null } }
}

# --- Additional system/app usage viewers (read-only, no admin required) ---

function Get-AppLaunchHistoryInfo {
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        $bamRoot = 'HKLM:\SYSTEM\CurrentControlSet\Services\bam\State\UserSettings'
        $root = $null
        try { $root = Get-Item -LiteralPath $bamRoot -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { }
        if ($null -ne $root) {
            try {
                Get-ChildItem -LiteralPath $bamRoot -ErrorAction Stop | ForEach-Object {
                    $sid = $_
                    try {
                        $key = Get-Item -LiteralPath $sid.PSPath -ErrorAction Stop
                        foreach ($name in @($key.Property)) {
                            # BAM also stores control values; only executable values are launch records.
                            if ($name -in @('Version', 'SequenceNumber')) { continue }
                            try {
                                $value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                                if ($null -eq $value) { throw 'The app launch value could not be read.' }
                                $row = @{ source = 'BAM'; name = $name; userSid = $sid.PSChildName; value = $value; valueType = [string]$key.GetValueKind($name) }
                                if ($value -is [byte[]] -and $value.Length -ge 8) {
                                    try {
                                        $fileTime = [BitConverter]::ToInt64($value, 0)
                                        if ($fileTime -gt 0) { $row.lastRun = [DateTime]::FromFileTimeUtc($fileTime).ToString('o') }
                                    } catch { }
                                }
                                $files.Add($row)
                            } catch { $partial = $true; $files.Add(@{ source = 'BAM'; name = $name; userSid = $sid.PSChildName }) }
                        }
                    } catch { $partial = $true }
                }
            } catch { $partial = $true }
        }
        @{ files = @($files.ToArray()); total = $files.Count; countUnit = 'tracked programs'; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some app launch history could not be checked.' } else { $null } }
    }
    catch { @{ error = $true; message = "Failed to read app launch history: $($_.Exception.Message)" } }
}

function Get-CleanupOfficeMruInventory {
    param([string]$RegistryRoot, [int]$PreviewLimit = [int]::MaxValue)
    $files = [Collections.Generic.List[object]]::new()
    $count = 0
    $partial = $false
    foreach ($suffix in @('*\*\File MRU', '*\*\Place MRU', '*\*\Security\Trusted Documents\TrustRecords')) {
        $pattern = "$RegistryRoot\Software\Microsoft\Office\$suffix"
        try {
            Get-Item -Path $pattern -ErrorAction Stop | ForEach-Object {
                $key = $_
                try {
                    $keyLeaf = Split-Path $key.Name -Leaf
                    $appLeaf = Split-Path (Split-Path $key.Name -Parent) -Leaf
                    $props = Get-ItemProperty -Path $key.PSPath -ErrorAction Stop
                    foreach ($property in @($props.PSObject.Properties | Where-Object { $_.Name -notin @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider') })) {
                        $count++
                        if ($files.Count -lt $PreviewLimit) {
                            $files.Add(@{ source = "$appLeaf\$keyLeaf"; name = (Split-Path $property.Name -Leaf); key = $property.Name; path = $key.PSPath; value = $property.Value; sizeKB = 0; modified = '' })
                        }
                    }
                } catch { $partial = $true }
            }
        } catch [System.Management.Automation.ItemNotFoundException] { }
        catch { $partial = $true }
    }
    @{ files = @($files.ToArray()); total = $count; totalSizeMB = 0; partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some Office history locations could not be checked.' } else { $null } }
}

function Get-OfficeMruInfo {
    # Returns individual entries from Office's File MRU, Place MRU, and
    # Trusted Documents TrustRecords keys. Office version varies 14.0-16.0
    # across installs, so both the version and app-name segments are
    # wildcarded. Each matched key stores its entries as registry VALUES
    # (not subkeys) — Get-Item resolves the wildcarded key itself, then a
    # Get-ItemProperty pass over its values enumerates them; Get-ChildItem on
    # the resolved key would return nothing since there are no subkeys to
    # descend into. For TrustRecords the value NAME is itself the trusted
    # document's path, so Split-Path -Leaf yields the filename; for File/Place
    # MRU the value name is already a plain label ("Item 1") and Split-Path
    # is a harmless no-op.
    Get-CleanupOfficeMruInventory -RegistryRoot 'HKCU:'
}

function Get-EmbeddedWebCacheInfo {
    # Returns WebView2 (EBWebView) cache-folder sizes — many modern Win32 apps
    # embed a Chromium WebView2 runtime whose per-app profile cache lives under
    # a nested "EBWebView" folder somewhere below LOCALAPPDATA. -Depth 4 caps
    # the walk so this stays a bounded scan, not a slow full-profile crawl.
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        try { $root = Get-Item -LiteralPath $env:LOCALAPPDATA -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
        if ($root) {
            if (-not $root.PSIsContainer) { throw 'Application data location is not a directory.' }
            Get-ChildItem -LiteralPath $env:LOCALAPPDATA -Recurse -Directory -Filter 'EBWebView' -Force -Depth 4 -ErrorAction Stop | ForEach-Object {
                $directory = $_
                $sizeBytes = [long]0
                try {
                    Get-ChildItem -LiteralPath $directory.FullName -Recurse -File -Force -ErrorAction Stop | ForEach-Object { $sizeBytes += $_.Length }
                } catch { $partial = $true }
                $files.Add(@{ source=(Split-Path (Split-Path $directory.FullName -Parent) -Leaf); name='EBWebView'; path=$directory.FullName; sizeKB=[math]::Round($sizeBytes / 1KB, 1); modified=$directory.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            }
        }
    } catch { $partial = $true }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); coverage='limited'; coverageNote='Checks application data to depth 4 for WebView folders.'; partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some WebView cache locations could not be checked.' } else { $null } }
}

function Get-CleanupRecursiveFileInventory {
    param([string]$Root, [string]$Source)
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
    try {
        try { $rootItem = Get-Item -LiteralPath $Root -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { $rootItem = $null }
        if ($rootItem) {
            if (-not $rootItem.PSIsContainer) { throw 'Inventory location is not a directory.' }
            Get-ChildItem -LiteralPath $Root -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                $files.Add(@{ source=$Source; name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
            }
        }
    } catch { $partial = $true }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files=@($files.ToArray()); total=$files.Count; totalSizeMB=[math]::Round($size / 1KB, 2); partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some locations could not be checked.' } else { $null } }
}

function Get-P2PUpdateCacheInfo {
    Get-CleanupRecursiveFileInventory -Root (Join-Path $env:SystemRoot 'SoftwareDistribution\DeliveryOptimization') -Source 'DeliveryOptimization'
}

function Get-ReliabilityHistoryInfo {
    Get-CleanupRecursiveFileInventory -Root (Join-Path $env:ProgramData 'Microsoft\RAC\StateData') -Source 'RAC'
}

function Get-CleanupExplorerSearchInventory {
    param([string]$RegistryRoot, [int]$PreviewLimit = [int]::MaxValue)
    $files = [Collections.Generic.List[object]]::new()
    $count = 0
    $partial = $false
    foreach ($source in @(
        @{ label='WordWheelQuery'; path="$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery" },
        @{ label='TypedPaths'; path="$RegistryRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths" }
    )) {
        try {
            try { $item = Get-Item -LiteralPath $source.path -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { continue }
            foreach ($name in @($item.Property | Where-Object { $_ -notmatch '^PS' })) {
                try {
                    $value = $item.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                    $type = [string]$item.GetValueKind($name)
                    $count++
                    if ($files.Count -lt $PreviewLimit) {
                        $record = @{ source=$source.label; name=$name; key=$name; path=$source.path; value=$value; valueType=$type; sizeKB=0; modified='' }
                        if ($source.label -eq 'WordWheelQuery' -and $name -match '^\d+$' -and $value -is [byte[]] -and $value.Length % 2 -eq 0) {
                            try {
                                $text = [System.Text.UnicodeEncoding]::new($false, $false, $true).GetString($value).TrimEnd([char]0)
                                if ($text -and $text -notmatch '[\x00-\x08\x0b\x0c\x0e-\x1f]') { $record.text = $text }
                            } catch { }
                        }
                        $files.Add($record)
                    }
                } catch { $partial = $true }
            }
        } catch { $partial = $true }
    }
    @{ files=@($files.ToArray()); total=$count; totalSizeMB=0; partial=$partial; status=if ($partial) { 'partial' } else { 'complete' }; message=if ($partial) { 'Some Explorer search history could not be checked.' } else { $null } }
}

function Get-ExplorerSearchHistoryInfo {
    # Returns Explorer search-box + address-bar typed-path history entries.
    # WordWheelQuery is present on all supported Windows versions; its
    # TypedPaths sibling can be absent depending on build — guarded
    # independently so a missing one doesn't blank out the other.
    Get-CleanupExplorerSearchInventory -RegistryRoot 'HKCU:'
}

function Get-SearchPersonalizationInfo {
    # Returns local Windows Search personalization data: per-app search
    # jumplist/launch tracking under CurrentVersion\Search (distinct from
    # Explorer's own WordWheelQuery/TypedPaths, already covered separately),
    # plus the inking/typing personalization store used to tailor search
    # suggestions. Cloud-synced query history tied to a Microsoft Account
    # lives server-side and cannot be reached from local tools.
    $files = [Collections.Generic.List[object]]::new()
    $partial = $false
        $searchRoots = @(
            @{ label = 'JumplistData'; path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\JumplistData' },
            @{ label = 'Launch';       path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search\Launch' }
        )
        foreach ($root in $searchRoots) {
            try {
                try { $key = Get-Item -LiteralPath $root.path -ErrorAction Stop }
                catch [System.Management.Automation.ItemNotFoundException] { continue }
                try {
                    Get-ChildItem -LiteralPath $root.path -ErrorAction Stop | ForEach-Object {
                        $files.Add(@{ source = $root.label; name = $_.PSChildName; path = $_.PSPath; sizeKB = 0; modified = '' })
                    }
                } catch { $partial = $true }
                foreach ($name in @($key.Property)) {
                    try {
                        $files.Add(@{ source = $root.label; name = $name; path = $root.path; value = $key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); valueType = [string]$key.GetValueKind($name); sizeKB = 0; modified = '' })
                    } catch { $partial = $true }
                }
            } catch { $partial = $true }
        }
        $trainedDataStore = "$env:LOCALAPPDATA\Microsoft\InputPersonalization\TrainedDataStore"
        try {
            try { $root = Get-Item -LiteralPath $trainedDataStore -Force -ErrorAction Stop }
            catch [System.Management.Automation.ItemNotFoundException] { $root = $null }
            if ($root) {
                if (-not $root.PSIsContainer) { throw 'Search personalization location is not a directory.' }
                Get-ChildItem -LiteralPath $trainedDataStore -Recurse -File -Force -ErrorAction Stop | ForEach-Object {
                    $files.Add(@{ source = 'TrainedDataStore'; name = $_.Name; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                }
            }
        } catch { $partial = $true }
    $size = ($files | ForEach-Object { $_.sizeKB } | Measure-Object -Sum).Sum
    @{ files = @($files.ToArray()); total = $files.Count; totalSizeMB = [math]::Round($size / 1KB, 2); partial = $partial; status = if ($partial) { 'partial' } else { 'complete' }; message = if ($partial) { 'Some search personalization locations could not be checked.' } else { $null } }
}

function Get-PreviousWindowsInstallInfo {
    # Returns presence + size of C:\Windows.old (the prior OS + user profiles
    # kept after a feature update). Read-only — does NOT take ownership of
    # the TrustedInstaller-ACL'd tree, so size may read 0 for subfolders this
    # process cannot enumerate; the actual erase (Pro) takes ownership first.
    try {
        $path = "$env:SystemDrive\Windows.old"
        if (-not (Test-Path $path -ErrorAction SilentlyContinue)) {
            return @{ present = $false; sizeMB = 0 }
        }
        $sizeBytes = (Get-ChildItem -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue |
            Measure-Object -Property Length -Sum).Sum
        @{ present = $true; sizeMB = [math]::Round(($sizeBytes / 1MB), 1) }
    }
    catch { @{ error = $true; message = $_.Exception.Message } }
}

# ============================================================================
# MULTI-USER CLEANUP CLEANUP
# ============================================================================
#
# These functions extend the per-category cleaners above to operate across
# all Windows user accounts on the machine, not just the current session.
#
# Architecture:
#   Get-UserProfiles           — enumerate non-system user profile dirs
#   Get-LoggedInUsers          — find accounts with active/disconnected sessions
#   Invoke-WithUserHive        — load NTUSER.DAT, run a scriptblock, unload
#   Invoke-CleanupClearForUser — clean specified categories for one account
#   Invoke-CleanupClearAllUsers — orchestrate across all/selected accounts
# ============================================================================

# Exclude only Windows' known default and utility profiles. Real accounts can
# contain words such as "sandbox" in their name or profile path; callers also
# check Win32_UserProfile.Special, SID and supported profile location.
function Test-WCVisibleUserProfile {
    param(
        [string]$Name,
        [string]$DisplayName,
        [string]$Path
    )
    $hiddenExact = @('Default', 'Default User', 'defaultuser0', 'Public', 'All Users', 'WDAGUtilityAccount')
    if ([string]::IsNullOrWhiteSpace($Name)) { return $false }
    if ($hiddenExact -contains $Name -or $hiddenExact -contains $DisplayName) { return $false }
    return $true
}

# Returns all non-system user profiles as objects with Name and Path.
# Mirrors the user_profiles() function in acquire.rs.
function Get-UserProfiles {
    # Win32_UserProfile is the authoritative Windows list of user profiles.
    # Filter: S-1-5-21-* SIDs = real local/domain user accounts.
    # Excludes S-1-5-82-* (IIS app pool virtual accounts like ".NET v4.5"),
    # S-1-5-80-* (NT Service accounts), S-1-5-18/19/20 (System/Network/Local).
    $sysDrive = $env:SystemDrive
    # Identify the current account by SID, NOT by name. $env:USERNAME is the
    # SAM logon name, which diverges from the profile FOLDER name after an
    # account rename (the C:\Users\<name> folder is created once and never
    # renamed). SID is stable and unambiguous.
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    try {
    $profiles = Get-CimInstance -ClassName Win32_UserProfile -ErrorAction Stop |
        Where-Object {
            -not $_.Special -and
            $_.SID -match '^S-1-5-21-' -and
            $_.LocalPath -like "$sysDrive\Users\*"
        } |
        ForEach-Object {
            $leaf = Split-Path $_.LocalPath -Leaf
            # `name` is the folder leaf — the stable internal key used to target
            # clears/scans. `displayName` is the real (possibly renamed) account
            # name resolved from the SID, falling back to the folder name.
            $display = $leaf
            try { $display = (([Security.Principal.SecurityIdentifier]$_.SID).Translate([Security.Principal.NTAccount]).Value -split '\\')[-1] } catch {}
            if (Test-WCVisibleUserProfile -Name $leaf -DisplayName $display -Path $_.LocalPath) {
                @{ name = $leaf; displayName = $display; path = $_.LocalPath; sid = $_.SID; isCurrent = ($_.SID -eq $mySid) }
            }
        }
    } catch {
        return @{ error = $true; message = 'Windows user profiles could not be read.'; profiles = @(); total = 0; currentUser = ''; currentSid = $mySid; isAdmin = [bool](Test-IsAdmin) }
    }
    # currentUser is the current SID's profile FOLDER name (so it matches the
    # `name` keys the UI uses), not $env:USERNAME — otherwise selecting your own
    # account would be treated as "another user".
    $myLeaf = (@($profiles) | Where-Object { $_.isCurrent } | Select-Object -First 1).name
    @{ profiles = @($profiles); total = @($profiles).Count; currentUser = $myLeaf; currentSid = $mySid; isAdmin = [bool](Test-IsAdmin) }
}

# Fleet access groups target Windows accounts, including newly-created users
# that do not have a C:\Users profile folder until their first sign-in.
function Get-FleetAccessUsers {
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    $users = Get-CimInstance -ClassName Win32_UserAccount -Filter 'LocalAccount = True' -ErrorAction Stop |
        Where-Object { $_.Name -and -not $_.Disabled } |
        Sort-Object Name |
        ForEach-Object {
            $display = if ([string]::IsNullOrWhiteSpace([string]$_.FullName)) { [string]$_.Name } else { [string]$_.FullName }
            @{ name = [string]$_.Name; displayName = $display; sid = [string]$_.SID; isCurrent = ([string]$_.SID -eq $mySid) }
        }

    @{ users = @($users); total = @($users).Count }
}

# Returns usernames of accounts with Active or Disconnected RDP/console sessions.
# These users have live registry hives — reg load would fail or corrupt data.
function Get-LoggedInUsers {
    $raw = & query session 2>&1
    $users = @()
    foreach ($line in $raw) {
        # query session output columns: SESSIONNAME  USERNAME  ID  STATE  ...
        if ($line -match '^\s*(?:>?\s*)(\S+)\s+(\S+)\s+\d+\s+(Active|Disc)') {
            $users += $Matches[2]
        }
    }
    @{ users = @($users | Select-Object -Unique) }
}

# Load an offline NTUSER.DAT hive under a temporary key, run $ScriptBlock
# with -HiveRoot pointing at it, then unload the hive.
# Requires Administrator or SYSTEM context.
# $ScriptBlock receives one named parameter: [string]$HiveRoot — the full
# Registry:: path prefix to use instead of "HKCU:".
function Invoke-WithUserHive {
    param(
        [Parameter(Mandatory)] [string]$UserProfilePath,
        [Parameter(Mandatory)] [scriptblock]$ScriptBlock
    )
    $username  = Split-Path $UserProfilePath -Leaf
    $ntuser    = Join-Path $UserProfilePath 'NTUSER.DAT'
    $mountName = "WC_TempHive_$([guid]::NewGuid().ToString('N'))"
    $mountPath = "HKEY_USERS\$mountName"

    if (-not (Test-Path $ntuser)) {
        Write-Warning "Invoke-WithUserHive: NTUSER.DAT not found at $ntuser"
        return
    }

    # Check the hive isn't already loaded (e.g. user partially logged off)
    $alreadyLoaded = Test-Path "Registry::$mountPath"
    if ($alreadyLoaded) { throw 'Temporary profile hive name already exists; refusing to reuse it.' }
    if (-not $alreadyLoaded) {
        $loadOut = & reg load $mountPath $ntuser 2>&1
        $loadOk  = ($LASTEXITCODE -eq 0) -and (Test-Path "Registry::$mountPath")
        if (-not $loadOk) {
            Write-Warning "Invoke-WithUserHive: reg load failed for $username - $($loadOut -join ' ')"
            return
        }
    }

    try {
        & $ScriptBlock -HiveRoot "Registry::$mountPath"
    } finally {
        # Release .NET registry handles before unloading
        [System.GC]::Collect()
        [System.GC]::WaitForPendingFinalizers()
        if (-not $alreadyLoaded) {
            $null = & reg unload $mountPath 2>&1
        }
    }
}

# Clean specified cleanup categories for a single user account.
# $UserProfilePath: full path to the profile (e.g. C:\Users\Bob)
# $CategoryIds: array of category IDs from cleanupCategories.ts
# $HiveRoot: "HKCU:" for current user, "Registry::HKEY_USERS\WC_TempHive_*" for offline
function Assert-CleanupCategoriesAllowed {
    param([string[]]$CategoryIds, [switch]$AllowDefaultExcluded)
    $protected = @(
        'wlanprofiles','wlanprofile','removewlanprofile','clearwlanprofiles',
        'browserfootprints','clearbrowserfootprints','notepadstate','clearnotepadstate',
        'wsldata','clearwsldata','dockerdesktopdata','cleardockerdesktopdata',
        'virtualmachineartifacts','clearvirtualmachineartifacts',
        'developercaches','cleardevelopercaches','credentialmanager','clearcredentialmanager',
        'sshstate','clearsshstate','passwordmanagercaches','clearpasswordmanagercaches'
    )
    foreach ($id in $CategoryIds) {
        $normalized = ($id -replace '[^a-zA-Z0-9]', '').ToLowerInvariant()
        if ($normalized -in $protected -and (-not $AllowDefaultExcluded -or @($CategoryIds).Count -ne 1)) {
            throw 'This category is protected from cleanup unless individually confirmed.'
        }
    }
}

function Invoke-CleanupClearForUser {
    param(
        [Parameter(Mandatory)] [string]$UserProfilePath,
        [Parameter(Mandatory)] [string[]]$CategoryIds,
        [string]$HiveRoot = 'HKCU:',
        [object]$AllowDefaultExcluded = $false
    )
    Assert-CleanupCategoriesAllowed $CategoryIds -AllowDefaultExcluded:($AllowDefaultExcluded -is [bool] -and $AllowDefaultExcluded)
    # Secure-erase shims. The original implementation called Erase-OneFile /
    # Erase-Dir, but those are only ever defined INSIDE the $script:EraseFunctions
    # here-string in auto-erase.ps1 (base64-embedded into scheduled tasks) — they
    # are NOT live functions in this dispatch session, so every call silently
    # threw and was swallowed by the per-category catch, making multi-user
    # clears a no-op. Delegate to Remove-ItemSecure (core/utils.ps1, always
    # loaded) which does the same secure single-pass erase for both files and directories.
    function Erase-OneFile($p) { if ($p) { Remove-ItemSecure -Path $p } }
    function Erase-Dir($d)     { if ($d) { Remove-ItemSecure -Path $d } }
    function Get-CleanupTargetFiles([array]$Sources) {
        $targets = @()
        foreach ($source in $Sources) {
            if (-not (Test-Path $source.path -ErrorAction SilentlyContinue)) { continue }
            foreach ($pattern in $source.patterns) {
                $targets += @(Get-ChildItem -Path $source.path -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue |
                    ForEach-Object { $_.FullName })
            }
        }
        @($targets)
    }

    $appData  = Join-Path $UserProfilePath 'AppData\Roaming'
    $localApp = Join-Path $UserProfilePath 'AppData\Local'
    $docs     = Join-Path $UserProfilePath 'Documents'

    # Per-category verification result, returned to the caller so
    # Invoke-CleanupClearAllUsers can report genuine failures instead of a
    # hardcoded 'cleaned' regardless of whether anything was actually removed.
    $catResults = @{}

    foreach ($cat in $CategoryIds) {
        try {
            switch ($cat) {
                'rdpHistory' {
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Default",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Servers",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\LocalDevices"
                    )) {
                        if (Test-Path $k) { Remove-ItemSecure -Path $k }
                    }
                    Erase-OneFile (Join-Path $docs 'Default.rdp')
                    Erase-OneFile (Join-Path $appData 'Microsoft\Windows\Recent\AutomaticDestinations\1b4dd67f29cb1962.automaticDestinations-ms')
                    Erase-Dir    (Join-Path $localApp 'Microsoft\Terminal Server Client\Cache')
                    # Remove stored TERMSRV credentials (only works for current user context)
                    if ($HiveRoot -eq 'HKCU:') {
                        cmdkey /list 2>$null | Select-String 'Target: Domain:target=TERMSRV/' | ForEach-Object {
                            $t = $_.ToString().Split('=')[1]; cmdkey /delete:$t 2>$null | Out-Null
                        }
                    }
                    $stillThere = @(
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Default",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\Servers",
                        "$HiveRoot\Software\Microsoft\Terminal Server Client\LocalDevices"
                    ) | Where-Object { Test-Path $_ }
                    $catResults[$cat] = if ($stillThere.Count -eq 0) { 'cleaned' } else { 'failed' }
                }
                'recentFiles' {
                    $recentDir = Join-Path $appData 'Microsoft\Windows\Recent'
                    Erase-Dir $recentDir
                    $officeRecent = Join-Path $appData 'Microsoft\Office\Recent'
                    if (Test-Path $officeRecent) {
                        Get-ChildItem $officeRecent -Recurse -File -Force -EA SilentlyContinue |
                            ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $recentLeft = if (Test-Path $recentDir) { @(Get-ChildItem $recentDir -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $officeLeft = if (Test-Path $officeRecent) { @(Get-ChildItem $officeRecent -Recurse -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $catResults[$cat] = if (($recentLeft + $officeLeft) -eq 0) { 'cleaned' } else { 'failed' }
                }
                'jumpLists' {
                    $autoDest = Join-Path $appData 'Microsoft\Windows\Recent\AutomaticDestinations'
                    $custDest = Join-Path $appData 'Microsoft\Windows\Recent\CustomDestinations'
                    Erase-Dir $autoDest
                    Erase-Dir $custDest
                    $autoLeft = if (Test-Path $autoDest) { @(Get-ChildItem $autoDest -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $custLeft = if (Test-Path $custDest) { @(Get-ChildItem $custDest -File -Force -EA SilentlyContinue).Count } else { 0 }
                    $catResults[$cat] = if (($autoLeft + $custLeft) -eq 0) { 'cleaned' } else { 'failed' }
                }
                'psHistory' {
                    $hist = Join-Path $appData 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
                    if (Test-Path $hist) { Erase-OneFile $hist }
                }
                'browserFootprints' {
                    $chromiumRoots = @(
                        (Join-Path $localApp 'Google\Chrome\User Data'),
                        (Join-Path $localApp 'Microsoft\Edge\User Data'),
                        (Join-Path $localApp 'BraveSoftware\Brave-Browser\User Data'),
                        (Join-Path $appData  'Opera Software\Opera Stable'),
                        (Join-Path $localApp 'Vivaldi\User Data')
                    )
                    $chromiumArtifacts = @(
                        'History','History-journal','Cookies','Cookies-journal',
                        'Login Data','Login Data-journal','Top Sites','Top Sites-journal',
                        'Favicons','Favicons-journal','Web Data','Web Data-journal',
                        'Visited Links','Shortcuts','Shortcuts-journal',
                        'Media History','Media History-journal'
                    )
                    foreach ($root in $chromiumRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue |
                            Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile*' } |
                            ForEach-Object {
                                $prof = $_.FullName
                                foreach ($a in $chromiumArtifacts) {
                                    $p = Join-Path $prof $a
                                    if (Test-Path $p -PathType Leaf)      { Erase-OneFile $p }
                                    elseif (Test-Path $p -PathType Container) { Erase-Dir $p }
                                }
                            }
                    }
                    $geckoRoots = @(
                        (Join-Path $appData  'Mozilla\Firefox\Profiles'),
                        (Join-Path $localApp 'LibreWolf\Profiles')
                    )
                    $geckoArtifacts = @(
                        'places.sqlite','places.sqlite-journal','places.sqlite-wal','places.sqlite-shm',
                        'cookies.sqlite','cookies.sqlite-journal','cookies.sqlite-wal','cookies.sqlite-shm',
                        'logins.json','formhistory.sqlite','favicons.sqlite','favicons.sqlite-journal'
                    )
                    foreach ($root in $geckoRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue | ForEach-Object {
                            foreach ($a in $geckoArtifacts) {
                                $p = Join-Path $_.FullName $a
                                if (Test-Path $p) { Erase-OneFile $p }
                            }
                        }
                    }
                    $leftover = 0
                    foreach ($root in $chromiumRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue |
                            Where-Object { $_.Name -eq 'Default' -or $_.Name -like 'Profile*' } |
                            ForEach-Object {
                                foreach ($a in $chromiumArtifacts) {
                                    if (Test-Path (Join-Path $_.FullName $a)) { $leftover++ }
                                }
                            }
                    }
                    foreach ($root in $geckoRoots) {
                        if (-not (Test-Path $root)) { continue }
                        Get-ChildItem $root -Directory -EA SilentlyContinue | ForEach-Object {
                            foreach ($a in $geckoArtifacts) {
                                if (Test-Path (Join-Path $_.FullName $a)) { $leftover++ }
                            }
                        }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'shellBags' {
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Microsoft\Windows\Shell\BagMRU",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU"
                    )) {
                        if (Test-Path $k) { Remove-ItemSecure -Path $k }
                    }
                    $stillThere = @(
                        "$HiveRoot\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Microsoft\Windows\Shell\BagMRU",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\Bags",
                        "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU"
                    ) | Where-Object { Test-Path $_ }
                    $catResults[$cat] = if ($stillThere.Count -eq 0) { 'cleaned' } else { 'failed' }
                }
                'execCache' {
                    # UserAssist, MuiCache, and the user-side PCA Store/Persisted
                    # keys are per-user. ShimCache and the HKLM PCA keys are
                    # system-wide and handled elsewhere.
                    $ua = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist"
                    if (Test-Path $ua -ErrorAction SilentlyContinue) {
                        Get-ChildItem $ua -EA SilentlyContinue | ForEach-Object {
                            $countPath = Join-Path $_.PSPath 'Count'
                            if (Test-Path $countPath -ErrorAction SilentlyContinue) {
                                Remove-Item -Path $countPath -Recurse -Force -ErrorAction SilentlyContinue
                            }
                        }
                    }

                    $muiCache = "$HiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"
                    if (Test-Path $muiCache -ErrorAction SilentlyContinue) {
                        Remove-Item -Path $muiCache -Recurse -Force -ErrorAction SilentlyContinue
                        New-Item -Path $muiCache -Force -ErrorAction SilentlyContinue | Out-Null
                    }

                    $recentApps = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps"
                    if (Test-Path $recentApps -ErrorAction SilentlyContinue) {
                        Get-ChildItem $recentApps -ErrorAction SilentlyContinue | ForEach-Object {
                            Remove-Item -Path $_.PSPath -Recurse -Force -ErrorAction SilentlyContinue
                        }
                    }

                    foreach ($pcaPath in @(
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store",
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted"
                    )) {
                        if (Test-Path $pcaPath -ErrorAction SilentlyContinue) {
                            Remove-Item -Path $pcaPath -Recurse -Force -ErrorAction SilentlyContinue
                        }
                    }

                    $leftover = 0
                    if (Test-Path $ua -ErrorAction SilentlyContinue) {
                        $leftover += @(Get-ChildItem $ua -EA SilentlyContinue | ForEach-Object {
                            $countPath = Join-Path $_.PSPath 'Count'
                            if (Test-Path $countPath -ErrorAction SilentlyContinue) { $countPath }
                        }).Count
                    }
                    if (Test-Path $muiCache -ErrorAction SilentlyContinue) {
                        $muiItem = Get-Item -Path $muiCache -EA SilentlyContinue
                        $leftover += @($muiItem.Property | Where-Object { $_ -notmatch '^PS' -and $_ -ne 'LangID' }).Count
                        $leftover += @(Get-ChildItem $muiCache -EA SilentlyContinue).Count
                    }
                    if (Test-Path $recentApps -ErrorAction SilentlyContinue) {
                        $leftover += @(Get-ChildItem $recentApps -EA SilentlyContinue).Count
                    }
                    foreach ($pcaPath in @(
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store",
                        "$HiveRoot\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted"
                    )) {
                        if (Test-Path $pcaPath -ErrorAction SilentlyContinue) { $leftover++ }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'netDrives' {
                    # Drive mappings are session-specific; only actionable for current user
                    if ($HiveRoot -eq 'HKCU:') {
                        Get-PSDrive -PSProvider FileSystem -EA SilentlyContinue |
                            Where-Object { $_.DisplayRoot -like '\\*' } |
                            ForEach-Object { Remove-PSDrive -Name $_.Name -Force -EA SilentlyContinue }
                        net use * /delete /yes 2>$null | Out-Null
                        $remaining = @(Get-PSDrive -PSProvider FileSystem -EA SilentlyContinue | Where-Object { $_.DisplayRoot -like '\\*' })
                        $catResults[$cat] = if ($remaining.Count -eq 0) { 'cleaned' } else { 'failed' }
                    } else {
                        $catResults[$cat] = 'cleaned'
                    }
                }
                'credentialManager' {
                    $credentialDirs = @(
                        (Join-Path $appData 'Microsoft\Credentials'),
                        (Join-Path $localApp 'Microsoft\Credentials'),
                        (Join-Path $localApp 'Microsoft\Vault')
                    )
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $isCurrentProfile = $currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\'))
                    if ($isCurrentProfile) {
                        $targets = @{}
                        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
                            $text = ([string]$line).Trim()
                            if ($text -match '(?i)target=(.+)$') {
                                $target = $Matches[1].Trim()
                                if ($target) { $targets[$target] = $true }
                            }
                        }
                        foreach ($target in $targets.Keys) { & cmdkey.exe "/delete:$target" 2>$null | Out-Null }
                    }
                    foreach ($credentialDir in $credentialDirs) { Erase-Dir $credentialDir }
                    $leftover = @($credentialDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    if ($isCurrentProfile) {
                        $remainingTargets = @(& cmdkey.exe /list 2>$null | Where-Object { ([string]$_).Trim() -match '(?i)target=(.+)$' }).Count
                        $leftover += $remainingTargets
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'networkWizardHistory' {
                    $networkWizard = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Network\Network Connection Wizard"
                    if (Test-Path $networkWizard -ErrorAction SilentlyContinue) { Remove-ItemSecure -Path $networkWizard }
                    $catResults[$cat] = if (Test-Path $networkWizard -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'ntUserTraces' {
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU",
                        "$HiveRoot\Software\Microsoft\Office\16.0\Common\Internet\LocationsMRU",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\TypedURLsTime",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\IntelliForms\Storage2"
                    )) {
                        if (Test-Path $k -ErrorAction SilentlyContinue) {
                            # These locations store history in registry values, not
                            # child keys, so enumerate-and-remove is a silent no-op.
                            Remove-Item -Path $k -Recurse -Force -ErrorAction SilentlyContinue
                        }
                    }
                    $leftover = 0
                    foreach ($k in @(
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU",
                        "$HiveRoot\Software\Microsoft\Office\16.0\Common\Internet\LocationsMRU",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\TypedURLsTime",
                        "$HiveRoot\Software\Microsoft\Internet Explorer\IntelliForms\Storage2"
                    )) {
                        if (Test-Path $k -ErrorAction SilentlyContinue) { $leftover++ }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'notepadState' {
                    $tabDirs = @(Get-ChildItem (Join-Path $localApp 'Packages') -Filter 'Microsoft.WindowsNotepad_*' -Directory -Force -EA SilentlyContinue |
                        ForEach-Object { Join-Path $_.FullName 'LocalState\TabState' } | Where-Object { Test-Path $_ })
                    foreach ($tab in $tabDirs) {
                        Get-ChildItem $tab -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    foreach ($tab in $tabDirs) { $leftover += @(Get-ChildItem $tab -File -Force -EA SilentlyContinue).Count }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'walFiles' {
                    $walFiles = @(Get-ChildItem $appData,$localApp -Include '*.db-wal','*.db-shm','*.sqlite-wal','*.sqlite-shm' `
                        -Recurse -File -Force -EA SilentlyContinue)
                    $walFiles | ForEach-Object { Erase-OneFile $_.FullName }
                    $leftover = @(Get-ChildItem $appData,$localApp -Include '*.db-wal','*.db-shm','*.sqlite-wal','*.sqlite-shm' `
                        -Recurse -File -Force -EA SilentlyContinue).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'crashDumps' {
                    $targets = @(
                        (Join-Path $localApp 'CrashDumps'),
                        "$env:SystemRoot\Minidump",
                        "$env:ProgramData\Microsoft\Windows\WER\ReportArchive",
                        "$env:ProgramData\Microsoft\Windows\WER\ReportQueue",
                        (Join-Path $localApp 'Microsoft\Windows\WER\ReportArchive'),
                        (Join-Path $localApp 'Microsoft\Windows\WER\ReportQueue')
                    )
                    foreach ($t in $targets) {
                        if (Test-Path $t) {
                            Get-ChildItem $t -Recurse -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                            Get-ChildItem $t -Recurse -Directory -Force -EA SilentlyContinue |
                                Sort-Object FullName -Descending | ForEach-Object { Remove-Item $_.FullName -Force -EA SilentlyContinue }
                        }
                    }
                    $leftover = 0
                    foreach ($t in $targets) {
                        if (Test-Path $t) { $leftover += @(Get-ChildItem $t -Recurse -File -Force -EA SilentlyContinue).Count }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'recallDb' {
                    $recallDirs = @(
                        (Join-Path $localApp 'CoreAIPlatform.00\UKP'),
                        (Join-Path $localApp 'ConnectedDevicesPlatform')
                    )
                    foreach ($d in $recallDirs) {
                        if (Test-Path $d) {
                            Get-ChildItem $d -Recurse -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                        }
                    }
                    $leftover = 0
                    foreach ($d in $recallDirs) {
                        if (Test-Path $d) { $leftover += @(Get-ChildItem $d -Recurse -File -Force -EA SilentlyContinue).Count }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'webCache' {
                    $webCacheDir = Join-Path $localApp 'Microsoft\Windows\WebCache'
                    if (Test-Path $webCacheDir) {
                        $targets  = @(Get-ChildItem -Path (Join-Path $webCacheDir 'WebCacheV*.dat') -Force -EA SilentlyContinue)
                        $targets += @(Get-ChildItem -Path (Join-Path $webCacheDir 'V01*.log') -Force -EA SilentlyContinue)
                        $targets | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    if (Test-Path $webCacheDir) {
                        $leftover += @(Get-ChildItem -Path (Join-Path $webCacheDir 'WebCacheV*.dat') -Force -EA SilentlyContinue).Count
                        $leftover += @(Get-ChildItem -Path (Join-Path $webCacheDir 'V01*.log') -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'activitiesTimeline' {
                    $cdpDir = Join-Path $localApp 'ConnectedDevicesPlatform'
                    if (Test-Path $cdpDir) {
                        Get-ChildItem -Path $cdpDir -Include 'ActivitiesCache.db*' -Recurse -Force -EA SilentlyContinue |
                            ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = if (Test-Path $cdpDir) {
                        @(Get-ChildItem -Path $cdpDir -Include 'ActivitiesCache.db*' -Recurse -Force -EA SilentlyContinue).Count
                    } else { 0 }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'rdpBitmapCache' {
                    $rdpCacheDir = Join-Path $localApp 'Microsoft\Terminal Server Client\Cache'
                    if (Test-Path $rdpCacheDir) {
                        $targets  = @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bin') -Force -EA SilentlyContinue)
                        $targets += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bmc') -Force -EA SilentlyContinue)
                        $targets | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    if (Test-Path $rdpCacheDir) {
                        $leftover += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bin') -Force -EA SilentlyContinue).Count
                        $leftover += @(Get-ChildItem -Path (Join-Path $rdpCacheDir '*.bmc') -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'thumbnailDb' {
                    $explorerDir = Join-Path $localApp 'Microsoft\Windows\Explorer'
                    if (Test-Path $explorerDir) {
                        $targets  = @(Get-ChildItem -Path (Join-Path $explorerDir 'thumbcache_*.db') -Force -EA SilentlyContinue)
                        $targets += @(Get-ChildItem -Path (Join-Path $explorerDir 'iconcache_*.db') -Force -EA SilentlyContinue)
                        $targets | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    if (Test-Path $explorerDir) {
                        $leftover += @(Get-ChildItem -Path (Join-Path $explorerDir 'thumbcache_*.db') -Force -EA SilentlyContinue).Count
                        $leftover += @(Get-ChildItem -Path (Join-Path $explorerDir 'iconcache_*.db') -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'notificationDb' {
                    $notifDir = Join-Path $localApp 'Microsoft\Windows\Notifications'
                    foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                        Erase-OneFile (Join-Path $notifDir $name)
                    }
                    $wpnidmDir = Join-Path $notifDir 'wpnidm'
                    if (Test-Path $wpnidmDir -ErrorAction SilentlyContinue) {
                        Get-ChildItem $wpnidmDir -Recurse -File -Force -EA SilentlyContinue | ForEach-Object { Erase-OneFile $_.FullName }
                    }
                    $leftover = 0
                    foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                        if (Test-Path (Join-Path $notifDir $name)) { $leftover++ }
                    }
                    if (Test-Path $wpnidmDir -ErrorAction SilentlyContinue) {
                        $leftover += @(Get-ChildItem $wpnidmDir -Recurse -File -Force -EA SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'officeMru' {
                    # Office version varies 14.0-16.0 — wildcard both the version
                    # and app-name segments. Each matched key is removed whole
                    # (best-effort per key) rather than value-by-value; Office
                    # recreates these keys on next use.
                    $officePatterns = @(
                        "$HiveRoot\Software\Microsoft\Office\*\*\File MRU",
                        "$HiveRoot\Software\Microsoft\Office\*\*\Place MRU",
                        "$HiveRoot\Software\Microsoft\Office\*\*\Security\Trusted Documents\TrustRecords"
                    )
                    foreach ($pattern in $officePatterns) {
                        Get-Item -Path $pattern -ErrorAction SilentlyContinue | ForEach-Object {
                            try { Remove-Item -Path $_.PSPath -Recurse -Force -ErrorAction SilentlyContinue } catch {}
                        }
                    }
                    $leftover = 0
                    foreach ($pattern in $officePatterns) {
                        $leftover += @(Get-Item -Path $pattern -ErrorAction SilentlyContinue).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'embeddedWebCache' {
                    $dirs = @(Get-ChildItem -Path $localApp -Recurse -Directory -Filter 'EBWebView' -Force -Depth 4 -EA SilentlyContinue)
                    foreach ($d in $dirs) {
                        try { Remove-Item -Path $d.FullName -Recurse -Force -ErrorAction SilentlyContinue } catch {}
                    }
                    $leftover = @(Get-ChildItem -Path $localApp -Recurse -Directory -Filter 'EBWebView' -Force -Depth 4 -EA SilentlyContinue).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'explorerSearchHistory' {
                    # Best-effort per VALUE (not whole-key delete) — mirrors the
                    # $HiveRoot convention used by the other HKCU-based cases above.
                    $searchKeys = @(
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery",
                        "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths"
                    )
                    foreach ($k in $searchKeys) {
                        if (Test-Path $k -ErrorAction SilentlyContinue) {
                            $props = Get-ItemProperty -Path $k -ErrorAction SilentlyContinue
                            if ($props) {
                                $props.PSObject.Properties | Where-Object { $_.Name -notin @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider') } | ForEach-Object {
                                    try { Remove-ItemProperty -Path $k -Name $_.Name -ErrorAction SilentlyContinue } catch {}
                                }
                            }
                        }
                    }
                    $leftover = 0
                    foreach ($k in $searchKeys) {
                        if (Test-Path $k -ErrorAction SilentlyContinue) {
                            $props = Get-ItemProperty -Path $k -ErrorAction SilentlyContinue
                            if ($props) {
                                $leftover += @($props.PSObject.Properties | Where-Object { $_.Name -notin @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider') }).Count
                            }
                        }
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'wslData' {
                    $wslVhdx = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $wslVhdx = @(Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                            ForEach-Object { Join-Path $_.FullName 'LocalState\ext4.vhdx' } |
                            Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue })
                    }
                    foreach ($vhdx in $wslVhdx) { Erase-OneFile $vhdx }
                    $leftover = @($wslVhdx | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'dockerDesktopData' {
                    $dockerWsl = Join-Path $localApp 'Docker\wsl'
                    $dockerLog = Join-Path $appData 'Docker\log.txt'
                    $vhdx = if (Test-Path $dockerWsl -ErrorAction SilentlyContinue) {
                        @(Get-ChildItem -Path $dockerWsl -Recurse -File -Filter '*.vhdx' -Force -ErrorAction SilentlyContinue)
                    } else { @() }
                    foreach ($item in $vhdx) { Erase-OneFile $item.FullName }
                    Erase-OneFile $dockerLog
                    $leftover = if (Test-Path $dockerWsl -ErrorAction SilentlyContinue) {
                        @(Get-ChildItem -Path $dockerWsl -Recurse -File -Filter '*.vhdx' -Force -ErrorAction SilentlyContinue).Count
                    } else { 0 }
                    if (Test-Path $dockerLog -PathType Leaf -ErrorAction SilentlyContinue) { $leftover++ }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'virtualMachineArtifacts' {
                    $targets = @()
                    $vmwareRoot = Join-Path $docs 'Virtual Machines'
                    $virtualBoxRoot = Join-Path $UserProfilePath 'VirtualBox VMs'
                    if (Test-Path $vmwareRoot -ErrorAction SilentlyContinue) {
                        foreach ($pattern in @('*-0000*.vmdk','*.vmsn','vmware*.log')) {
                            $targets += @(Get-ChildItem -Path $vmwareRoot -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                        }
                    }
                    if (Test-Path $virtualBoxRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $virtualBoxRoot -Directory -Recurse -Force -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'Snapshots' } | ForEach-Object {
                            foreach ($pattern in @('*.vdi','*.vbox-prev')) {
                                $targets += @(Get-ChildItem -Path $_.FullName -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                            }
                        }
                        foreach ($pattern in @('VBox.log','VBoxSVC.log')) {
                            $targets += @(Get-ChildItem -Path $virtualBoxRoot -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                        }
                    }
                    $targets += @(
                        (Join-Path $env:ProgramData 'VMware\vmnetdhcp.log'),
                        (Join-Path $env:ProgramData 'VMware\vmnetnat.log')
                    ) | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }
                    $virtualBoxXml = Join-Path $UserProfilePath '.VirtualBox\VirtualBox.xml'
                    $vmwareMru = "$HiveRoot\Software\VMware, Inc.\VMware Workstation"
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $canAccessProfileHive = ($HiveRoot -ne 'HKCU:') -or ($currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\')))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    Erase-OneFile $virtualBoxXml
                    if ($canAccessProfileHive -and (Test-Path $vmwareMru -ErrorAction SilentlyContinue)) {
                        (Get-Item -Path $vmwareMru -ErrorAction SilentlyContinue).Property |
                            Where-Object { $_ -match '(?i)mru|recent' } |
                            ForEach-Object { Remove-ItemProperty -Path $vmwareMru -Name $_ -ErrorAction SilentlyContinue }
                    }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    if (Test-Path $virtualBoxXml -PathType Leaf -ErrorAction SilentlyContinue) { $leftover++ }
                    if ($canAccessProfileHive -and (Test-Path $vmwareMru -ErrorAction SilentlyContinue)) {
                        $leftover += @((Get-Item -Path $vmwareMru -ErrorAction SilentlyContinue).Property | Where-Object { $_ -match '(?i)mru|recent' }).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'developerCaches' {
                    $cacheDirs = @(
                        (Join-Path $appData 'npm-cache'),
                        (Join-Path $localApp 'npm-cache'),
                        (Join-Path $localApp 'pip\Cache'),
                        (Join-Path $UserProfilePath '.cargo\registry\cache'),
                        (Join-Path $UserProfilePath '.cargo\registry\src')
                    )
                    $configFiles = @(
                        (Join-Path $UserProfilePath '.npmrc'),
                        (Join-Path $UserProfilePath '.cargo\config.toml'),
                        (Join-Path $UserProfilePath '.cargo\credentials.toml')
                    )
                    foreach ($cacheDir in $cacheDirs) { Erase-Dir $cacheDir }
                    foreach ($configFile in $configFiles) { Erase-OneFile $configFile }
                    $leftover = @($cacheDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $leftover += @($configFiles | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'stickyNotes' {
                    $targets = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.MicrosoftStickyNotes_*' -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            foreach ($name in @('plum.sqlite','plum.sqlite-wal','plum.sqlite-shm')) {
                                $path = Join-Path $_.FullName "LocalState\\$name"
                                if (Test-Path $path -PathType Leaf -ErrorAction SilentlyContinue) { $targets += $path }
                            }
                        }
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'oneDriveMetadata' {
                    $settingsRoot = Join-Path $localApp 'Microsoft\\OneDrive\\settings'
                    $targets = @()
                    if (Test-Path $settingsRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $settingsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            foreach ($pattern in @('*.dat','*.db')) {
                                $targets += @(Get-ChildItem -Path $_.FullName -Recurse -File -Filter $pattern -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
                            }
                        }
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'spotlightCache' {
                    $assetDirs = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $assetDirs = @(Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.Windows.ContentDeliveryManager_*' -Force -ErrorAction SilentlyContinue |
                            ForEach-Object { Join-Path $_.FullName 'LocalState\\Assets' } |
                            Where-Object { Test-Path $_ -ErrorAction SilentlyContinue })
                    }
                    foreach ($assetDir in $assetDirs) { Erase-Dir $assetDir }
                    $leftover = @($assetDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'legacyIconCache' {
                    $iconCache = Join-Path $localApp 'IconCache.db'
                    Erase-OneFile $iconCache
                    $catResults[$cat] = if (Test-Path $iconCache -PathType Leaf -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'gameCaptures' {
                    $captureDirs = @((Join-Path $UserProfilePath 'Videos\\Captures'))
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                            Where-Object { $_.Name -like 'Microsoft.XboxGamingOverlay_*' -or $_.Name -like 'Microsoft.GamingApp_*' } |
                            ForEach-Object { $captureDirs += Join-Path $_.FullName 'LocalState' }
                    }
                    foreach ($captureDir in $captureDirs) { Erase-Dir $captureDir }
                    $leftover = @($captureDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'photosCache' {
                    $photoDirs = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $photoDirs = @(Get-ChildItem -Path $packagesRoot -Directory -Filter 'Microsoft.Windows.Photos_*' -Force -ErrorAction SilentlyContinue |
                            ForEach-Object { Join-Path $_.FullName 'LocalState' } |
                            Where-Object { Test-Path $_ -ErrorAction SilentlyContinue })
                    }
                    foreach ($photoDir in $photoDirs) { Erase-Dir $photoDir }
                    $leftover = @($photoDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'xboxCache' {
                    $xboxDirs = @()
                    $packagesRoot = Join-Path $localApp 'Packages'
                    if (Test-Path $packagesRoot -ErrorAction SilentlyContinue) {
                        $xboxDirs = @(Get-ChildItem -Path $packagesRoot -Directory -Force -ErrorAction SilentlyContinue |
                            Where-Object { $_.Name -like 'Microsoft.GamingApp_*' -or $_.Name -like 'Microsoft.XboxApp_*' } |
                            ForEach-Object { Join-Path $_.FullName 'LocalState' } |
                            Where-Object { Test-Path $_ -ErrorAction SilentlyContinue })
                    }
                    foreach ($xboxDir in $xboxDirs) { Erase-Dir $xboxDir }
                    $leftover = @($xboxDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'communicationCaches' {
                    $cacheDirs = @(
                        (Join-Path $appData 'Microsoft\\Teams\\Cache'), (Join-Path $appData 'Microsoft\\Teams\\IndexedDB'),
                        (Join-Path $localApp 'Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\Microsoft\\MSTeams\\EBWebView\\Default\\IndexedDB'),
                        (Join-Path $appData 'Slack\\Cache'), (Join-Path $appData 'Slack\\IndexedDB'), (Join-Path $appData 'Slack\\Local Storage\\leveldb'),
                        (Join-Path $appData 'Zoom\\data'), (Join-Path $appData 'Zoom\\logs'),
                        (Join-Path $appData 'discord\\Cache'), (Join-Path $appData 'discord\\IndexedDB'), (Join-Path $appData 'discord\\Local Storage\\leveldb')
                    )
                    foreach ($cacheDir in $cacheDirs) { Erase-Dir $cacheDir }
                    $leftover = @($cacheDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'editorHistory' {
                    $targets = @((Join-Path $appData 'Code\\User\\globalStorage\\storage.json'), (Join-Path $appData 'Code\\User\\globalStorage\\state.vscdb'))
                    $historyDirs = @((Join-Path $appData 'Code\\User\\History'))
                    $jetBrainsRoot = Join-Path $appData 'JetBrains'
                    if (Test-Path $jetBrainsRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $jetBrainsRoot -Directory -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            $targets += Join-Path $_.FullName 'options\\recentProjects.xml'
                            $historyDirs += Join-Path $localApp ("JetBrains\\{0}\\LocalHistory" -f $_.Name)
                        }
                    }
                    foreach ($target in $targets) { Erase-OneFile $target }
                    foreach ($historyDir in $historyDirs) { Erase-Dir $historyDir }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $leftover += @($historyDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'gitActivity' {
                    $gitConfig = Join-Path $UserProfilePath '.gitconfig'
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $isCurrentProfile = $currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\'))
                    if ($isCurrentProfile) {
                        foreach ($line in @(& cmdkey.exe /list 2>$null)) {
                            $text = ([string]$line).Trim()
                            if ($text -match '(?i)target=(.+)$' -and $Matches[1].Trim() -match '(?i)git:') {
                                & cmdkey.exe ("/delete:{0}" -f $Matches[1].Trim()) 2>$null | Out-Null
                            }
                        }
                    }
                    if (Test-Path $gitConfig -PathType Leaf -ErrorAction SilentlyContinue) { & git.exe config --file $gitConfig --unset-all safe.directory 2>$null | Out-Null }
                    $leftover = if (Test-Path $gitConfig -PathType Leaf -ErrorAction SilentlyContinue) {
                        @(& git.exe config --file $gitConfig --get-all safe.directory 2>$null).Count
                    } else { 0 }
                    if ($isCurrentProfile) {
                        $leftover += @(& cmdkey.exe /list 2>$null | Where-Object { ([string]$_).Trim() -match '(?i)target=.*git:' }).Count
                    }
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'sshState' {
                    $knownHosts = Join-Path $UserProfilePath '.ssh\\known_hosts'
                    Erase-OneFile $knownHosts
                    $currentProfile = [Environment]::GetFolderPath('UserProfile')
                    $agentVerified = $false
                    if ($currentProfile -and ($UserProfilePath.TrimEnd('\\') -eq $currentProfile.TrimEnd('\\'))) {
                        try {
                            if ((Invoke-WCSshAdd -Option '-D') -ne 0) { throw 'SSH agent identities could not be removed.' }
                            $agentVerified = (Invoke-WCSshAdd -Option '-l') -eq 1
                        } catch { $agentVerified = $false }
                    }
                    $catResults[$cat] = if ($agentVerified -and -not (Test-Path -LiteralPath $knownHosts -PathType Leaf -ErrorAction Stop)) { 'cleaned' } else { 'failed' }
                }
                'remoteAccessLogs' {
                    $targets = @(Get-CleanupTargetFiles @(
                        @{ path = (Join-Path $appData 'TeamViewer'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $appData 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $env:ProgramData 'AnyDesk'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $appData 'RealVNC'); patterns = @('Connections*.txt','*.trace','*.log') },
                        @{ path = (Join-Path $localApp 'Google\\Chrome Remote Desktop'); patterns = @('Connections*.txt','*.trace','*.log') }
                    ))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'passwordManagerCaches' {
                    $cacheDirs = @(
                        (Join-Path $localApp '1Password\\Cache'), (Join-Path $localApp '1Password\\Local Storage\\leveldb'),
                        (Join-Path $appData 'Bitwarden\\Cache'), (Join-Path $appData 'Bitwarden\\Local Storage\\leveldb'), (Join-Path $appData 'Bitwarden\\Session Storage'),
                        (Join-Path $appData 'LastPass\\Cache'), (Join-Path $appData 'LastPass\\Local Storage\\leveldb'), (Join-Path $appData 'LastPass\\Session Storage')
                    )
                    foreach ($cacheDir in $cacheDirs) { Erase-Dir $cacheDir }
                    $leftover = @($cacheDirs | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'gameLauncherLogs' {
                    $steamRoot = ${env:ProgramFiles(x86)}
                    if (-not $steamRoot) { $steamRoot = $env:ProgramFiles }
                    $targets = @(Get-CleanupTargetFiles @(
                        @{ path = (Join-Path $steamRoot 'Steam\\logs'); patterns = @('*') },
                        @{ path = (Join-Path $localApp 'EpicGamesLauncher\\Saved\\Logs'); patterns = @('*') },
                        @{ path = (Join-Path $appData 'Battle.net\\Logs'); patterns = @('*') },
                        @{ path = (Join-Path $env:ProgramData 'Battle.net\\Logs'); patterns = @('*') }
                    ))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'adobeRecent' {
                    $readerRoot = "$HiveRoot\Software\Adobe\Acrobat Reader"
                    $targets = @()
                    if (Test-Path $readerRoot -ErrorAction SilentlyContinue) {
                        Get-ChildItem -Path $readerRoot -Force -ErrorAction SilentlyContinue | ForEach-Object {
                            foreach ($suffix in @('AVGeneral\\cRecentFiles','AVGeneral\\cDigitalSignatures')) {
                                $path = Join-Path $_.PSPath $suffix
                                if (Test-Path $path -ErrorAction SilentlyContinue) { $targets += $path }
                            }
                        }
                    }
                    foreach ($target in $targets) { Remove-ItemSecure -Path $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'officeTempFiles' {
                    $roots = @((Join-Path $docs ''), (Join-Path $UserProfilePath 'Desktop'), (Join-Path $UserProfilePath 'Downloads'))
                    $tempRoot = Join-Path $localApp 'Temp'
                    $readTargets = {
                        foreach ($root in @($roots) + @($tempRoot)) {
                            try { $item = Get-Item -LiteralPath $root -Force -ErrorAction Stop }
                            catch [System.Management.Automation.ItemNotFoundException] { continue }
                            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Office temporary-file coverage is unavailable.' }
                            if ($root -eq $tempRoot) {
                                Get-ChildItem -LiteralPath $root -Recurse -Depth 2 -File -Filter '*.tmp' -Force -ErrorAction Stop | ForEach-Object { $_.FullName }
                            } else {
                                Get-ChildItem -Path $root -Recurse -Depth 2 -File -Include '~$*.doc*','~$*.xls*','~$*.ppt*' -Force -ErrorAction Stop | ForEach-Object { $_.FullName }
                            }
                        }
                    }
                    $targets = @(& $readTargets)
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @(& $readTargets).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'vpnPhonebooks' {
                    $targets = @(Get-CleanupTargetFiles @(
                        @{ path = (Join-Path $appData 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') },
                        @{ path = (Join-Path $localApp 'Microsoft\\Network\\Connections'); patterns = @('rasphone.pbk','*.log','*.etl') },
                        @{ path = (Join-Path $env:ProgramData 'Microsoft\\Network\\Connections\\Pbk'); patterns = @('rasphone.pbk','*.log','*.etl') }
                    ))
                    foreach ($target in $targets) { Erase-OneFile $target }
                    $leftover = @($targets | Where-Object { Test-Path $_ -PathType Leaf -ErrorAction SilentlyContinue }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                'proxyCache' {
                    $connections = "$HiveRoot\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Connections"
                    if (Test-Path $connections -ErrorAction SilentlyContinue) { Remove-ItemSecure -Path $connections }
                    $catResults[$cat] = if (Test-Path $connections -ErrorAction SilentlyContinue) { 'failed' } else { 'cleaned' }
                }
                'cloudPlaceholders' {
                    $targets = @(Get-WCCloudPlaceholderFiles -UserProfilePath $UserProfilePath | ForEach-Object { $_.item.FullName })
                    foreach ($target in $targets) { Remove-ItemSecure -Path $target }
                    $leftover = @((Get-WCCloudPlaceholderFiles -UserProfilePath $UserProfilePath) | ForEach-Object { $_.item.FullName }).Count
                    $catResults[$cat] = if ($leftover -eq 0) { 'cleaned' } else { 'failed' }
                }
                # System-wide categories are intentionally omitted — they are
                # handled by the existing single-user Clear-* functions and do
                # not need per-user iteration (event logs, prefetch, USB, etc.)
            }
        } catch {
            # Record the failure per-category instead of silently swallowing it —
            # one category's exception must not be reported as 'cleaned', and
            # must not abort the rest of the requested categories.
            $catResults[$cat] = 'failed'
        }
    }

    return $catResults
}

# Orchestrate a cleanup clear across all or selected user accounts.
# $CategoryIds: which categories to clean (scopeAware ones; systemWide
#               categories are silently skipped — they affect all users
#               already and should be cleaned via their own Clear-* function)
# $TargetUsers: usernames to target; empty = all non-system accounts
#
# Per-user logic:
#   current user (logged in) → Invoke-CleanupClearForUser with HKCU:
#   other user logged in     → file-system paths only (skip live HKCU)
#   other user offline       → load hive → clean → unload
function Invoke-CleanupClearAllUsers {
    [CmdletBinding()]
    param(
        # Accept either a real array or a comma-separated string — the backend
        # router passes all params as strings so we split here.
        [Parameter(Mandatory)] [string]$CategoryIds,
        [string]$TargetUsers = '',
        [object]$AllowDefaultExcluded = $false
    )

    $categoryArr = $CategoryIds -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    if (@($categoryArr).Count -eq 0) { throw 'At least one cleanup category is required.' }
    Assert-CleanupCategoriesAllowed $categoryArr -AllowDefaultExcluded:($AllowDefaultExcluded -is [bool] -and $AllowDefaultExcluded)
    $hasTargetUsers = $TargetUsers -and $TargetUsers.Trim() -ne ''
    $targetArr   = @(if ($hasTargetUsers) {
        $TargetUsers -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    })
    if ($hasTargetUsers -and $targetArr.Count -eq 0) { throw 'No valid user profiles were specified; no cleanup was performed.' }

    # Filter to only scope-aware categories
    $scopeAwareIds = @(
        'rdpHistory','recentFiles','jumpLists','psHistory','browserFootprints',
        'shellBags','execCache','netDrives','ntUserTraces','notepadState',
        'walFiles','crashDumps','recallDb',
        'webCache','thumbnailDb','notificationDb','activitiesTimeline','rdpBitmapCache',
        'wslData','dockerDesktopData','virtualMachineArtifacts','developerCaches',
        'credentialManager','networkWizardHistory',
        'stickyNotes','oneDriveMetadata','spotlightCache','legacyIconCache',
        'gameCaptures','photosCache','xboxCache',
        'communicationCaches','editorHistory','gitActivity','sshState','remoteAccessLogs',
        'passwordManagerCaches','gameLauncherLogs','adobeRecent','officeTempFiles',
        'vpnPhonebooks','proxyCache','cloudPlaceholders',
        'officeMru','embeddedWebCache','explorerSearchHistory'
    )
    $effectiveCats = $categoryArr | Where-Object { $scopeAwareIds -contains $_ }
    if (@($effectiveCats).Count -ne @($categoryArr).Count) { throw 'An unsupported cleanup category was requested.' }
    if ($effectiveCats.Count -eq 0) {
        return @{ status = 'ok'; message = 'No scope-aware categories requested'; results = @() }
    }

    # Current account is identified by SID (stable across renames), not by
    # $env:USERNAME — otherwise the logged-in profile would be mistaken for an
    # offline account and we'd try to `reg load` a hive that's already in use.
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    # Enumerate target profiles — S-1-5-21-* SIDs only (real user accounts)
    $sysDrive    = $env:SystemDrive
    try {
    $allProfiles = @(Get-CimInstance -ClassName Win32_UserProfile -ErrorAction Stop |
        Where-Object { -not $_.Special -and $_.SID -match '^S-1-5-21-' -and $_.LocalPath -like "$sysDrive\Users\*" } |
        ForEach-Object {
            $leaf = Split-Path $_.LocalPath -Leaf
            $display = $leaf
            try { $display = (([Security.Principal.SecurityIdentifier]$_.SID).Translate([Security.Principal.NTAccount]).Value -split '\\')[-1] } catch {}
            if (Test-WCVisibleUserProfile -Name $leaf -DisplayName $display -Path $_.LocalPath) {
                [pscustomobject]@{ Name = $leaf; FullName = $_.LocalPath; SID = $_.SID }
            }
        })
    } catch { throw 'Windows user profiles could not be read; no cleanup was performed.' }

    # Privilege gate — clearing another user's traces requires loading their
    # offline NTUSER.DAT hive and writing under their profile, both of which
    # need Administrator. A non-admin caller may explicitly target only their
    # own account; an all-users request is rejected rather than misreported.
    $isAdmin = Test-IsAdmin
    if (-not $isAdmin -and $targetArr.Count -eq 0) {
        throw 'Administrator access is required to clean all user profiles; no cleanup was performed.'
    }
    $profiles = @($allProfiles | Where-Object {
        ($isAdmin -or $_.SID -eq $mySid) -and ($targetArr.Count -eq 0 -or $targetArr -contains $_.SID -or $targetArr -contains $_.Name)
    })
    if ($targetArr.Count -gt 0) {
        foreach ($target in $targetArr) {
            if (@($profiles | Where-Object { $_.SID -eq $target -or $_.Name -eq $target }).Count -ne 1) {
                throw 'A selected user profile is unavailable, ambiguous or not authorized; no cleanup was performed.'
            }
        }
    } elseif ($profiles.Count -eq 0) {
        throw 'No supported user profiles were found; no cleanup was performed.'
    }

    $loggedIn = (Get-LoggedInUsers).users
    $results  = @()

    foreach ($prof in $profiles) {
        $username      = $prof.Name
        $isCurrentUser = ($prof.SID -eq $mySid)
        $isLoggedIn    = (($loggedIn -contains $username) -or (Test-Path -LiteralPath "Registry::HKEY_USERS\$($prof.SID)"))

        if ($isCurrentUser) {
            $catResults = Invoke-CleanupClearForUser -UserProfilePath $prof.FullName `
                -CategoryIds $effectiveCats -HiveRoot 'HKCU:' -AllowDefaultExcluded $AllowDefaultExcluded
            $failedCats = @($catResults.Keys | Where-Object { $catResults[$_] -ne 'cleaned' })
            $results += if ($failedCats.Count -eq 0) {
                @{ user = $username; status = 'cleaned' }
            } else {
                @{
                    user        = $username
                    status      = 'partial'
                    note        = 'Some categories could not be verified as removed'
                    failedCats  = $failedCats
                    cleanedCats = @($catResults.Keys | Where-Object { $catResults[$_] -eq 'cleaned' })
                }
            }

        } elseif ($isLoggedIn) {
            # File paths accessible to admin even for a live session
            $fileOnlyCats = $effectiveCats | Where-Object {
                $_ -in @('recentFiles','jumpLists','psHistory','browserFootprints',
                         'notepadState','walFiles','crashDumps','recallDb','credentialManager',
                         'wslData','dockerDesktopData','virtualMachineArtifacts','developerCaches',
                         'stickyNotes','oneDriveMetadata','spotlightCache','legacyIconCache',
                         'gameCaptures','photosCache','xboxCache','communicationCaches','editorHistory',
                         'gitActivity','sshState','remoteAccessLogs','passwordManagerCaches',
                         'gameLauncherLogs','officeTempFiles','vpnPhonebooks','cloudPlaceholders')
            }
            $registryCats = $effectiveCats | Where-Object {
                $_ -in @('rdpHistory','shellBags','execCache','netDrives','ntUserTraces','networkWizardHistory','adobeRecent','proxyCache')
            }
            $catResults = if ($fileOnlyCats.Count -gt 0) {
                Invoke-CleanupClearForUser -UserProfilePath $prof.FullName `
                    -CategoryIds $fileOnlyCats -HiveRoot 'HKCU:' -AllowDefaultExcluded $AllowDefaultExcluded
            } else { @{} }
            $failedCats = @($catResults.Keys | Where-Object { $catResults[$_] -ne 'cleaned' })
            $results += @{
                user        = $username
                status      = 'partial'
                note        = 'User is logged in - registry categories skipped (live hive)'
                skippedCats = @($registryCats)
                cleanedCats = @($catResults.Keys | Where-Object { $catResults[$_] -eq 'cleaned' })
                failedCats  = $failedCats
            }

        } else {
            # Offline user — load hive, clean, unload
            $capturedProf = $prof
            $capturedCats = $effectiveCats
            $catResults = Invoke-WithUserHive -UserProfilePath $prof.FullName -ScriptBlock {
                param([string]$HiveRoot)
                Invoke-CleanupClearForUser -UserProfilePath $capturedProf.FullName `
                    -CategoryIds $capturedCats -HiveRoot $HiveRoot -AllowDefaultExcluded $AllowDefaultExcluded
            }
            if ($null -eq $catResults) {
                # Invoke-WithUserHive returned early (NTUSER.DAT missing or reg
                # load failed) — nothing was verified, so this is a failure,
                # not a silent 'cleaned'.
                $results += @{
                    user       = $username
                    status     = 'partial'
                    note       = 'Could not load offline hive - categories not verified'
                    failedCats = @($capturedCats)
                }
            } else {
                $failedCats = @($catResults.Keys | Where-Object { $catResults[$_] -ne 'cleaned' })
                $results += if ($failedCats.Count -eq 0) {
                    @{ user = $username; status = 'cleaned' }
                } else {
                    @{
                        user        = $username
                        status      = 'partial'
                        note        = 'Some categories could not be verified as removed'
                        failedCats  = $failedCats
                        cleanedCats = @($catResults.Keys | Where-Object { $catResults[$_] -eq 'cleaned' })
                    }
                }
            }
        }
    }

    @{
        status  = 'ok'
        cleaned = @($results | Where-Object { $_.status -eq 'cleaned' }).Count
        partial = @($results | Where-Object { $_.status -eq 'partial' }).Count
        results = $results
    }
}

# ============================================================================
# PER-USER CLEANUP VIEWER
# ============================================================================
# Get-CleanupSummaryAllUsers: scan every scope-aware category for every
# real user account and return counts + preview items — same shape as the
# per-category Get-* commands the UI already uses, but grouped by user.
#
# File-based categories (recentFiles, psHistory, browserFootprints,
# notepadState, walFiles, crashDumps, recallDb) are read directly from
# the profile path — no special privileges needed beyond admin.
#
# Registry-based categories (rdpHistory, shellBags, execCache, netDrives,
# ntUserTraces) use the live HKU\<SID> hive for logged-in users, and
# Invoke-WithUserHive for offline users (requires admin).
# ============================================================================

function Convert-CleanupSummaryHistory {
    param([string[]]$Lines, [int]$PreviewMax)
    $nonEmpty = @($Lines | Where-Object { $_ -is [string] -and $_.Trim() -ne '' })
    $items = [Collections.Generic.List[string]]::new(); $records = [Collections.Generic.List[object]]::new()
    $start = [Math]::Max(0, $nonEmpty.Count - $PreviewMax)
    for ($index = $nonEmpty.Count - 1; $index -ge $start; $index--) {
        $items.Add($nonEmpty[$index])
        $records.Add(@{ id = $index; command = $nonEmpty[$index] })
    }
    return @{ count = $nonEmpty.Count; items = @($items.ToArray()); records = @($records.ToArray()) }
}

function Test-CleanupSummaryDetailSize {
    param($Records, $Items)
    $json = ConvertTo-Json -InputObject @{ records = @($Records); items = @($Items) } -Depth 30 -Compress
    return [Text.Encoding]::UTF8.GetByteCount($json) -le 8MB
}

function ConvertTo-CleanupCanonicalJson {
    param($Value)
    if ($null -eq $Value) { return 'null' }
    if ($Value -is [System.Collections.IDictionary] -or $Value.GetType().FullName -eq 'System.Management.Automation.PSCustomObject') {
        [string[]]$keys = @(if ($Value -is [System.Collections.IDictionary]) { $Value.Keys | ForEach-Object { [string]$_ } } else { $Value.PSObject.Properties.Name })
        [Array]::Sort($keys, [StringComparer]::Ordinal)
        $parts = foreach ($key in $keys) {
            $child = if ($Value -is [System.Collections.IDictionary]) { $Value[$key] } else { $Value.PSObject.Properties[$key].Value }
            $script:CleanupJsonSerializer.Serialize($key) + ':' + (ConvertTo-CleanupCanonicalJson $child)
        }
        return '{' + ($parts -join ',') + '}'
    }
    if ($Value -is [System.Collections.IList] -or $Value -is [array]) {
        $parts = foreach ($item in $Value) { ConvertTo-CleanupCanonicalJson $item }
        return '[' + ($parts -join ',') + ']'
    }
    return $script:CleanupJsonSerializer.Serialize($Value)
}

function Get-CleanupSummaryPage {
    param([object[]]$Records, [int]$Count, [string]$Category, [string]$ProfileSid, [int]$Offset, [string]$ExpectedSnapshot, [int]$ChunkOffset = 0)
    if ($Offset -lt 0 -or $ChunkOffset -lt 0 -or (($Offset -gt 0 -or $ChunkOffset -gt 0) -and $ExpectedSnapshot -notmatch '^sha256:[0-9a-f]{64}$') -or ($Offset -eq 0 -and $ChunkOffset -eq 0 -and $ExpectedSnapshot)) {
        throw 'The account detail page request is invalid.'
    }
    Add-Type -AssemblyName System.Web.Extensions -ErrorAction Stop
    $script:CleanupJsonSerializer = [System.Web.Script.Serialization.JavaScriptSerializer]::new()
    $script:CleanupJsonSerializer.MaxJsonLength = [int]::MaxValue
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        # Length-framed records preserve identity and order without constructing
        # a second serialized copy of the whole account snapshot in memory.
        $hashText = {
            param([string]$Text)
            $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
            $length = [BitConverter]::GetBytes([int64]$bytes.Length)
            $null = $sha.TransformBlock($length, 0, $length.Length, $length, 0)
            if ($bytes.Length) { $null = $sha.TransformBlock($bytes, 0, $bytes.Length, $bytes, 0) }
        }
        & $hashText (ConvertTo-CleanupCanonicalJson @{ version=2; category=$Category; sid=$ProfileSid; count=$Count; rows=$Records.Count })
        foreach ($record in $Records) { & $hashText (ConvertTo-CleanupCanonicalJson $record) }
        $null = $sha.TransformFinalBlock([byte[]]@(), 0, 0)
        $snapshot = 'sha256:' + [BitConverter]::ToString($sha.Hash).Replace('-', '').ToLowerInvariant()
    } finally { $sha.Dispose() }
    if ($ExpectedSnapshot -and $ExpectedSnapshot -cne $snapshot) { throw 'Account details changed while loading. Rescan before trying again.' }
    if ($Offset -gt $Records.Count -or ($ChunkOffset -gt 0 -and $Offset -ge $Records.Count)) { throw 'The account detail page offset is invalid.' }
    $pageSize = 5000
    $pageBudget = 8MB - 1024
    $pageBytes = 0
    $pageRows = [Collections.Generic.List[object]]::new()
    for ($index = $Offset; $index -lt $Records.Count -and $pageRows.Count -lt $pageSize; $index++) {
        $json = ConvertTo-Json -InputObject $Records[$index] -Depth 30 -Compress
        $rowBytes = [Text.Encoding]::UTF8.GetByteCount($json) + 1
        if ($rowBytes -gt $pageBudget) {
            if ($pageRows.Count -eq 0) {
                $bytes = [Text.Encoding]::UTF8.GetBytes($json)
                if ($ChunkOffset -ge $bytes.Length) { throw 'The account detail chunk offset is invalid.' }
                $chunkLength = [Math]::Min(3145728, $bytes.Length - $ChunkOffset)
                $chunk = [Convert]::ToBase64String($bytes, $ChunkOffset, $chunkLength)
                $nextChunk = $ChunkOffset + $chunkLength
                $nextRecord = $Offset + 1
                return @{ records=@(); snapshotHash=$snapshot; nextOffset=if ($nextRecord -lt $Records.Count) { $nextRecord } else { $null }; totalRows=$Records.Count; recordChunk=@{ offset=$ChunkOffset; totalBytes=$bytes.Length; data=$chunk; nextByteOffset=if ($nextChunk -lt $bytes.Length) { $nextChunk } else { $null } } }
            }
            break
        }
        if ($ChunkOffset -gt 0) { throw 'The account detail chunk request is invalid.' }
        if ($pageBytes + $rowBytes -gt $pageBudget) { break }
        $pageRows.Add($Records[$index])
        $pageBytes += $rowBytes
    }
    $page = @($pageRows.ToArray())
    while (-not (Test-CleanupSummaryDetailSize -Records $page -Items @())) {
        if ($page.Count -le 1) { throw 'One account detail record exceeds the size limit.' }
        $pageSize = [Math]::Max(1, [int][Math]::Floor($page.Count / 2))
        $page = @($Records | Select-Object -Skip $Offset -First $pageSize)
    }
    $next = $Offset + $page.Count
    return @{ records=$page; snapshotHash=$snapshot; nextOffset=if ($next -lt $Records.Count) { $next } else { $null }; totalRows=$Records.Count }
}

function New-CleanupSummaryCategoryRecord {
    param([string]$Category, [int]$Count, $items, $records, [string]$ScanError, [int]$previewMax)
    # Both delegated current-user scans and direct other-user scans are scoped
    # for these categories, even when they find no entries in that scope.
    $limited = $Category -in @('walFiles', 'recallDb')
    $returned = @($records | Select-Object -First $previewMax)
    $visibleCount = $returned.Count
    if ($Category -eq 'ntUserTraces' -and $returned.Count -and @($returned | Where-Object { $null -eq $_.entries }).Count -eq 0) {
        $visibleCount = 0
        foreach ($section in $returned) { $visibleCount += @($section.entries).Count }
    }
    $record = @{
        id = $Category; count = $Count
        items = @($items | Select-Object -First $previewMax)
        records = $returned
        status = if ($ScanError) { 'error' } elseif ($limited) { 'scoped' } else { 'complete' }
        error = [bool]$ScanError; message = $ScanError
        sampleCount = $returned.Count
        previewLimit = $previewMax
        truncated = (-not $ScanError -and $Count -gt $visibleCount)
    }
    if ($limited) { $record.coverage = 'limited' }
    return $record
}

function Get-CleanupSummaryAllUsers {
    [CmdletBinding()]
    param(
        # Comma-separated list of category IDs to scan; empty = all scope-aware
        [string]$CategoryIds = '',
        # Comma-separated list of usernames to scan; empty = every account.
        # The UI scans one user at a time (the user switcher), so it normally
        # passes a single name here to avoid the cost of loading every hive.
        [string]$TargetUsers = '',
        [bool]$FullDetail = $false,
        [int]$DetailOffset = 0,
        [string]$SnapshotHash = '',
        [int]$DetailChunkOffset = 0
    )

    function Test-CleanupSummaryPath {
        param([Parameter(Mandatory)][string]$Path, [string]$PathType = '')
        try { $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop }
        catch [System.Management.Automation.ItemNotFoundException] { return $false }
        if ($PathType -eq 'Leaf') { return -not $item.PSIsContainer }
        return $true
    }

    $allCats = @('rdpHistory','recentFiles','jumpLists','psHistory','browserFootprints',
                 'shellBags','execCache','ntUserTraces','notepadState','walFiles',
                 'crashDumps','recallDb','netDrives','webCache','thumbnailDb','notificationDb',
                 'activitiesTimeline','rdpBitmapCache','officeMru','embeddedWebCache','explorerSearchHistory','developerCaches')
    $supportedCats = @($allCats) + @('sshState', 'passwordManagerCaches', 'wslData', 'dockerDesktopData')
    $requestedCats = if ($CategoryIds -and $CategoryIds.Trim() -ne '') {
        $CategoryIds -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' -and $_ -in $supportedCats }
    } else { $allCats }

    if (@($requestedCats).Count -eq 0) { return @{ error = $true; status = 'error'; message = 'No supported per-user cleanup categories were requested.'; users = @(); userCount = 0 } }

    $targetArr = @(if ($TargetUsers -and $TargetUsers.Trim() -ne '') {
        $TargetUsers -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    })
    if ($FullDetail -and ($requestedCats.Count -ne 1 -or $targetArr.Count -ne 1)) {
        return @{ error = $true; status = 'error'; message = 'Full details require exactly one category and one user.'; users = @(); userCount = 0 }
    }
    if ((-not $FullDetail -and ($DetailOffset -ne 0 -or $DetailChunkOffset -ne 0 -or $SnapshotHash)) -or $DetailOffset -lt 0 -or $DetailChunkOffset -lt 0 -or (($DetailOffset -gt 0 -or $DetailChunkOffset -gt 0) -and $SnapshotHash -notmatch '^sha256:[0-9a-f]{64}$') -or ($DetailOffset -eq 0 -and $DetailChunkOffset -eq 0 -and $SnapshotHash)) {
        return @{ error = $true; status = 'error'; message = 'The account detail page request is invalid.'; users = @(); userCount = 0 }
    }

    # Current account identified by SID (stable across renames), not name.
    $mySid = ''
    try { $mySid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value } catch {}

    $sysDrive    = $env:SystemDrive
    try {
    $allProfiles = @(Get-CimInstance -ClassName Win32_UserProfile -ErrorAction Stop |
        Where-Object { -not $_.Special -and $_.SID -match '^S-1-5-21-' -and $_.LocalPath -like "$sysDrive\Users\*" } |
        ForEach-Object {
            $leaf = Split-Path $_.LocalPath -Leaf
            $display = $leaf
            try { $display = (([Security.Principal.SecurityIdentifier]$_.SID).Translate([Security.Principal.NTAccount]).Value -split '\\')[-1] } catch {}
            if (Test-WCVisibleUserProfile -Name $leaf -DisplayName $display -Path $_.LocalPath) {
                [pscustomobject]@{ Name = $leaf; FullName = $_.LocalPath; SID = $_.SID }
            }
        })
    } catch { return @{ error = $true; status = 'error'; message = 'User profile inventory could not be read.'; users = @(); userCount = 0 } }

    # Privilege gate — reading another user's offline hive / profile requires
    # Administrator. A non-admin caller can only ever see their own account
    # (matched by SID), regardless of what was requested.
    $isAdmin = Test-IsAdmin
    $allProfiles = @($allProfiles | Where-Object {
        ($isAdmin -or $_.SID -eq $mySid) -and
        ($targetArr.Count -eq 0 -or $targetArr -contains $_.SID -or $targetArr -contains $_.Name)
    })
    foreach ($target in $targetArr) {
        if (@($allProfiles | Where-Object { $_.SID -eq $target -or $_.Name -eq $target }).Count -ne 1) {
            return @{ error = $true; status = 'error'; message = 'The selected user profile is unavailable, ambiguous or not authorized.'; users = @(); userCount = 0 }
        }
    }
    if ($FullDetail -and @($allProfiles).Count -ne 1) {
        return @{ error = $true; status = 'error'; message = 'The selected user profile is unavailable or not authorized.'; users = @(); userCount = 0 }
    }

    $loggedIn = (Get-LoggedInUsers).users
    $userResults = @()

    foreach ($prof in $allProfiles) {
        $username      = $prof.Name
        $isCurrentUser = ($prof.SID -eq $mySid)
        $isLoggedIn    = ($loggedIn -contains $username)
        $appData       = Join-Path $prof.FullName 'AppData\Roaming'
        $localApp      = Join-Path $prof.FullName 'AppData\Local'

        # Determine registry hive root
        $hiveRoot      = 'HKCU:'   # default — overridden for non-current users
        $hiveLoaded    = $false
        $hiveAvailable = $true     # false when reg load failed or hive unreachable
        $hiveError     = $null     # surfaced to UI as a warning
        $mountName     = "WC_TempHive_$([guid]::NewGuid().ToString('N'))"
        $mountPath     = "HKEY_USERS\$mountName"

        try {
        if (-not $isCurrentUser -and @($requestedCats | Where-Object { $_ -notin @('sshState','passwordManagerCaches','dockerDesktopData') }).Count -gt 0) {
            # Strategy: try the live SID path in HKEY_USERS FIRST — this works for
            # any user who is logged in (interactive, RDP, disconnected session, etc.)
            # without needing reg load. Only fall back to reg load for truly offline users.
            # This avoids the "NTUSER.DAT locked" error for logged-in users that
            # Get-LoggedInUsers failed to detect.
            $sidRoot = "Registry::HKEY_USERS\$($prof.SID)"
            if (Test-CleanupSummaryPath $sidRoot) {
                $hiveRoot = $sidRoot
            } else {
                # SID not in HKEY_USERS — user is genuinely offline. Load the hive.
                $ntuser = Join-Path $prof.FullName 'NTUSER.DAT'
                if (Test-CleanupSummaryPath $ntuser) {
                    $already = Test-CleanupSummaryPath "Registry::$mountPath"
                    if (-not $already) {
                        $loadOut = & reg load $mountPath $ntuser 2>&1
                        $loadOk  = ($LASTEXITCODE -eq 0) -and (Test-CleanupSummaryPath "Registry::$mountPath")
                    } else {
                        $loadOk = $false
                    }
                    if ($loadOk) {
                        $hiveRoot   = "Registry::$mountPath"
                        $hiveLoaded = (-not $already)
                    } else {
                        # Truly inaccessible (locked by a session we cannot see).
                        # File-system categories still run; registry counts remain unknown.
                        $hiveAvailable = $false
                    }
                } else {
                    $hiveAvailable = $false
                }
            }
        }

        } catch {
            $hiveAvailable = $false
            $hiveError = 'The user registry hive could not be accessed.'
        }
        if (-not $hiveAvailable -and -not $hiveError) { $hiveError = 'The user registry hive is unavailable; registry categories were not scanned.' }

        # The card grid only shows the first few items, but detail dialogs need
        # enough rows to audit common high-count traces without hiding records.
        $previewMax = if ($FullDetail) { [int]::MaxValue } else { 500 }
        $categories = @()
        foreach ($cat in $requestedCats) {
            $count = 0; $items = @(); $records = @(); $r = $null; $scanError = $null; $categoryPartial = $false; $cacheUnavailable = $false; $dockerUnavailable = $false
            try {
                if (-not $hiveAvailable -and $cat -in @('rdpHistory','shellBags','execCache','ntUserTraces','netDrives','officeMru','explorerSearchHistory','wslData')) { throw $hiveError }

                # For the current user delegate to the same per-category functions the
                # main cleanup panel uses — counts will always be identical.
                # For other users fall through to the file-system + offline-hive path.
                $delegated = $false
                if ($cat -eq 'developerCaches') {
                    $profileApp = if ($isCurrentUser) { $env:APPDATA } else { $appData }
                    $profileLocal = if ($isCurrentUser) { $env:LOCALAPPDATA } else { $localApp }
                    $profileRoot = if ($isCurrentUser) { $env:USERPROFILE } else { $prof.FullName }
                    $r = Get-CleanupDeveloperCachesInfo -AppDataPath $profileApp -LocalAppDataPath $profileLocal -UserProfilePath $profileRoot -previewLimit $previewMax
                    $delegated = $true
                    $count = if ($r.total) { [int]$r.total } else { 0 }
                    $records = @($r.files)
                    $items = @($r.files | ForEach-Object { $_.name })
                } elseif ($isCurrentUser) {
                    $delegated = $true
                    switch ($cat) {
                        'recentFiles' {
                            $r = Get-RecentFiles
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.entries) | Select-Object -First $previewMax
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'psHistory' {
                            $r = Get-PSHistory
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.entries) | Select-Object -First $previewMax
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.command }
                        }
                        'jumpLists' {
                            $r = Get-JumpLists
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.entries) | Select-Object -First $previewMax
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'browserFootprints' {
                            $r = Get-BrowserFootprints
                            $count = if ($r.totalBrowsers) { [int]$r.totalBrowsers } else { 0 }
                            $records = @($r.browsers) | Select-Object -First $previewMax
                            $items = @($r.browsers) | Select-Object -First $previewMax | ForEach-Object { "$($_.browser): $($_.totalSizeKB) KB" }
                        }
                        'shellBags' {
                            $r = Get-ShellBags
                            $count = @($r.entries).Count
                            $records = @($r.entries) | Select-Object -First $previewMax
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.path }
                        }
                        'execCache' {
                            $r = Get-ExecutionCache
                            $count = @($r.entries).Count
                            $records = @($r.entries) | Select-Object -First $previewMax
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.path }
                        }
                        'ntUserTraces' {
                            $r = Get-NTUserTraces
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.sections) | Select-Object -First $previewMax
                            $items = @($r.sections) | ForEach-Object { "$($_.name): $($_.count)" } | Select-Object -First $previewMax
                        }
                        'rdpHistory' {
                            $r = Get-RDPHistory
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.entries) | Select-Object -First $previewMax
                            $items = @($r.entries) | Select-Object -First $previewMax | ForEach-Object { $_.host }
                        }
                        'notepadState' {
                            $r = Get-NotepadStateFiles
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'sshState' {
                            $r = Get-SSHStateInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($records | ForEach-Object { $_.name })
                        }
                        'passwordManagerCaches' {
                            $r = Get-PasswordManagerCachesInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($records | ForEach-Object { $_.name })
                        }
                        'wslData' {
                            $r = Get-WSLDataInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($records | ForEach-Object { $_.name })
                        }
                        'dockerDesktopData' {
                            $r = Get-DockerDesktopDataInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($records | ForEach-Object { $_.name })
                        }
                        'crashDumps' {
                            $r = Get-CrashDumpList
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.dumps) | Select-Object -First $previewMax
                            $items = @($r.dumps) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'walFiles' {
                            $r = Get-SQLiteWALList
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'recallDb' {
                            $r = Get-RecallDatabaseInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.databases) | Select-Object -First $previewMax
                            $items = @($r.databases) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'webCache' {
                            $r = Get-WebCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'thumbnailDb' {
                            $r = Get-ThumbnailCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'notificationDb' {
                            $r = Get-NotificationDatabaseInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'activitiesTimeline' {
                            $r = Get-ActivitiesTimelineInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'rdpBitmapCache' {
                            $r = Get-RdpBitmapCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'officeMru' {
                            $r = Get-OfficeMruInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'embeddedWebCache' {
                            $r = Get-EmbeddedWebCacheInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        'explorerSearchHistory' {
                            $r = Get-ExplorerSearchHistoryInfo
                            $count = if ($r.total) { [int]$r.total } else { 0 }
                            $records = @($r.files) | Select-Object -First $previewMax
                            $items = @($r.files) | Select-Object -First $previewMax | ForEach-Object { $_.name }
                        }
                        default { $delegated = $false }
                    }
                }
                if ($delegated -and ($null -eq $r -or $r.error -eq $true)) {
                    throw $(if ($r.message) { [string]$r.message } else { 'The category scan failed or was incomplete.' })
                }
                if (-not $delegated) {
                # Other-users path (file-system + offline-hive scanning)
                switch ($cat) {
                    'dockerDesktopData' {
                        # This is read-only metadata for selected profile defaults.
                        # System-wide and custom disks and engine state are excluded.
                        $categoryPartial = $true
                        $parent = Get-Item -LiteralPath (Split-Path -Parent $prof.FullName) -Force -ErrorAction Stop
                        $root = Get-Item -LiteralPath $prof.FullName -Force -ErrorAction Stop
                        foreach ($ancestor in @($parent, $root)) {
                            if (-not $ancestor.PSIsContainer -or ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The selected profile path cannot be checked safely.' }
                        }
                        $sources = @(
                            @{ label='Docker WSL disk'; relative='AppData\Local\Docker\wsl'; tree=$true },
                            @{ label='Docker VMM disk'; relative='AppData\Local\Docker\vm-data\DockerDesktop.vhdx'; tree=$false },
                            @{ label='Docker log'; relative='AppData\Roaming\Docker\log.txt'; tree=$false }
                        )
                        foreach ($source in $sources) {
                            $sourcePath = $prof.FullName; $available = $true; $sourceItem = $null
                            $components = @($source.relative -split '\\')
                            for ($i = 0; $i -lt $components.Count; $i++) {
                                $sourcePath = Join-Path $sourcePath $components[$i]
                                try { $sourceItem = Get-Item -LiteralPath $sourcePath -Force -ErrorAction Stop }
                                catch [System.Management.Automation.ItemNotFoundException] { $available = $false; break }
                                catch { $available = $false; $dockerUnavailable = $true; break }
                                if (($sourceItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -or (($i -lt $components.Count - 1 -or $source.tree) -and -not $sourceItem.PSIsContainer) -or ($i -eq $components.Count - 1 -and -not $source.tree -and $sourceItem.PSIsContainer)) { $available = $false; $dockerUnavailable = $true; break }
                            }
                            if (-not $available) { continue }
                            $queue = [Collections.Generic.Queue[object]]::new()
                            $queue.Enqueue($sourceItem)
                            while ($queue.Count -gt 0) {
                                $candidate = $queue.Dequeue()
                                if ($candidate.Attributes -band [IO.FileAttributes]::ReparsePoint) { $dockerUnavailable = $true; continue }
                                if ($candidate.PSIsContainer) {
                                    try {
                                        $directory = Get-Item -LiteralPath $candidate.FullName -Force -ErrorAction Stop
                                        if (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $dockerUnavailable = $true; continue }
                                        foreach ($child in @(Get-ChildItem -LiteralPath $candidate.FullName -Force -ErrorAction Stop)) { $queue.Enqueue($child) }
                                    } catch { $dockerUnavailable = $true }
                                    continue
                                }
                                if ($source.tree -and $candidate.Name -notlike '*.vhdx') { continue }
                                $count++
                                if ($records.Count -lt $previewMax) {
                                    $records += @{ name="$($source.label): $($candidate.Name)"; path=$candidate.FullName; sizeKB=[math]::Round($candidate.Length / 1KB, 1); modified=$candidate.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                                    $items += "$($source.label): $($candidate.Name)"
                                }
                            }
                        }
                    }
                    'wslData' {
                        # Inspect the selected SID's registration only. Do not
                        # invoke wsl.exe in the operator's account, and do not
                        # follow imported BasePath locations or open disks.
                        $categoryPartial = $true
                        $lxssPath = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Lxss"
                        if (Test-CleanupSummaryPath $lxssPath) {
                            foreach ($registration in @(Get-ChildItem -LiteralPath $lxssPath -ErrorAction Stop)) {
                                $count++
                                if ($records.Count -ge $previewMax) { continue }
                                $row = @{ registrationId = [string]$registration.PSChildName; registryPath = [string]$registration.PSPath; registryValues = @{} }
                                try {
                                    foreach ($name in @($registration.Property)) {
                                        $row.registryValues[[string]$name] = Get-ItemPropertyValue -LiteralPath $registration.PSPath -Name $name -ErrorAction Stop
                                    }
                                } catch { $row.readStatus = 'unavailable' }
                                $records += $row
                                $items += [string]$registration.PSChildName
                            }
                        }
                    }
                    'passwordManagerCaches' {
                        # Read only fixed cache locations. Other vendors and
                        # custom paths remain outside this partial inventory.
                        $categoryPartial = $true
                        $parent = Get-Item -LiteralPath (Split-Path -Parent $prof.FullName) -Force -ErrorAction Stop
                        $root = Get-Item -LiteralPath $prof.FullName -Force -ErrorAction Stop
                        foreach ($ancestor in @($parent, $root)) {
                            if (-not $ancestor.PSIsContainer -or ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The selected profile path cannot be checked safely.' }
                        }
                        $sources = @(
                            @{ label='1Password'; relative='AppData\Local\1Password\Cache' },
                            @{ label='1Password'; relative='AppData\Local\1Password\Local Storage\leveldb' },
                            @{ label='Bitwarden'; relative='AppData\Roaming\Bitwarden\Cache' },
                            @{ label='Bitwarden'; relative='AppData\Roaming\Bitwarden\Local Storage\leveldb' },
                            @{ label='Bitwarden'; relative='AppData\Roaming\Bitwarden\Session Storage' },
                            @{ label='LastPass'; relative='AppData\Roaming\LastPass\Cache' },
                            @{ label='LastPass'; relative='AppData\Roaming\LastPass\Local Storage\leveldb' },
                            @{ label='LastPass'; relative='AppData\Roaming\LastPass\Session Storage' }
                        )
                        foreach ($source in $sources) {
                            $sourcePath = $prof.FullName
                            $available = $true
                            foreach ($component in ($source.relative -split '\\')) {
                                $sourcePath = Join-Path $sourcePath $component
                                try { $directory = Get-Item -LiteralPath $sourcePath -Force -ErrorAction Stop }
                                catch [System.Management.Automation.ItemNotFoundException] { $available = $false; break }
                                catch { $available = $false; $cacheUnavailable = $true; break }
                                if (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $available = $false; $cacheUnavailable = $true; break }
                            }
                            if (-not $available) { continue }
                            $queue = [Collections.Generic.Queue[string]]::new()
                            $queue.Enqueue($sourcePath)
                            while ($queue.Count -gt 0) {
                                $directoryPath = $queue.Dequeue()
                                try {
                                    $directory = Get-Item -LiteralPath $directoryPath -Force -ErrorAction Stop
                                    if (-not $directory.PSIsContainer -or ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $cacheUnavailable = $true; continue }
                                    $children = @(Get-ChildItem -LiteralPath $directoryPath -Force -ErrorAction Stop)
                                } catch { $cacheUnavailable = $true; continue }
                                foreach ($child in $children) {
                                    if ($child.Attributes -band [IO.FileAttributes]::ReparsePoint) { $cacheUnavailable = $true; continue }
                                    if ($child.PSIsContainer) { $queue.Enqueue([string]$child.FullName); continue }
                                    $count++
                                    if ($records.Count -lt $previewMax) {
                                        $relativeName = $child.FullName.Substring($sourcePath.Length).TrimStart('\\')
                                        $records += @{ name = "$($source.label): $relativeName"; path = $child.FullName; sizeKB = [math]::Round($child.Length / 1KB, 1); modified = $child.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                                        $items += "$($source.label): $relativeName"
                                    }
                                }
                            }
                        }
                    }
                    'sshState' {
                        # The other account's agent is unavailable in this
                        # session. Inspect known_hosts metadata only.
                        $categoryPartial = $true
                        $parent = Get-Item -LiteralPath (Split-Path -Parent $prof.FullName) -Force -ErrorAction Stop
                        if (-not $parent.PSIsContainer -or ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The Windows users folder cannot be checked safely.' }
                        $root = Get-Item -LiteralPath $prof.FullName -Force -ErrorAction Stop
                        if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The selected profile path is unavailable.' }
                        $sshPath = Join-Path $prof.FullName '.ssh'
                        if (Test-CleanupSummaryPath $sshPath) {
                            $ssh = Get-Item -LiteralPath $sshPath -Force -ErrorAction Stop
                            if (-not $ssh.PSIsContainer -or ($ssh.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The SSH folder cannot be checked safely.' }
                            $knownHosts = Join-Path $sshPath 'known_hosts'
                            if (Test-CleanupSummaryPath $knownHosts) {
                                $file = Get-Item -LiteralPath $knownHosts -Force -ErrorAction Stop
                                if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'The SSH known-hosts file cannot be checked safely.' }
                                $count = 1
                                $records = @(@{ name='SSH known_hosts'; path=$file.FullName; sizeKB=[math]::Round($file.Length / 1KB, 1); modified=$file.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                                $items = @('SSH known_hosts')
                            }
                        }
                    }
                    'recentFiles' {
                        $p = Join-Path $appData 'Microsoft\Windows\Recent'
                        if (Test-CleanupSummaryPath $p) {
                            $files = @(Get-ChildItem -LiteralPath $p -File -Force -EA Stop)
                            $count = $files.Count
                            $shell = $null
                            $records = @($files | Select-Object -First $previewMax | ForEach-Object {
                                $target = $null
                                if ($_.Extension -ieq '.lnk') {
                                    try {
                                        if (-not $shell) { $shell = New-Object -ComObject WScript.Shell }
                                        $target = $shell.CreateShortcut($_.FullName).TargetPath
                                    } catch { }
                                }
                                @{ name=$_.BaseName; path=$_.FullName; extension=$_.Extension; target=$target; lastModified=$_.LastWriteTime.ToString('o'); sizeBytes=$_.Length }
                            })
                            $items = @($records | ForEach-Object { $_.name })
                        }
                    }
                    'psHistory' {
                        $p = Join-Path $appData 'Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt'
                        if (Test-CleanupSummaryPath $p) {
                            # Keep the original command text for the selected user's bounded preview.
                            $lines = [System.IO.File]::ReadAllLines($p)
                            $history = Convert-CleanupSummaryHistory -Lines $lines -PreviewMax $previewMax
                            $count = $history.count; $items = $history.items; $records = $history.records
                        }
                    }
                    'jumpLists' {
                        $auto = Join-Path $appData 'Microsoft\Windows\Recent\AutomaticDestinations'
                        $cust = Join-Path $appData 'Microsoft\Windows\Recent\CustomDestinations'
                        $f1 = if (Test-CleanupSummaryPath $auto) { @(Get-ChildItem $auto -File -Force -EA Stop) } else { @() }
                        $f2 = if (Test-CleanupSummaryPath $cust) { @(Get-ChildItem $cust -File -Force -EA Stop) } else { @() }
                        $count = @($f1).Count + @($f2).Count
                        $allRecords = @(foreach ($group in @(@{ type='Automatic'; files=$f1 }, @{ type='Custom'; files=$f2 })) {
                            foreach ($file in @($group.files)) {
                                @{ name=$file.Name; path=$file.FullName; type=$group.type; sizeKB=[math]::Round($file.Length / 1KB, 1); lastModified=$file.LastWriteTime.ToString('o') }
                            }
                        })
                        $records = @($allRecords | Select-Object -First $previewMax)
                        $items = @($records | ForEach-Object { $_.name })
                    }
                    'browserFootprints' {
                        $inventory = @(Get-CleanupSummaryBrowserInventory -ProfilePath $prof.FullName)
                        $r = Get-CleanupBrowserFootprints -Browsers $inventory
                        if ($r.error -or $r.partial -or $r.complete -eq $false) { throw 'The browser scan failed or was incomplete.' }
                        $count = [int]$r.totalBrowsers
                        $records = @($r.browsers | Select-Object -First $previewMax)
                        $items = @($records | ForEach-Object { "$($_.browser): $($_.totalSizeKB) KB" })
                    }
                    'notepadState' {
                        $packageRoot = Join-Path $localApp 'Packages'
                        $packages = @()
                        if (Test-CleanupSummaryPath $packageRoot) {
                            $rootItem = Get-Item -LiteralPath $packageRoot -Force -ErrorAction Stop
                            if (-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Notepad package coverage is unavailable.' }
                            $packages = @(Get-ChildItem -LiteralPath $packageRoot -Filter 'Microsoft.WindowsNotepad_*' -Directory -Force -EA Stop)
                        }
                        $files = @()
                        foreach ($pkg in $packages) {
                            if ($pkg.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'A Notepad package path is a reparse point.' }
                            $tab = Join-Path $pkg.FullName 'LocalState\TabState'
                            if (Test-CleanupSummaryPath $tab) {
                                $tabItem = Get-Item -LiteralPath $tab -Force -ErrorAction Stop
                                if (-not $tabItem.PSIsContainer -or ($tabItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Notepad tab-state coverage is unavailable.' }
                                $files += @(Get-ChildItem -LiteralPath $tab -File -Recurse -Force -ErrorAction Stop)
                            }
                        }
                        $count = $files.Count
                        $records = @($files | Select-Object -First $previewMax | ForEach-Object { @{ name = $_.Name; path = $_.FullName; sizeKB = [math]::Round($_.Length / 1KB, 1); modified = $_.LastWriteTime.ToString('o') } })
                        $items = @($records | ForEach-Object { $_.name })
                    }
                    'crashDumps' {
                        # Match Get-CrashDumpList: CrashDumps + Minidump + WER ReportArchive + ReportQueue
                        $dirs = @(
                            @{ label='CrashDumps'; path=(Join-Path $localApp 'CrashDumps') },
                            @{ label='Minidump'; path="$env:SystemRoot\Minidump" },
                            @{ label='WER ReportArchive'; path=(Join-Path $localApp 'Microsoft\Windows\WER\ReportArchive') },
                            @{ label='WER ReportQueue'; path=(Join-Path $localApp 'Microsoft\Windows\WER\ReportQueue') }
                        )
                        $dumpRows = [Collections.Generic.List[object]]::new()
                        foreach ($source in $dirs) {
                            if (Test-CleanupSummaryPath $source.path) {
                                $f = @(Get-ChildItem -LiteralPath $source.path -Recurse -File -Force -EA Stop)
                                $count += $f.Count
                                foreach ($file in $f) {
                                    if ($dumpRows.Count -ge $previewMax) { break }
                                    $dumpRows.Add(@{ source=$source.label; name=$file.Name; path=$file.FullName; sizeKB=[math]::Round($file.Length / 1KB, 1); modified=$file.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                                }
                            }
                        }
                        $records = @($dumpRows.ToArray())
                        $items = @($records | ForEach-Object { $_.name })
                    }
                    'walFiles' {
                        # Match per-category function: *.wal, *.db-wal, *.sqlite-wal, *.shm, *.db-shm
                        # Depth 5 balances coverage vs scan time (browsers live ≤4 deep in AppData).
                        $roots = @(@($appData,$localApp) | Where-Object { Test-CleanupSummaryPath $_ })
                        $f = if ($roots.Count -gt 0) { Get-ChildItem -LiteralPath $roots `
                            -Include '*.wal','*.db-wal','*.sqlite-wal','*.shm','*.db-shm' `
                            -Recurse -Depth 5 -File -Force -EA Stop } else { @() }
                        $count = @($f).Count
                        $records = @(@($f) | Select-Object -First $previewMax | ForEach-Object { @{ name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); dir=(Split-Path (Split-Path $_.FullName -Parent) -Leaf); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } })
                        $items = @($records | ForEach-Object { $_.name })
                    }
                    'recallDb' {
                        # Match the four recursive locations in Get-RecallDatabaseInfo.
                        $dirs = @(
                            @{ label='Recall'; path=(Join-Path $localApp 'CoreAIPlatform.00') },
                            @{ label='Recall'; path=(Join-Path $localApp 'Microsoft\Windows\Recall') },
                            @{ label='Timeline'; path=(Join-Path $localApp 'ConnectedDevicesPlatform') },
                            @{ label='ActionCenter'; path=(Join-Path $localApp 'Microsoft\Windows\ActionCenter') }
                        )
                        $recallRows = [Collections.Generic.List[object]]::new()
                        foreach ($source in $dirs) {
                            if (Test-CleanupSummaryPath $source.path) {
                                $files = @(Get-ChildItem -Path $source.path -Include '*.db','*.db-wal','*.db-shm','*.db-journal','*.edb' -Recurse -File -Force -EA Stop)
                                $count += $files.Count
                                foreach ($file in $files) {
                                    if ($recallRows.Count -ge $previewMax) { break }
                                    $recallRows.Add(@{ source=$source.label; name=$file.Name; path=$file.FullName; sizeKB=[math]::Round($file.Length / 1KB, 1); modified=$file.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') })
                                }
                            }
                        }
                        $records = @($recallRows.ToArray())
                        $items = @($records | ForEach-Object { $_.name })
                    }
                    'webCache' {
                        # Match Get-WebCacheInfo: WebCacheV*.dat (V01 historically, V24 on newer builds) + rotating V01*.log
                        $p = Join-Path $localApp 'Microsoft\Windows\WebCache'
                        if (Test-CleanupSummaryPath $p) {
                            $f  = @(Get-ChildItem -LiteralPath $p -File -Filter 'WebCacheV*.dat' -Force -EA Stop)
                            $f += @(Get-ChildItem -LiteralPath $p -File -Filter 'V01*.log' -Force -EA Stop)
                            $count = $f.Count
                            $records = @($f | Select-Object -First $previewMax | ForEach-Object { @{ name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } })
                            $items = @($records | ForEach-Object { $_.name })
                        }
                    }
                    'thumbnailDb' {
                        # Match Get-ThumbnailCacheInfo: thumbcache_*.db + iconcache_*.db
                        $p = Join-Path $localApp 'Microsoft\Windows\Explorer'
                        if (Test-CleanupSummaryPath $p) {
                            $f  = @(Get-ChildItem -LiteralPath $p -File -Filter 'thumbcache_*.db' -Force -EA Stop)
                            $f += @(Get-ChildItem -LiteralPath $p -File -Filter 'iconcache_*.db' -Force -EA Stop)
                            $count = $f.Count
                            $records = @($f | Select-Object -First $previewMax | ForEach-Object { @{ name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } })
                            $items = @($records | ForEach-Object { $_.name })
                        }
                    }
                    'notificationDb' {
                        # Match Get-NotificationDatabaseInfo: wpndatabase.db + -wal + -shm
                        $p = Join-Path $localApp 'Microsoft\Windows\Notifications'
                        $f = @()
                        foreach ($name in @('wpndatabase.db', 'wpndatabase.db-wal', 'wpndatabase.db-shm')) {
                            $filePath = Join-Path $p $name
                            if (Test-CleanupSummaryPath $filePath -PathType Leaf) { $f += Get-Item -LiteralPath $filePath -Force -EA Stop }
                        }
                        $wpnidm = Join-Path $p 'wpnidm'
                        if (Test-CleanupSummaryPath $wpnidm) { $f += @(Get-ChildItem -LiteralPath $wpnidm -Recurse -File -Force -EA Stop) }
                        $count = @($f).Count
                        $records = @($f | Select-Object -First $previewMax | ForEach-Object { @{ name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } })
                        $items = @($records | ForEach-Object { $_.name })
                    }
                    'activitiesTimeline' {
                        # Match Get-ActivitiesTimelineInfo: ActivitiesCache.db + -wal/-shm sidecars
                        $p = Join-Path $localApp 'ConnectedDevicesPlatform'
                        if (Test-CleanupSummaryPath $p) {
                            $f = Get-ChildItem -Path $p -Include 'ActivitiesCache.db','*.db-wal','*.db-shm' -Recurse -File -Force -EA Stop
                            $count = @($f).Count
                            $records = @(@($f) | Select-Object -First $previewMax | ForEach-Object { @{ source='ConnectedDevicesPlatform'; name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } })
                            $items = @($records | ForEach-Object { $_.name })
                        }
                    }
                    'rdpBitmapCache' {
                        # Match Get-RdpBitmapCacheInfo: *.bin + *.bmc
                        $p = Join-Path $localApp 'Microsoft\Terminal Server Client\Cache'
                        if (Test-CleanupSummaryPath $p) {
                            $f  = @(Get-ChildItem -LiteralPath $p -File -Filter '*.bin' -Force -EA Stop)
                            $f += @(Get-ChildItem -LiteralPath $p -File -Filter '*.bmc' -Force -EA Stop)
                            $count = $f.Count
                            $records = @($f | Select-Object -First $previewMax | ForEach-Object { @{ name=$_.Name; path=$_.FullName; sizeKB=[math]::Round($_.Length / 1KB, 1); modified=$_.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') } })
                            $items = @($records | ForEach-Object { $_.name })
                        }
                    }
                    'embeddedWebCache' {
                        # Match Get-EmbeddedWebCacheInfo: EBWebView folders under LOCALAPPDATA, depth-capped
                        $dirs = if (Test-CleanupSummaryPath $localApp) { @(Get-ChildItem -LiteralPath $localApp -Recurse -Directory -Filter 'EBWebView' -Force -Depth 4 -EA Stop) } else { @() }
                        $count = @($dirs).Count
                        $records = @($dirs | Select-Object -First $previewMax | ForEach-Object {
                            $dir = $_
                            $size = (Get-ChildItem -LiteralPath $dir.FullName -Recurse -File -Force -EA Stop | Measure-Object -Property Length -Sum).Sum
                            @{ source=(Split-Path (Split-Path $dir.FullName -Parent) -Leaf); name='EBWebView'; path=$dir.FullName; sizeKB=[math]::Round(([long]$size) / 1KB, 1); modified=$dir.LastWriteTime.ToString('yyyy-MM-dd HH:mm:ss') }
                        })
                        $items = @($records | ForEach-Object { $_.source })
                    }
                    'rdpHistory' {
                        if ($hiveAvailable) {
                            # Match Get-RDPHistory: Default MRU + per-server UsernameHint entries
                            $defaultK = "$hiveRoot\Software\Microsoft\Terminal Server Client\Default"
                            if (Test-CleanupSummaryPath $defaultK) {
                                $props = (Get-Item $defaultK -EA Stop).Property |
                                    Where-Object { $_ -match '^MRU\d+$' }
                                foreach ($property in @($props)) {
                                    $hostValue = Get-ItemPropertyValue -LiteralPath $defaultK -Name $property -EA Stop
                                    $count++
                                    if ($records.Count -lt $previewMax) {
                                        $records += @{ type='MRU'; host=$hostValue; key=$property }
                                        $items += [string]$hostValue
                                    }
                                }
                            }
                            $serversK = "$hiveRoot\Software\Microsoft\Terminal Server Client\Servers"
                            if (Test-CleanupSummaryPath $serversK) {
                                $servers = Get-ChildItem -LiteralPath $serversK -EA Stop
                                foreach ($server in @($servers)) {
                                    $serverProps = Get-ItemProperty -LiteralPath $server.PSPath -EA Stop
                                    $count++
                                    if ($records.Count -lt $previewMax) {
                                        $records += @{ type='Server'; host=$server.PSChildName; username=$serverProps.UsernameHint; key=$server.PSPath }
                                        $items += $server.PSChildName
                                    }
                                }
                            }
                            # Default.rdp file (profile-relative)
                            $defaultRdp = Join-Path (Split-Path $appData -Parent | Split-Path -Parent) 'Documents\Default.rdp'
                            if (Test-CleanupSummaryPath $defaultRdp) {
                                $file = Get-Item -LiteralPath $defaultRdp -Force -EA Stop
                                $count++
                                if ($records.Count -lt $previewMax) {
                                    $records += @{ type='DefaultRDP'; host='Default.rdp'; key=$defaultRdp; lastModified=$file.LastWriteTime.ToString('o') }
                                    $items += 'Default.rdp'
                                }
                            }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'shellBags' {
                        if ($hiveAvailable) {
                            # Preserve each observed registry slot and its values. PIDL
                            # bytes are not necessarily a decodable filesystem path.
                            foreach ($bagRoot in @(
                                "$hiveRoot\Software\Microsoft\Windows\Shell\BagMRU",
                                "$hiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\BagMRU"
                            )) {
                                try {
                                    if (Test-CleanupSummaryPath $bagRoot) {
                                        Get-ChildItem -LiteralPath $bagRoot -Recurse -EA Stop | ForEach-Object {
                                            $bag = $_; $count++
                                            if ($records.Count -lt $previewMax) {
                                                $values = @{}
                                                foreach ($name in @($bag.Property)) {
                                                    try { $values[$name] = Get-ItemPropertyValue -LiteralPath $bag.PSPath -Name $name -EA Stop }
                                                    catch { $categoryPartial = $true }
                                                }
                                                $records += @{ path = $bag.PSPath; source = 'ShellBags'; registryValues = $values }
                                                $items += [string]$bag.PSPath
                                            }
                                        }
                                    }
                                } catch { $categoryPartial = $true }
                            }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'execCache' {
                        if ($hiveAvailable) {
                            # Mirror Get-ExecutionCache exactly: Store+Persisted (HKLM, all users) +
                            # UserAssist ROT13 decoded paths + MuiCache exe keys + RecentApps.
                            # Deduplicate across all sources the same way.
                            $seen = @{}
                            $addExecutionEntry = {
                                param([string]$EntryPath, [string]$EntrySource)
                                if (-not [string]::IsNullOrWhiteSpace($EntryPath) -and -not $seen.ContainsKey($EntryPath)) {
                                    $seen[$EntryPath] = $true
                                    $items += $EntryPath
                                    $records += @{ path = $EntryPath; source = $EntrySource }
                                }
                            }

                            # HKLM AppCompat Store + Persisted (same for every user, no hive needed)
                            foreach ($hklmP in @(
                                'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Store',
                                'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Compatibility Assistant\Persisted'
                            )) {
                                try {
                                if (Test-CleanupSummaryPath $hklmP) {
                                    $p = Get-ItemProperty $hklmP -EA Stop
                                    if ($p) {
                                        $p.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                                            . $addExecutionEntry $_.Name (Split-Path -Leaf $hklmP)
                                        }
                                    }
                                }
                                } catch { $categoryPartial = $true }
                            }

                            # UserAssist — ROT13 decode property names under each GUID\Count subkey
                            $uaK = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist"
                            try {
                            if (Test-CleanupSummaryPath $uaK) {
                                Get-ChildItem $uaK -EA Stop | ForEach-Object {
                                    try {
                                    $cK = "$($_.PSPath)\Count"
                                    if (Test-CleanupSummaryPath $cK) {
                                        $p = Get-ItemProperty $cK -EA Stop | Select-Object -Property *
                                        if ($p) {
                                            $p.PSObject.Properties | Where-Object { $_.Name -notmatch '^PS' } | ForEach-Object {
                                                $d = ($_.Name.ToCharArray() | ForEach-Object {
                                                    $c = [int][char]$_
                                                    if ($c -ge 65 -and $c -le 90) { [char](((($c-65)+13)%26)+65) }
                                                    elseif ($c -ge 97 -and $c -le 122) { [char](((($c-97)+13)%26)+97) }
                                                    else { [char]$c }
                                                }) -join ''
                                                $d = $d -replace '^\\\?\\', ''
                                                . $addExecutionEntry $d 'UserAssist'
                                            }
                                        }
                                    }
                                    } catch { $categoryPartial = $true }
                                }
                            }
                            } catch { $categoryPartial = $true }

                            # MuiCache — property NAME is the exe path (same logic as Get-ExecutionCache)
                            $muiK = "$hiveRoot\Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"
                            try {
                            if (Test-CleanupSummaryPath $muiK) {
                                (Get-Item $muiK -EA Stop).Property |
                                    Where-Object { $_ -notmatch '^PS' -and $_ -ne 'LangID' } |
                                    ForEach-Object { . $addExecutionEntry $_ 'MuiCache' }
                            }
                            } catch { $categoryPartial = $true }

                            # Match the current-user scanner: the executable path is AppPath.
                            $raK = "$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Search\RecentApps"
                            try {
                            if (Test-CleanupSummaryPath $raK) {
                                Get-ChildItem $raK -EA Stop | ForEach-Object {
                                    try {
                                    $p2 = Get-ItemProperty $_.PSPath -EA Stop
                                    if ($p2 -and $p2.AppPath) { . $addExecutionEntry $p2.AppPath 'RecentApps' }
                                    } catch { $categoryPartial = $true }
                                }
                            }
                            } catch { $categoryPartial = $true }

                            $shimCachePath = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\AppCompatCache'
                            try {
                            if (Test-CleanupSummaryPath $shimCachePath) {
                                $shimData = Get-ItemProperty -LiteralPath $shimCachePath -ErrorAction Stop
                                if ($shimData -and $shimData.AppCompatCache) {
                                    . $addExecutionEntry "ShimCache (Binary Blob - $($shimData.AppCompatCache.Length) bytes)" 'ShimCache'
                                }
                            }
                            } catch { $categoryPartial = $true }

                            $count = $seen.Count
                        } else { $items = @("[registry unavailable]") }
                    }
                    'ntUserTraces' {
                        if ($hiveAvailable) {
                            # TypedURLs lives under Internet Explorer, not Explorer.
                            $traceSources = @(
                                @{ section='RunMRU'; path="$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU" },
                                @{ section='TypedPaths'; path="$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths" },
                                @{ section='TypedURLs'; path="$hiveRoot\Software\Microsoft\Internet Explorer\TypedURLs" },
                                @{ section='WordWheelQuery'; path="$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\WordWheelQuery" },
                                @{ section='ComDlg32\OpenSavePidlMRU'; path="$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\OpenSavePidlMRU" },
                                @{ section='ComDlg32\LastVisitedPidlMRU'; path="$hiveRoot\Software\Microsoft\Windows\CurrentVersion\Explorer\ComDlg32\LastVisitedPidlMRU" }
                            )
                            foreach ($source in $traceSources) {
                                if (Test-CleanupSummaryPath $source.path) {
                                    $props = (Get-Item -LiteralPath $source.path -EA Stop).Property |
                                        Where-Object { $_ -ne '(default)' -and -not ($source.section -eq 'RunMRU' -and $_ -match '^MRUList') }
                                    foreach ($prop in @($props)) {
                                        $raw = Get-ItemPropertyValue -LiteralPath $source.path -Name $prop -EA Stop
                                        $original = $raw
                                        if ($source.section -eq 'RunMRU' -and $raw -is [string]) { $raw = $raw -replace '\\1$', '' }
                                        $count++
                                        if ($records.Count -lt $previewMax) {
                                            # Preserve binary bytes and empty values; display may decode text separately.
                                            $record = @{ section=$source.section; key=$prop; value=$raw }
                                            if ($source.section -eq 'RunMRU') { $record.rawValue = $original }
                                            $records += $record
                                            $items += "$($source.section)\$prop"
                                        }
                                    }
                                }
                            }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'netDrives' {
                        if ($hiveAvailable) {
                            $k = "$hiveRoot\Network"
                            if (Test-CleanupSummaryPath $k) {
                                $drives = @(Get-ChildItem -LiteralPath $k -EA Stop)
                                $count = @($drives).Count
                                foreach ($drive in $drives) {
                                    if ($records.Count -ge $previewMax) { break }
                                    $row = @{ name = [string]$drive.PSChildName; path = [string]$drive.PSPath }
                                    try {
                                        $mapping = Get-ItemProperty -LiteralPath $drive.PSPath -ErrorAction Stop
                                        foreach ($property in @($mapping.PSObject.Properties)) {
                                            if ($property.Name -in @('PSPath', 'PSParentPath', 'PSChildName', 'PSDrive', 'PSProvider')) { continue }
                                            $row[$property.Name] = $property.Value
                                        }
                                        if ($null -ne $mapping.RemotePath) { $row.DisplayRoot = $mapping.RemotePath }
                                    } catch { $categoryPartial = $true; $row.readStatus = 'unavailable' }
                                    $records += $row
                                    $items += [string]$drive.PSChildName
                                }
                            }
                        } else { $items = @("[registry unavailable]") }
                    }
                    'officeMru' {
                        if ($hiveAvailable) {
                            $office = Get-CleanupOfficeMruInventory -RegistryRoot $hiveRoot -PreviewLimit $previewMax
                            $count = $office.total
                            $records = @($office.files)
                            $items = @($records | ForEach-Object { $_.name })
                            $categoryPartial = [bool]$office.partial
                        } else { $items = @("[registry unavailable]") }
                    }
                    'explorerSearchHistory' {
                        if ($hiveAvailable) {
                            $history = Get-CleanupExplorerSearchInventory -RegistryRoot $hiveRoot -PreviewLimit $previewMax
                            $count = $history.total
                            $records = @($history.files)
                            $items = @($records | ForEach-Object { $_.name })
                            $categoryPartial = [bool]$history.partial
                        } else { $items = @("[registry unavailable]") }
                    }
                }
                } # end if (-not $delegated)
                if ($count -lt 0) { throw 'The category scan returned an invalid count.' }
            } catch {
                $scanError = $_.Exception.Message
                # Keep observations already obtained from this profile and
                # category. The failed count must remain unknown so they can
                # be inspected without authorizing a cleanup.
                $count = -1
            }
            if (@($records).Count -eq 0 -and @($items).Count -gt 0) {
                $records = @($items | Select-Object -First $previewMax | ForEach-Object {
                    [pscustomobject]@{ entry = [string]$_ }
                })
            }
            $detailPage = $null
            if ($FullDetail -and -not $scanError) {
                try { $detailPage = Get-CleanupSummaryPage -Records @($records) -Count $count -Category $cat -ProfileSid $prof.SID -Offset $DetailOffset -ExpectedSnapshot $SnapshotHash -ChunkOffset $DetailChunkOffset }
                catch { $scanError = $_.Exception.Message; $count = -1; $items = @(); $records = @() }
            }
            $categoryRecord = New-CleanupSummaryCategoryRecord -Category $cat -Count $count -Items $items -Records $records -ScanError $scanError -previewMax $previewMax
            if ($categoryPartial -and -not $scanError) {
                $categoryRecord.partial = $true; $categoryRecord.status = 'partial'
                $categoryRecord.message = 'Some sources could not be checked.'
                if ($cat -eq 'sshState' -and -not $isCurrentUser) { $categoryRecord.message = 'SSH agent state cannot be checked for another account.' }
                if ($cat -eq 'wslData' -and -not $isCurrentUser) { $categoryRecord.message = 'Only this account’s registered WSL distributions were checked; disks and provider-managed data were not inspected.'; $categoryRecord.coverage = 'Selected registry registrations only' }
                if ($cat -eq 'dockerDesktopData' -and -not $isCurrentUser) { $categoryRecord.message = if ($dockerUnavailable) { 'Some default Docker locations could not be checked; custom locations and other engines were not inspected.' } else { 'Only selected profile Docker defaults were checked; custom locations and other engines were not inspected.' }; $categoryRecord.coverage = 'Selected profile defaults only' }
                if ($cat -eq 'passwordManagerCaches' -and -not $isCurrentUser) { $categoryRecord.message = if ($cacheUnavailable) { 'Some selected cache locations could not be checked; other locations are not included.' } else { 'Only selected password-manager cache locations were checked; other locations may not be included.' }; $categoryRecord.coverage = 'Selected locations only' }
            }
            if ($delegated -and $r -and -not $scanError) {
                if ($r.coverage -is [string] -and $r.coverage.Trim()) { $categoryRecord.coverage = $r.coverage }
                if ($r.partial -eq $true -or $r.complete -eq $false -or $r.status -eq 'partial' -or $r.coverage -eq 'partial' -or $r.accessDenied -eq $true) {
                    $categoryRecord.partial = $true; $categoryRecord.status = 'partial'
                }
                $facts = @{}
                foreach ($key in $r.Keys) {
                    if ($key -in @('error','message','status','partial','complete','coverage','accessDenied','total','count','sampleCount','previewLimit','truncated','scannedAt','scanTime')) { continue }
                    $value = $r[$key]
                    if ($value -is [string] -or $value -is [bool] -or $value -is [ValueType]) { $facts[$key] = $value }
                }
                if ($facts.Count) { $categoryRecord.scanFacts = $facts }
            }
            if ($detailPage) {
                $categoryRecord.records = $detailPage.records; $categoryRecord.items = @(); $categoryRecord.sampleCount = @($detailPage.records).Count
                $categoryRecord.sourceTruncated = $categoryRecord.truncated
                $categoryRecord.truncated = $categoryRecord.truncated -or ($null -ne $detailPage.nextOffset) -or ($null -ne $detailPage.recordChunk)
                $categoryRecord.snapshotHash = $detailPage.snapshotHash; $categoryRecord.nextOffset = $detailPage.nextOffset; $categoryRecord.totalRows = $detailPage.totalRows
                if ($detailPage.recordChunk) { $categoryRecord.recordChunk = $detailPage.recordChunk }
            }
            $categories += $categoryRecord
        }

        # Unload hive if we loaded it
        if ($hiveLoaded) {
            [System.GC]::Collect(); [System.GC]::WaitForPendingFinalizers()
            $null = & reg unload $mountPath 2>&1
        }

        $knownTotal = [long]0
        foreach ($category in $categories) { if (-not $category.error) { $knownTotal += $category.count } }
        $isComplete = @($categories | Where-Object { $_.error -or $_.partial }).Count -eq 0
        $totalCount = if ($isComplete) { $knownTotal } else { -1 }
        $entry = @{
            username      = $username
            path          = $prof.FullName
            sid           = $prof.SID
            isCurrentUser = $isCurrentUser
            isLoggedIn    = $isLoggedIn
            total         = $totalCount
            knownTotal    = $knownTotal
            complete      = $isComplete
            status        = if ($isComplete) { 'complete' } else { 'partial' }
            categories    = $categories
            hiveAvailable = $hiveAvailable
        }
        if ($hiveError) { $entry.hiveError = $hiveError }
        $userResults += $entry
    }

    @{ status = 'ok'; users = $userResults; userCount = $userResults.Count; isAdmin = [bool]$isAdmin }
}
