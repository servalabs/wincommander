# SPDX-License-Identifier: AGPL-3.0-or-later
# Share Safely format corpus: every common Documents/Pictures file type, renamed
# images, empty, read-only, locked, unicode and long paths. Builds the corpus in
# a disposable temp folder, checks that the per-file report names each file with
# the expected reason and detected type, then deletes the corpus.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$tokens = $null; $errors = $null
$source = Join-Path $PSScriptRoot '../src-tauri/commander-free/scripts/modules/dependencies/dependencies.ps1'
$ast = [Management.Automation.Language.Parser]::ParseFile($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'dependencies.ps1 parse failed' }
$fn = @($ast.EndBlock.Statements | Where-Object { $_ -is [Management.Automation.Language.FunctionDefinitionAst] -and $_.Name -eq 'Get-ScrubEngineVersion' })
if ($fn.Count -ne 1) { throw 'Get-ScrubEngineVersion not found' }
. ([scriptblock]::Create($fn[0].Extent.Text))
$qpdfOut = @('qpdf version 12.4.2', 'Run qpdf --copyright to see copyright and license information.')
if ((Get-ScrubEngineVersion $qpdfOut 'qpdf version (\d+(?:\.\d+)+)') -ne '12.4.2') { throw 'qpdf version not parsed' }
if ((Get-ScrubEngineVersion @('13.59') '^\s*(\d+(?:\.\d+)+)') -ne '13.59') { throw 'ExifTool version not parsed' }
if ($null -ne (Get-ScrubEngineVersion @() '^\s*(\d+(?:\.\d+)+)')) { throw 'Missing version must be null' }

$base = Join-Path ([IO.Path]::GetTempPath()) ('wc-scrub-corpus-' + [guid]::NewGuid().ToString('N'))
$expected = [Collections.Generic.List[string]]::new()
function Add-CorpusFile([string]$Rel, [byte[]]$Bytes, [string]$Reason, [string]$Detected) {
    $path = Join-Path $base $Rel
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllBytes($path, $Bytes)
    $expected.Add("$Rel`t$Reason`t$Detected")
}
function Get-Ascii([string]$Text) { [Text.Encoding]::UTF8.GetBytes($Text) }
function Join-Bytes { param([Parameter(ValueFromRemainingArguments)][object[]]$Parts) [byte[]]($Parts | ForEach-Object { if ($_ -is [string]) { Get-Ascii $_ } else { $_ } } | ForEach-Object { $_ }) }
function Get-Bmff([string]$Brand) { Join-Bytes ([byte[]](0, 0, 0, 24)) "ftyp$Brand" ([byte[]](0, 0, 0, 0)) "mif1$Brand" }
function Get-Zip([hashtable[]]$Entries) {
    $ms = [IO.MemoryStream]::new()
    $zip = [IO.Compression.ZipArchive]::new($ms, [IO.Compression.ZipArchiveMode]::Create, $true)
    foreach ($e in $Entries) {
        $level = if ($e.ContainsKey('Stored')) { [IO.Compression.CompressionLevel]::NoCompression } else { [IO.Compression.CompressionLevel]::Optimal }
        $w = [IO.StreamWriter]::new($zip.CreateEntry($e.Name, $level).Open())
        $w.Write($e.Text); $w.Dispose()
    }
    $zip.Dispose()
    , $ms.ToArray()
}

$jpeg = [byte[]](0xFF, 0xD8, 0xFF, 0xE0, 0, 16) + (Get-Ascii 'JFIF') + [byte[]](0, 1, 1, 0, 0, 1, 0, 1, 0, 0, 0xFF, 0xD9)
$png = [byte[]](0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13) + (Get-Ascii 'IHDR') + [byte[]]::new(17)
$webp = (Join-Bytes 'RIFF' ([byte[]](26, 0, 0, 0)) 'WEBPVP8L') + [byte[]]::new(14)
$ooxml = { param($dir) Get-Zip @(@{ Name = '[Content_Types].xml'; Text = '<Types/>' }, @{ Name = "$dir/main.xml"; Text = '<x/>' }, @{ Name = 'docProps/core.xml'; Text = '<cp:coreProperties/>' }) }

