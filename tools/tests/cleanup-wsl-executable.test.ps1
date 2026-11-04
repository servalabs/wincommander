$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-CleanupWslExecutable' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$script:mode = 'present'; $script:requested = ''
function Get-Item {
    [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
    $script:requested = $LiteralPath
    if ($script:mode -eq 'absent') { throw [System.Management.Automation.ItemNotFoundException]::new('Missing') }
    if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Denied') }
    [pscustomobject]@{ PSIsContainer = $false; Attributes = if ($script:mode -eq 'reparse') { [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Normal } }
}
$expectedDirectory = [Environment]::SystemDirectory
if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) { $expectedDirectory = Join-Path ([Environment]::GetFolderPath('Windows')) 'Sysnative' }
$present = Get-CleanupWslExecutable
if (-not $present.available -or $present.path -ine (Join-Path $expectedDirectory 'wsl.exe') -or $script:requested -ine $present.path) { throw 'WSL must resolve to the fixed Windows system executable.' }
$script:mode = 'absent'; $absent = Get-CleanupWslExecutable
if ($absent.available -or -not $absent.verifiedAbsent) { throw 'A missing system executable must be reported as absent.' }
foreach ($mode in @('denied', 'reparse')) {
    $script:mode = $mode; $unknown = Get-CleanupWslExecutable
    if ($unknown.available -or $unknown.verifiedAbsent) { throw "$mode must remain unverified, not absent." }
}
Write-Output 'Passed: fixed WSL system path, missing file and unverified file states.'
