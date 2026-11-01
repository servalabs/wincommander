$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-OfficeTempFilesInfo' }, $true)
if (-not $definition) { throw 'Office temporary-file scanner is missing.' }
. ([scriptblock]::Create($definition.Extent.Text))

$originalTemp = $env:TEMP; $originalProfile = $env:USERPROFILE
try {
    $env:TEMP = 'C:\Fixture\Temp'; $env:USERPROFILE = 'C:\Fixture\Profile'
    $script:deny = $false
    function Get-Item {
        param($LiteralPath, [switch]$Force, $ErrorAction)
        if ($script:deniedRoot -and $LiteralPath -eq 'C:\Fixture\Temp') { throw [UnauthorizedAccessException]::new('Synthetic root access denied.') }
        if ($LiteralPath -ne 'C:\Fixture\Temp') { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent root.') }
        [pscustomobject]@{ PSIsContainer = $true; Attributes = [IO.FileAttributes]::Directory }
    }
    function Get-ChildItem {
        param($Path, [switch]$Recurse, [int]$Depth, [switch]$File, $Filter, $Include, [switch]$Force, $ErrorAction)
        if ($script:deny) { throw [UnauthorizedAccessException]::new('Synthetic access denied.') }
        if ($Path -ne 'C:\Fixture\Temp' -or $Filter -ne '*.tmp') { throw 'Unexpected scanner source.' }
        for ($i = 0; $i -lt 501; $i++) {
            [pscustomobject]@{ FullName = "C:\Fixture\Temp\file-$i.tmp"; Length = 1024; LastWriteTime = [datetime]'2026-01-01' }
        }
    }
    $result = Get-OfficeTempFilesInfo
    if ($result.error -or $result.total -ne 501 -or @($result.files | Where-Object { -not $_.path }).Count) { throw 'Temporary-file results were truncated or omitted paths.' }
    $script:deny = $true
    if (-not (Get-OfficeTempFilesInfo).error) { throw 'A denied enumeration must not look like a complete scan.' }
    $script:deny = $false; $script:deniedRoot = $true
    if (-not (Get-OfficeTempFilesInfo).error) { throw 'A denied root must not look like an absent folder.' }
    Write-Output 'Passed: more than 500 temporary-file records, paths and denied enumeration.'
} finally { $env:TEMP = $originalTemp; $env:USERPROFILE = $originalProfile }
