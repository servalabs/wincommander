$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path,[ref]$tokens,[ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
$fn=$ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Get-BITSQueueInfo'},$true)
. ([scriptblock]::Create($fn.Extent.Text))
$script:mode='complete'
function Get-BitsTransfer {
    param([switch]$AllUsers,$ErrorAction)
    if ($script:mode -eq 'deniedJobs') { throw [UnauthorizedAccessException]::new('denied') }
    [pscustomobject]@{ DisplayName='Synthetic job'; JobState='Transferring'; JobId='abc'; OwnerAccount='synthetic'; BytesTransferred=3; BytesTotal=10 }
    if ($script:mode -eq 'streamJobs') { throw [UnauthorizedAccessException]::new('denied after job') }
}
function Get-Item {
    param([string]$LiteralPath,[switch]$Force,$ErrorAction)
    if ($script:mode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('absent') }
    if ($script:mode -eq 'deniedFiles') { throw [UnauthorizedAccessException]::new('denied') }
    [pscustomobject]@{ PSIsContainer=$true }
}
function Get-ChildItem {
    param([string]$LiteralPath,[switch]$File,[string]$Filter,[switch]$Force,$ErrorAction)
    [pscustomobject]@{ Name='qmgr0.dat'; FullName="$LiteralPath\qmgr0.dat"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($script:mode -eq 'streamFiles') { throw [UnauthorizedAccessException]::new('denied after file') }
}
$r=Get-BITSQueueInfo
if ($r.partial -or $r.total -ne 2 -or $r.files[0].jobId -ne 'abc' -or $r.files[0].bytesTransferred -ne 3 -or -not $r.files[1].path.EndsWith('qmgr0.dat')) { throw 'Complete BITS result lost details.' }
$script:mode='deniedJobs'; $r=Get-BITSQueueInfo
if (-not $r.partial -or $r.total -ne 1 -or -not $r.files[0].path.EndsWith('qmgr0.dat')) { throw 'Job failure discarded queue file.' }
$script:mode='deniedFiles'; $r=Get-BITSQueueInfo
if (-not $r.partial -or $r.total -ne 1 -or $r.files[0].jobId -ne 'abc') { throw 'File failure discarded job.' }
$script:mode='streamJobs'; $r=Get-BITSQueueInfo
if (-not $r.partial -or $r.total -ne 2) { throw 'Job emitted before failure was lost.' }
$script:mode='streamFiles'; $r=Get-BITSQueueInfo
if (-not $r.partial -or $r.total -ne 2) { throw 'File emitted before failure was lost.' }
$script:mode='missing'; $r=Get-BITSQueueInfo
if ($r.partial -or $r.total -ne 1) { throw 'Absent queue directory should not be a failure.' }
$text=[IO.File]::ReadAllText((Resolve-Path $source).Path)
if ($text -notmatch "(?s)'bits_queue'.*?if \(\`$result\.error -or \`$result\.partial\)") { throw 'Fleet partial boundary missing.' }
Write-Output 'Passed: BITS queue preserves details and partial observations.'
