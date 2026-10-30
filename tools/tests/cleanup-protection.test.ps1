$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
foreach ($name in @('Assert-CleanupCategoriesAllowed','Invoke-CleanupClearForUser','Invoke-CleanupClearAllUsers','Clear-BrowserFootprints')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing guard boundary: $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
function Get-CimInstance { throw 'Protected request reached profile enumeration.' }
function Remove-ItemSecure { throw 'Protected request reached erasure.' }
$ids = @('wlanProfiles','browser_footprints','NotepadState','wslData','dockerDesktopData','virtualMachineArtifacts','developerCaches','credentialManager','sshState','passwordManagerCaches')
foreach ($id in $ids) {
    foreach ($operation in @(
        { Invoke-CleanupClearForUser -UserProfilePath 'C:\Mock' -CategoryIds @('recentFiles',$id) },
        { Invoke-CleanupClearAllUsers -CategoryIds "recentFiles,$id" }
    )) {
        $blocked = $false
        try { & $operation } catch { if ($_.Exception.Message -like '*protected from cleanup*') { $blocked = $true } else { throw } }
        if (-not $blocked) { throw "Protected category was not blocked: $id" }
    }
    Assert-CleanupCategoriesAllowed @($id) -AllowDefaultExcluded
    $mixedBlocked = $false
    try { Assert-CleanupCategoriesAllowed @('recentFiles',$id) -AllowDefaultExcluded } catch {
        $mixedBlocked = $_.Exception.Message -like '*protected from cleanup*'
    }
    if (-not $mixedBlocked) { throw "Mixed request bypassed individual confirmation: $id" }
}
$blocked = $false
try { Clear-BrowserFootprints } catch { $blocked = $_.Exception.Message -like '*protected from cleanup*' }
if (-not $blocked) { throw 'Browser direct clearer was not blocked.' }
$blocked = $false
try { Invoke-CleanupClearAllUsers -CategoryIds 'browserFootprints' -AllowDefaultExcluded 'false' } catch {
    $blocked = $_.Exception.Message -like '*protected from cleanup*'
}
if (-not $blocked) { throw 'String false bypassed individual confirmation.' }
$blocked = $false
try { Invoke-CleanupClearAllUsers -CategoryIds 'browserFootprints' -AllowDefaultExcluded 'true' } catch {
    $blocked = $_.Exception.Message -like '*protected from cleanup*'
}
if (-not $blocked) { throw 'String true bypassed typed individual confirmation.' }
$reachedProfiles = $false
try { Invoke-CleanupClearAllUsers -CategoryIds 'browserFootprints' -AllowDefaultExcluded $true } catch {
    $reachedProfiles = $_.Exception.Message -like '*Windows user profiles could not be read*'
}
if (-not $reachedProfiles) { throw 'Explicit single-category confirmation did not reach the next boundary.' }
Write-Output 'Passed: ten exclusions block default and mixed requests; explicit single-category confirmation reaches only the mocked next boundary.'
