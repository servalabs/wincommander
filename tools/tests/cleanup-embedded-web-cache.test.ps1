$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path,[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Cleanup module did not parse.' }
$definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-EmbeddedWebCacheInfo'},$true)
. ([scriptblock]::Create($definition.Extent.Text))
$savedLocal=$env:LOCALAPPDATA; $env:LOCALAPPDATA='C:\fixture'
try {
    function Get-Item {
        param($LiteralPath,[switch]$Force,$ErrorAction)
        if ($script:scenario -eq 'absent') { throw [Management.Automation.ItemNotFoundException]::new('Absent root') }
        if ($script:scenario -eq 'denied') { throw [UnauthorizedAccessException]::new('Denied root') }
        [pscustomobject]@{PSIsContainer=$true;Attributes=if($script:scenario -eq 'root-link'){[IO.FileAttributes]::ReparsePoint}else{[IO.FileAttributes]::Directory}}
    }
    function Get-ChildItem {
        param($LiteralPath,[switch]$Force,$ErrorAction)
        if ($LiteralPath -like '*junction*') { throw 'A junction was traversed.' }
        if ($script:scenario -eq 'empty') { return }
        if ($LiteralPath -eq 'C:\fixture') {
            [pscustomobject]@{Name='junction';FullName='C:\fixture\junction';PSIsContainer=$true;Attributes=[IO.FileAttributes]::ReparsePoint}
            [pscustomobject]@{Name='App';FullName='C:\fixture\App';PSIsContainer=$true;Attributes=[IO.FileAttributes]::Directory}
        } elseif ($LiteralPath -eq 'C:\fixture\App') {
            [pscustomobject]@{Name='EBWebView';FullName='C:\fixture\App\EBWebView';PSIsContainer=$true;Attributes=[IO.FileAttributes]::Directory;LastWriteTime=[datetime]'2026-01-01'}
        } elseif ($LiteralPath -eq 'C:\fixture\App\EBWebView') {
            if ($script:scenario -eq 'denied-cache') { throw [UnauthorizedAccessException]::new('Denied cache') }
            [pscustomobject]@{Name='data';FullName='C:\fixture\App\EBWebView\data';PSIsContainer=$false;Attributes=[IO.FileAttributes]::Normal;Length=2048}
        } else { throw 'Unexpected traversal.' }
    }
    foreach($scenario in @('absent','empty')) { $script:scenario=$scenario; $r=Get-EmbeddedWebCacheInfo; if($r.partial -or $r.total -ne 0) {throw "$scenario must be an empty limited scan."} }
    foreach($scenario in @('denied','root-link')) { $script:scenario=$scenario; $r=Get-EmbeddedWebCacheInfo; if(-not $r.partial) {throw "$scenario must be incomplete."} }
    $script:scenario='populated'; $r=Get-EmbeddedWebCacheInfo
    if($r.partial -or $r.total -ne 1 -or $r.files[0].sizeKB -ne 2 -or $r.skippedLinks -ne 1) { throw 'Populated scan lost a folder, size or skipped junction.' }
    $timed=Get-EmbeddedWebCacheInfo -MaxMilliseconds 0
    if(-not $timed.partial -or $timed.total -ne 0 -or $null -ne $timed.totalSizeMB -or $timed.message -notmatch 'reached its limit') { throw 'A time-limited scan must report incomplete coverage and unknown total size.' }
    $script:scenario='denied-cache'; $r=Get-EmbeddedWebCacheInfo
    if(-not $r.partial -or $r.total -ne 1 -or -not $r.files[0].sizeIncomplete -or $null -ne $r.files[0].sizeKB) { throw 'An inaccessible cache must keep the folder and leave its size unknown.' }
    'PASS: absent, empty, populated, denied and reparse WebView inventories.'
} finally { $env:LOCALAPPDATA=$savedLocal }
