$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Assert-CleanupDeveloperCacheAncestors', 'Get-CleanupDeveloperCachesInfo', 'Get-DeveloperCachesInfo')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Developer cache scanner function $name is missing." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

$oldApp = $env:APPDATA; $oldLocal = $env:LOCALAPPDATA; $oldProfile = $env:USERPROFILE
try {
    $env:APPDATA = 'C:\Fixture\Roaming'; $env:LOCALAPPDATA = 'C:\Fixture\Local'; $env:USERPROFILE = 'C:\Fixture\User'
    $script:deny = $false; $script:denyRoot = $false; $script:reparseRoot = $false; $script:reparseAncestor = $false; $script:reparseFile = $false; $script:allMissing = $false; $script:denyConfig = $false; $script:reparseConfig = $false; $script:includeConfig = $false
    function Get-Item {
        param($LiteralPath, [switch]$Force, $ErrorAction)
        if ($LiteralPath -eq 'C:\Fixture\Roaming\npm-cache' -and -not $script:allMissing) {
            if ($script:denyRoot) { throw [UnauthorizedAccessException]::new('Synthetic root access denied.') }
            return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=if ($script:reparseRoot) { [IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Directory } }
        }
        if ($LiteralPath -in @('C:\Fixture\Roaming', 'C:\Fixture\User')) {
            return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=if ($script:reparseAncestor) { [IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Directory } }
        }
        if ($LiteralPath -eq 'C:\Fixture\User\.npmrc' -and ($script:denyConfig -or $script:reparseConfig -or $script:includeConfig)) {
            if ($script:denyConfig) { throw [UnauthorizedAccessException]::new('Synthetic configuration access denied.') }
            return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$false; Attributes=if ($script:reparseConfig) { [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Normal }; Length=0; LastWriteTime=[datetime]'2026-01-01' }
        }
        throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic path is absent.')
    }
    function Get-ChildItem {
        param($LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
        if ($script:deny) { throw [UnauthorizedAccessException]::new('Synthetic access denied.') }
        if ($LiteralPath -ne 'C:\Fixture\Roaming\npm-cache') { throw 'Unexpected cache source.' }
        [pscustomobject]@{ FullName = "$LiteralPath\nested"; PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
        for ($i = 0; $i -lt 201; $i++) { [pscustomobject]@{ FullName = "$LiteralPath\file-$i"; Attributes=if ($i -eq 0 -and $script:reparseFile) { [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Normal }; Length = 1024; LastWriteTime = [datetime]'2026-01-01' } }
    }
    $result = Get-DeveloperCachesInfo
    if ($result.error -or $result.partial -or $result.coverage -ne 'limited' -or $result.total -ne 201 -or $result.files.Count -ne 200 -or -not $result.truncated) { throw 'Cache count or preview coverage is incorrect.' }
    if (@($result.files | Where-Object { -not $_.path }).Count) { throw 'A returned cache record omitted its location.' }
    $full = Get-CleanupDeveloperCachesInfo -AppDataPath $env:APPDATA -LocalAppDataPath $env:LOCALAPPDATA -UserProfilePath $env:USERPROFILE -previewLimit ([int]::MaxValue)
    if ($full.error -or $full.total -ne 201 -or $full.files.Count -ne 201 -or $full.truncated) { throw 'Full cache detail did not include records beyond the normal preview.' }
    $script:deny = $true
    if (-not (Get-DeveloperCachesInfo).partial) { throw 'A denied cache inventory must not be reported complete.' }
    $script:deny = $false; $script:allMissing = $true
    $empty = Get-DeveloperCachesInfo
    if ($empty.error -or $empty.partial -or $empty.coverage -ne 'limited' -or $empty.total -ne 0) { throw 'Absent cache and configuration paths should produce a limited empty scan.' }
    $script:allMissing = $false; $script:denyRoot = $true
    if (-not (Get-DeveloperCachesInfo).partial) { throw 'An inaccessible cache root must not be treated as absent.' }
    $script:denyRoot = $false; $script:reparseRoot = $true
    if (-not (Get-DeveloperCachesInfo).partial) { throw 'A reparse-point cache root must not be followed.' }
    $script:reparseRoot = $false; $script:reparseFile = $true
    if (-not (Get-DeveloperCachesInfo).partial) { throw 'A reparse-point cache entry must not be reported as an ordinary file.' }
    $script:reparseFile = $false; $script:reparseAncestor = $true
    if (-not (Get-DeveloperCachesInfo).partial) { throw 'A reparse-point cache ancestor must not be followed.' }
    $script:reparseAncestor = $false; $script:denyConfig = $true
    $partial = Get-DeveloperCachesInfo
    if (-not $partial.partial -or $partial.total -ne 201 -or @($partial.files).Count -ne 200 -or -not $partial.truncated) { throw 'A denied configuration must retain earlier cache observations and stay incomplete.' }
    $script:denyConfig = $false; $script:reparseConfig = $true
    if (-not (Get-DeveloperCachesInfo).partial) { throw 'A reparse-point config must not be followed.' }
    $script:reparseConfig = $false; $script:includeConfig = $true
    $withConfig = Get-CleanupDeveloperCachesInfo -AppDataPath $env:APPDATA -LocalAppDataPath $env:LOCALAPPDATA -UserProfilePath $env:USERPROFILE -previewLimit ([int]::MaxValue)
    if ($withConfig.error -or $withConfig.total -ne 202 -or @($withConfig.files | Where-Object { $_.path -eq 'C:\Fixture\User\.npmrc' }).Count -ne 1) { throw 'An existing regular configuration must be reported by presence.' }
    Write-Output 'Passed: developer cache paths, aggregate count and denied enumeration.'
} finally { $env:APPDATA = $oldApp; $env:LOCALAPPDATA = $oldLocal; $env:USERPROFILE = $oldProfile }
