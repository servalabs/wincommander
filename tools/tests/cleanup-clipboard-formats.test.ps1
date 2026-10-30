$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Convert-CleanupClipboardItemDetails', 'Convert-CleanupClipboardHistoryResult', 'Convert-CleanupClipboardImageStream', 'Get-ClipboardHistoryStatus')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($definition.Extent.Text))
}
$item = [pscustomobject]@{ Id='fixture'; Timestamp='2026-01-01T00:00:00Z'; Content=[pscustomobject]@{ AvailableFormats=@('Text','HTML Format','Rich Text Format','Bitmap') } }
$formats = @{ text='Text'; html='HTML Format'; rtf='Rich Text Format' }
$details = Convert-CleanupClipboardItemDetails -Item $item -FormatNames $formats -ReadTextFormat { param($entry,$name) switch ($name) { 'text' { 'fixture text' } 'html' { '<p>fixture</p>' } 'rtf' { '{\rtf1 fixture}' } } }
if ($details.text -ne 'fixture text' -or $details.html -ne '<p>fixture</p>' -or $details.rtf -ne '{\rtf1 fixture}' -or $details.formats -notcontains 'Bitmap' -or $details.id -ne 'fixture') { throw 'Clipboard detail lost an available format.' }
$failed = Convert-CleanupClipboardItemDetails -Item $item -FormatNames $formats -ReadTextFormat { param($entry,$name) if ($name -eq 'html') { throw 'fixture' }; 'available' }
if ($failed.unavailableFormats -notcontains 'HTML Format' -or $failed.text -ne 'available' -or $failed.rtf -ne 'available') { throw 'One unavailable format discarded other available content.' }
$richItem = [pscustomobject]@{ Id='rich'; Timestamp=''; Content=[pscustomobject]@{ AvailableFormats=@('HTML Format') } }
$rich = Convert-CleanupClipboardItemDetails -Item $richItem -FormatNames $formats -ReadTextFormat { param($entry,$name) '<p>fixture</p>' }
if ($rich.html -ne '<p>fixture</p>' -or $rich.type -ne 'rich text' -or $rich.ContainsKey('preview')) { throw 'Available rich text must not be labeled as requiring another viewer.' }
$bitmapItem = [pscustomobject]@{ Id='bitmap'; Timestamp=''; Content=[pscustomobject]@{ AvailableFormats=@('Bitmap') } }
$bitmapDetails = Convert-CleanupClipboardItemDetails -Item $bitmapItem -FormatNames $formats -ReadTextFormat { throw 'Unexpected text read.' }
if ($bitmapDetails.type -ne 'image' -or $bitmapDetails.ContainsKey('preview')) { throw 'A loadable bitmap must not be labeled as requiring another viewer.' }
$unknownItem = [pscustomobject]@{ Id='unknown'; Timestamp=''; Content=[pscustomobject]@{ AvailableFormats=@('CustomFormat') } }
$unknown = Convert-CleanupClipboardItemDetails -Item $unknownItem -FormatNames $formats -ReadTextFormat { throw 'Unexpected text read.' }
if (-not $unknown.contentStatus -or $unknown.ContainsKey('preview')) { throw 'Unsupported formats must be reported as unavailable.' }
$minimal = Convert-CleanupClipboardHistoryResult -Result ([pscustomobject]@{ Status='Success'; Items=@($item) })
if ($minimal.items[0].ContainsKey('formats') -or $minimal.items[0].ContainsKey('text')) { throw 'Non-preview inventory must not include clipboard content.' }
$png = [Convert]::FromBase64String('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/aG8AAAAASUVORK5CYII=')
$image = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new($png))
if ($image.status -ne 'complete' -or $image.imageMime -ne 'image/png' -or $image.imageBase64 -ne [Convert]::ToBase64String($png)) { throw 'A supported image did not round trip.' }
Add-Type -AssemblyName System.Runtime.WindowsRuntime -ErrorAction Stop
$winrtStream = [Windows.Storage.Streams.InMemoryRandomAccessStream, Windows.Storage.Streams, ContentType=WindowsRuntime]::new()
$writeStream = [IO.WindowsRuntimeStreamExtensions]::AsStream($winrtStream)
try {
    $writeStream.Write($png, 0, $png.Length); $writeStream.Flush(); $writeStream.Position = 0
    $readStream = [IO.WindowsRuntimeStreamExtensions]::AsStreamForRead($winrtStream)
    try { $winrtImage = Convert-CleanupClipboardImageStream -Stream $readStream } finally { $readStream.Dispose() }
    if ($winrtImage.status -ne 'complete' -or $winrtImage.imageBase64 -ne [Convert]::ToBase64String($png)) { throw 'WinRT stream did not round trip.' }
} finally { $writeStream.Dispose(); $winrtStream.Dispose() }
$tooLarge = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new((New-Object byte[] 9))) -MaxBytes 8
if ($tooLarge.status -ne 'too-large' -or $tooLarge.ContainsKey('imageBase64')) { throw 'An over-limit image must not be sent.' }
$unsupported = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new([Text.Encoding]::UTF8.GetBytes('<svg/>')))
if ($unsupported.status -ne 'unsupported' -or $unsupported.ContainsKey('imageBase64')) { throw 'Unsupported image data must not be sent.' }
$large = New-Object byte[] 300000
[Array]::Copy($png, $large, $png.Length)
$first = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new($large))
if ($first.status -ne 'complete' -or $first.nextOffset -ne 262144 -or $first.totalBytes -ne 300000 -or ([Convert]::FromBase64String($first.imageBase64)).Length -ne 262144 -or $first.snapshotHash -notmatch '^[0-9a-f]{64}$') { throw 'Image first chunk is invalid.' }
$last = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new($large)) -Offset $first.nextOffset -ExpectedHash $first.snapshotHash
if ($last.status -ne 'complete' -or $null -ne $last.nextOffset -or $last.snapshotHash -ne $first.snapshotHash -or ([Convert]::FromBase64String($last.imageBase64)).Length -ne (300000 - 262144)) { throw 'Image continuation is invalid.' }
$large[299999] = 1
$changed = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new($large)) -Offset $first.nextOffset -ExpectedHash $first.snapshotHash
if ($changed.status -ne 'changed' -or $changed.ContainsKey('imageBase64')) { throw 'Changed clipboard content must not be mixed.' }
$missingHash = Convert-CleanupClipboardImageStream -Stream ([IO.MemoryStream]::new($large)) -Offset 262144
if ($missingHash.status -ne 'unavailable') { throw 'Continuation must require the original snapshot hash.' }
function Get-CleanupClipboardImage { param($ItemId, $Offset, $ExpectedHash) return @{ status='fixture'; id=$ItemId; offset=$Offset; hash=$ExpectedHash } }
$route = Get-ClipboardHistoryStatus -ImageItemId 'fixture' -ImageOffset 262144 -ImageSnapshot ('a' * 64)
if ($route.status -ne 'fixture' -or $route.id -ne 'fixture' -or $route.offset -ne 262144 -or $route.hash -ne ('a' * 64)) { throw 'The existing clipboard command did not route the selected image request.' }
Write-Output 'Passed: local clipboard formats and metadata-only inventory.'
