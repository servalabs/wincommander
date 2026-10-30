$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Unable to parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-NotepadStateFiles' }, $true)
if (-not $definition) { throw 'Missing Notepad scanner.' }
. ([scriptblock]::Create($definition.Extent.Text))

# Fake provider calls; no actual package or tab-state files are inspected.
$savedLocal = $env:LOCALAPPDATA
try {
    $env:LOCALAPPDATA = 'C:\Fixture\Local'
    $script:deny = $false
    $script:missing = $false
    function Get-Item {
        param([string]$LiteralPath, [switch]$Force, $ErrorAction)
        if ($script:deny) { throw [UnauthorizedAccessException]::new('Synthetic access denial.') }
        if ($script:missing) { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent path.') }
        [pscustomobject]@{ FullName = $LiteralPath; PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory }
    }
    function Get-ChildItem {
        param([string]$LiteralPath, [string]$Filter, [switch]$Directory, [switch]$File, [switch]$Recurse, [switch]$Force, $ErrorAction)
        if ($Directory) {
            if ($Filter -ne 'Microsoft.WindowsNotepad_*') { throw 'Unexpected package filter.' }
            return @([pscustomobject]@{ FullName = 'C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_one'; Attributes = [IO.FileAttributes]::Directory }, [pscustomobject]@{ FullName = 'C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_two'; Attributes = [IO.FileAttributes]::Directory })
        }
        if (-not $File -or -not $Recurse) { throw 'Notepad inventory must only return recursive files.' }
        return [pscustomobject]@{ Name = 'tab.bin'; FullName = "$LiteralPath\nested\tab.bin"; Length = 2048; LastWriteTime = [datetime]'2026-01-01' }
    }
    $result = Get-NotepadStateFiles
    if ($result.error -or $result.total -ne 2 -or $result.packageFound -ne $true -or @($result.files | Where-Object { $_.path -notlike '*\nested\tab.bin' }).Count) { throw 'Notepad scanner omitted files or a package.' }
    $script:missing = $true
    $result = Get-NotepadStateFiles
    if ($result.error -or $result.total -ne 0 -or $result.packageFound) { throw 'Absent packages must be reported as empty.' }
    $script:missing = $false; $script:deny = $true
    $result = Get-NotepadStateFiles
    if (-not $result.error) { throw 'An inaccessible package must not be reported as empty.' }
    Write-Output 'Passed: multi-package Notepad files, paths, empty and access failure fixtures.'
} finally { $env:LOCALAPPDATA = $savedLocal }
