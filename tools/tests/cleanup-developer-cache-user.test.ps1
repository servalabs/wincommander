$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupSummaryAllUsers', 'Assert-CleanupDeveloperCacheAncestors', 'Get-CleanupDeveloperCachesInfo', 'New-CleanupSummaryCategoryRecord')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}

# Mock the account and filesystem providers. No real profile or cache is read.
$fixtureUser = 'wc-cache-fixture'
$fixtureProfile = "$env:SystemDrive\Users\$fixtureUser"
$fixtureCache = "$fixtureProfile\AppData\Roaming\npm-cache"
$script:fixtureSid = 'S-1-5-21-999-999-999-999'; $script:isAdmin = $true
function Get-CimInstance { param($ClassName, $ErrorAction) [pscustomobject]@{ Special=$false; SID=$script:fixtureSid; LocalPath=$fixtureProfile } }
function Test-WCVisibleUserProfile { param($Name, $DisplayName, $Path) return $true }
function Test-IsAdmin { return $script:isAdmin }
function Get-LoggedInUsers { return @{ users=@($fixtureUser) } }
function Test-Path { param($Path, $PathType, $ErrorAction) return $Path -eq $fixtureCache }
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($LiteralPath -eq $fixtureCache) { return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory } }
    if ($LiteralPath -eq "$fixtureProfile\AppData\Roaming") { return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory } }
    throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic path is absent.')
}
function Get-ChildItem {
    param($LiteralPath, [switch]$Recurse, [switch]$File, [switch]$Force, $ErrorAction)
    if ($LiteralPath -ne $fixtureCache) { throw 'The scan used the wrong account path.' }
    for ($i = 0; $i -lt 501; $i++) { [pscustomobject]@{ FullName="$LiteralPath\file-$i"; Attributes=[IO.FileAttributes]::Normal; Length=1024; LastWriteTime=[datetime]'2026-01-01' } }
}
function Get-CleanupSummaryPage {
    param($Records, $Count, $Category, $ProfileSid, $Offset, $ExpectedSnapshot, $ChunkOffset)
    if ($Category -ne 'developerCaches' -or $ProfileSid -ne $script:fixtureSid) { throw 'The page was not bound to the selected account and card.' }
    return @{ records=@($Records); snapshotHash=('sha256:' + ('a' * 64)); nextOffset=$null; totalRows=@($Records).Count }
}

$preview = Get-CleanupSummaryAllUsers -CategoryIds 'developerCaches' -TargetUsers $fixtureUser
$previewCategory = @($preview.users)[0].categories[0]
if ($preview.error -or $previewCategory.error -or $previewCategory.count -ne 501 -or @($previewCategory.records).Count -ne 500 -or -not $previewCategory.truncated) { throw 'The normal account preview was not bounded and truthful.' }
$full = Get-CleanupSummaryAllUsers -CategoryIds 'developerCaches' -TargetUsers $fixtureUser -FullDetail $true
$fullCategory = @($full.users)[0].categories[0]
if ($full.error -or $fullCategory.error -or $fullCategory.count -ne 501 -or @($fullCategory.records).Count -ne 501 -or $fullCategory.truncated) { throw 'The full account view omitted cache records.' }
if (@($fullCategory.records | Where-Object { $_.path -notlike "$fixtureCache\*" }).Count) { throw 'A returned record belonged to a different account.' }
$unauthorized = Get-CleanupSummaryAllUsers -CategoryIds 'developerCaches' -TargetUsers 'not-the-selected-user' -FullDetail $true
if (-not $unauthorized.error -or $unauthorized.userCount -ne 0) { throw 'Full details accepted the wrong user.' }
$multiple = Get-CleanupSummaryAllUsers -CategoryIds 'developerCaches' -TargetUsers "$fixtureUser,other-user" -FullDetail $true
if (-not $multiple.error -or $multiple.userCount -ne 0) { throw 'Full details accepted multiple users.' }
$script:isAdmin = $false
$denied = Get-CleanupSummaryAllUsers -CategoryIds 'developerCaches' -TargetUsers $fixtureUser -FullDetail $true
if (-not $denied.error -or $denied.userCount -ne 0) { throw 'A non-admin caller accessed another user.' }
$oldApp = $env:APPDATA; $oldLocal = $env:LOCALAPPDATA; $oldProfile = $env:USERPROFILE
try {
    $script:fixtureSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $env:APPDATA = "$fixtureProfile\AppData\Roaming"; $env:LOCALAPPDATA = "$fixtureProfile\AppData\Local"; $env:USERPROFILE = $fixtureProfile
    $own = Get-CleanupSummaryAllUsers -CategoryIds 'developerCaches' -TargetUsers $fixtureUser -FullDetail $true
    if ($own.error -or @($own.users)[0].categories[0].count -ne 501) { throw 'A non-admin caller could not access their own detail.' }
} finally { $env:APPDATA = $oldApp; $env:LOCALAPPDATA = $oldLocal; $env:USERPROFILE = $oldProfile }
Write-Output 'Passed: selected-account developer-cache preview and full-detail routing.'
