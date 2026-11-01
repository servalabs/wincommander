$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup module parse failed.' }
foreach ($name in @('Get-FontCacheInfo', 'Get-LegacyIconCacheInfo', 'Get-FirewallLogInfo')) {
    $fn = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    . ([scriptblock]::Create($fn.Extent.Text))
}
$script:mode = 'missing'
function Get-Item {
    param([string]$LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:mode -eq 'missing') { throw [Management.Automation.ItemNotFoundException]::new('absent') }
    if ($script:mode -eq 'denied' -or ($script:mode -eq 'partial' -and $LiteralPath -match 'blocked')) { throw [UnauthorizedAccessException]::new('denied') }
    [pscustomobject]@{ Name='example.dat'; FullName=$LiteralPath; PSIsContainer=($LiteralPath -match 'FontCache$'); Length=1024; LastWriteTime=[datetime]'2026-01-01' }
}
function Get-ChildItem {
    param([string]$LiteralPath, [switch]$Recurse, [switch]$File, [string]$Filter, [switch]$Force, $ErrorAction)
    [pscustomobject]@{ Name='font.dat'; FullName="$LiteralPath\font.dat"; Length=1024; LastWriteTime=[datetime]'2026-01-01' }
    if ($script:mode -eq 'partial') { throw [UnauthorizedAccessException]::new('denied after one file') }
}
function Get-NetFirewallProfile {
    param($ErrorAction)
    @([pscustomobject]@{ Name='Domain'; LogFileName='C:\readable.log' }, [pscustomobject]@{ Name='Private'; LogFileName='C:\blocked.log' })
}
$r=Get-FontCacheInfo; if ($r.total -ne 0) { throw 'Absent font cache is not empty.' }
$script:mode='partial'; $r=Get-FontCacheInfo
if (-not $r.partial -or $r.total -ne 1 -or -not $r.files[0].path.EndsWith('font.dat')) { throw 'Partial font cache lost its path or record.' }
$r=Get-FirewallLogInfo
if (-not $r.partial -or $r.total -ne 1 -or $r.files[0].path -ne 'C:\readable.log') { throw 'Partial firewall inventory lost its path or record.' }
$script:mode='denied'; $r=Get-LegacyIconCacheInfo
if (-not $r.error) { throw 'Denied icon cache was reported as empty.' }
$script:mode='complete'; $r=Get-LegacyIconCacheInfo
if ($r.total -ne 1 -or -not $r.files[0].path.EndsWith('IconCache.db')) { throw 'Icon cache path was not returned.' }
Write-Output 'Passed: font, icon and firewall scans retain paths and report access failures.'
