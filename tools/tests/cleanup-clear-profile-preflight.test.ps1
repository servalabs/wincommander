$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Cleanup source did not parse.' }
foreach ($name in @('Test-WCVisibleUserProfile', 'Invoke-CleanupClearAllUsers')) {
    $definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing function $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
function Assert-CleanupCategoriesAllowed { param($CategoryIds, [switch]$AllowDefaultExcluded) }
$script:profileMode = 'denied'
$script:clearCalls = 0
$script:admin = $true
$script:currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
function Test-IsAdmin { $script:admin }
function Get-CimInstance {
    param($ClassName, $ErrorAction)
    if ($script:profileMode -eq 'denied') { throw 'synthetic profile access failure' }
    if ($script:profileMode -eq 'empty') { return }
    [pscustomobject]@{ Special = $false; SID = $script:currentSid; LocalPath = "$env:SystemDrive\Users\FixtureCurrent" }
    [pscustomobject]@{ Special = $false; SID = 'S-1-5-21-11-22-33-444'; LocalPath = "$env:SystemDrive\Users\FixtureOther" }
}
function Get-LoggedInUsers { @{ users = @() } }
function Invoke-CleanupClearForUser { $script:clearCalls++; @{ recentFiles = 'cleaned' } }
function Invoke-WithUserHive { $script:clearCalls++; @{ recentFiles = 'cleaned' } }
function Expect-NoCleanup($Targets, $Expected) {
    $script:clearCalls = 0
    $errorMessage = ''
    try { Invoke-CleanupClearAllUsers -CategoryIds 'recentFiles' -TargetUsers $Targets | Out-Null }
    catch { $errorMessage = $_.Exception.Message }
    if ($errorMessage -notlike "*$Expected*" -or $script:clearCalls -ne 0) { throw "Expected $Expected without cleanup; got $errorMessage and $script:clearCalls calls." }
}
Expect-NoCleanup '' 'profiles could not be read'
$script:profileMode = 'empty'
Expect-NoCleanup '' 'No supported user profiles'
$script:profileMode = 'normal'
Expect-NoCleanup ', , ' 'No valid user profiles'
Expect-NoCleanup 'FixtureCurrent,Missing' 'unavailable, ambiguous or not authorized'
$script:admin = $false
Expect-NoCleanup 'FixtureOther' 'unavailable, ambiguous or not authorized'
$script:clearCalls = 0
Expect-NoCleanup '' 'Administrator access is required to clean all user profiles'
$result = Invoke-CleanupClearAllUsers -CategoryIds 'recentFiles' -TargetUsers 'FixtureCurrent'
if ($result.cleaned -ne 1 -or $script:clearCalls -ne 1) { throw 'An authorized explicit current-user selection did not run.' }
Write-Output 'Passed: failed, empty, invalid, missing and unauthorized profile selections perform no cleanup.'
