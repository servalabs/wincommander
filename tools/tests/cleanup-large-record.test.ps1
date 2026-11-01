$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module could not be parsed.' }
foreach ($name in @('Test-CleanupSummaryDetailSize', 'ConvertTo-CleanupCanonicalJson', 'Get-CleanupSummaryPage')) {
    $helper = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $helper) { throw "Missing helper $name" }
    . ([scriptblock]::Create($helper.Extent.Text))
}
$unicode = [char]::ConvertFromUtf32(0x1F9EA) + [char]::ConvertFromUtf32(0x4E16)
$record = [ordered]@{ command = ('a' * (3MB - 19)) + ([char]::ConvertFromUtf32(0x1F9EA) * 100) + ('a' * (5MB + 200)) + $unicode; nested = @{ value = $unicode } }
$records = @($record)
$first = Get-CleanupSummaryPage -Records $records -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot ''
if (-not $first.recordChunk -or @($first.records).Count -ne 0 -or $first.recordChunk.offset -ne 0) { throw 'The oversized record was not chunked.' }
$firstBytes = [Convert]::FromBase64String($first.recordChunk.data)
try { $null = [Text.UTF8Encoding]::new($false, $true).GetString($firstBytes); throw 'Fixture did not split a Unicode sequence.' }
catch [Text.DecoderFallbackException] { }
$stream = [IO.MemoryStream]::new()
try {
    $page = $first; $offset = 0
    do {
        $chunk = $page.recordChunk
        if ($chunk.offset -ne $offset -or $chunk.totalBytes -le 8MB -or $chunk.data.Length -gt 4194304) { throw 'Chunk bounds are incorrect.' }
        $bytes = [Convert]::FromBase64String($chunk.data)
        $stream.Write($bytes, 0, $bytes.Length)
        $offset += $bytes.Length
        if ($null -ne $chunk.nextByteOffset) {
            if ($chunk.nextByteOffset -ne $offset) { throw 'Chunk offsets are inconsistent.' }
            $page = Get-CleanupSummaryPage -Records $records -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot $first.snapshotHash -ChunkOffset $offset
        }
    } while ($null -ne $chunk.nextByteOffset)
    $decoded = [Text.UTF8Encoding]::new($false, $true).GetString($stream.ToArray()) | ConvertFrom-Json
    if ($decoded.command -cne $record.command -or $decoded.nested.value -cne $unicode -or $offset -ne $chunk.totalBytes) { throw 'The full Unicode record was not preserved.' }
    try { $null = Get-CleanupSummaryPage -Records @(@{ command = 'changed' }) -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-1' -Offset 0 -ExpectedSnapshot $first.snapshotHash -ChunkOffset 1; throw 'Changed data was accepted.' }
    catch { if ($_.Exception.Message -notmatch 'changed while loading') { throw } }
    try { $null = Get-CleanupSummaryPage -Records $records -Count 1 -Category 'psHistory' -ProfileSid 'S-1-5-21-2' -Offset 0 -ExpectedSnapshot $first.snapshotHash -ChunkOffset 1; throw 'A different account was accepted.' }
    catch { if ($_.Exception.Message -notmatch 'changed while loading') { throw } }
} finally { $stream.Dispose() }
Write-Output 'Passed: bounded oversized Unicode record chunks and snapshot identity.'