$lock = $null
try {
    Add-CorpusFile 'Pictures\camera photo.jpg' $jpeg 'unsupported_format' 'jpeg'
    Add-CorpusFile 'Pictures\scan.jpeg' $jpeg 'unsupported_format' 'jpeg'
    Add-CorpusFile 'Pictures\screenshot.png' $png 'unsupported_format' 'png'
    Add-CorpusFile 'Pictures\sticker.webp' $webp 'unsupported_format' 'webp'
    Add-CorpusFile 'Pictures\anim.gif' ((Get-Ascii 'GIF89a') + [byte[]]::new(16)) 'unsupported_format' 'gif'
    Add-CorpusFile 'Pictures\legacy.bmp' ((Get-Ascii 'BM') + [byte[]]::new(52)) 'nothing_to_remove' 'bmp'
    Add-CorpusFile 'Pictures\print.tiff' ([byte[]](0x49, 0x49, 0x2A, 0, 8, 0, 0, 0)) 'unsupported_format' 'tiff'
    Add-CorpusFile 'Pictures\raw-shot.dng' ([byte[]](0x49, 0x49, 0x2A, 0, 8, 0, 0, 0)) 'unsupported_format' 'tiff'
    Add-CorpusFile 'Pictures\iphone.heic' (Get-Bmff 'heic') 'unsupported_format' 'heic'
    Add-CorpusFile 'Pictures\modern.avif' (Get-Bmff 'avif') 'unsupported_format' 'avif'
    Add-CorpusFile 'Pictures\favicon.ico' ([byte[]](0, 0, 1, 0, 1, 0) + [byte[]]::new(16)) 'nothing_to_remove' 'ico'
    Add-CorpusFile 'Pictures\logo.svg' (Get-Ascii '<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg"/>') 'unsupported_format' 'svg'
    Add-CorpusFile 'Pictures\clip.mp4' (Get-Bmff 'isom') 'unsupported_format' 'mp4'
    Add-CorpusFile 'Pictures\clip.mov' (Get-Bmff 'qt  ') 'unsupported_format' 'mov'
    Add-CorpusFile 'Pictures\movie.mkv' ([byte[]](0x1A, 0x45, 0xDF, 0xA3, 0xA3) + [byte[]]::new(8)) 'unsupported_format' 'mkv'
    Add-CorpusFile 'Pictures\WhatsApp Image 2026-11-01.jpg' $png 'content_mismatch' 'png'
    Add-CorpusFile 'Pictures\IMG-20261101-WA0001.jpg' $webp 'content_mismatch' 'webp'
    Add-CorpusFile 'Pictures\export.png' $jpeg 'content_mismatch' 'jpeg'
    Add-CorpusFile 'Pictures\empty.jpg' ([byte[]]@()) 'empty_file' '-'
    Add-CorpusFile "Pictures\Trip $([char]0x2708) $([char]0x6771)$([char]0x4EAC)\caf$([char]0xE9) photo 1.jpg" $jpeg 'unsupported_format' 'jpeg'
    Add-CorpusFile 'Documents\report.pdf' (Get-Ascii "%PDF-1.4`n%%EOF`n") 'unsupported_format' 'pdf'
    Add-CorpusFile 'Documents\letter.docx' (& $ooxml 'word') 'unsupported_format' 'docx'
    Add-CorpusFile 'Documents\budget.xlsx' (& $ooxml 'xl') 'unsupported_format' 'xlsx'
    Add-CorpusFile 'Documents\deck.pptx' (& $ooxml 'ppt') 'unsupported_format' 'pptx'
    Add-CorpusFile 'Documents\essay.odt' (Get-Zip @(@{ Name = 'mimetype'; Text = 'application/vnd.oasis.opendocument.text'; Stored = $true }, @{ Name = 'meta.xml'; Text = '<meta/>' })) 'unsupported_format' 'odt'
    Add-CorpusFile 'Documents\archive.zip' (Get-Zip @(@{ Name = 'inner.txt'; Text = 'x' })) 'unsupported_format' 'zip'
    Add-CorpusFile 'Documents\memo.rtf' (Get-Ascii '{\rtf1\ansi{\info{\author A}}Hi}') 'unsupported_format' 'rtf'
    Add-CorpusFile 'Documents\notes.txt' (Get-Ascii "plain notes`r`n") 'nothing_to_remove' 'text'
    Add-CorpusFile 'Documents\data.csv' (Get-Ascii "a,b`r`n1,2`r`n") 'nothing_to_remove' 'text'
    Add-CorpusFile 'Documents\readme.md' (Get-Ascii "# Title`n") 'nothing_to_remove' 'text'
    Add-CorpusFile 'Documents\config.json' (Get-Ascii '{"a": 1}') 'nothing_to_remove' 'text'
    Add-CorpusFile 'Documents\song.mp3' ((Get-Ascii 'ID3') + [byte[]](3, 0, 0, 0, 0, 0, 0, 0xFF, 0xFB, 0x90, 0)) 'unsupported_format' 'mp3'
    Add-CorpusFile 'Documents\recording.wav' (Join-Bytes 'RIFF' ([byte[]](36, 0, 0, 0)) 'WAVEfmt ' ([byte[]]::new(8))) 'unsupported_format' 'wav'
    Add-CorpusFile 'Documents\track.flac' ((Get-Ascii 'fLaC') + [byte[]]::new(38)) 'unsupported_format' 'flac'
    Add-CorpusFile 'Documents\voice memo.m4a' (Get-Bmff 'M4A ') 'unsupported_format' 'm4a'
    Add-CorpusFile 'Documents\readonly.jpg' $jpeg 'unsupported_format' 'jpeg'
    Add-CorpusFile 'Documents\locked.jpg' $jpeg 'locked' '-'
    $deep = (@('very long folder name for path testing') * 7) -join '\'
    Add-CorpusFile "Documents\$deep\deep photo.jpg" $jpeg 'unsupported_format' 'jpeg'
    if ((Join-Path $base "Documents\$deep\deep photo.jpg").Length -le 260) { throw 'Long-path fixture is not long' }
    # Earlier output must not be reported as input.
    [IO.Directory]::CreateDirectory((Join-Path $base 'Documents\_scrubbed')) | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $base 'Documents\_scrubbed\old output.jpg'), $jpeg)
    Set-ItemProperty -LiteralPath (Join-Path $base 'Documents\readonly.jpg') -Name IsReadOnly -Value $true
    [IO.File]::WriteAllLines((Join-Path $base 'expected.tsv'), $expected)
    $lock = [IO.File]::Open((Join-Path $base 'Documents\locked.jpg'), 'Open', 'ReadWrite', 'None')
    $env:WC_SCRUB_CORPUS = $base
    Push-Location (Join-Path $PSScriptRoot '../src-tauri')
    try { cargo test -p commander-free --lib corpus_every_file_has_an_expected_outcome -- --ignored }
    finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "Corpus outcome test failed ($LASTEXITCODE)" }
    Write-Host "Scrub format corpus passed: $($expected.Count) files"
} finally {
    if ($lock) { $lock.Dispose() }
    Remove-Item Env:WC_SCRUB_CORPUS -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $base) {
        Get-ChildItem -LiteralPath $base -Recurse -File | ForEach-Object { $_.IsReadOnly = $false }
        Remove-Item -LiteralPath $base -Recurse -Force
    }
}
