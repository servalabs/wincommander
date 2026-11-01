$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupChromiumProfiles', 'Get-CleanupSummaryBrowserInventory', 'Get-CleanupBrowserFootprints', 'Get-BrowserFootprints', 'Clear-BrowserFootprints')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:root = 'C:\FakeUser\Chrome\User Data'
$script:paths = @("$script:root\Default\History", "$script:root\Profile 1\Cookies")
$script:includeDeniedBrowser = $false
function Get-InstalledBrowsers { @{ Name='Chrome'; Engine='Chromium'; UserDataDir=$script:root }; if ($script:includeDeniedBrowser) { @{ Name='Denied'; Engine='Chromium'; UserDataDir='C:\Denied' } } }
function Test-Path { param($Path, $LiteralPath, $PathType, $ErrorAction) $p = if ($LiteralPath) { $LiteralPath } else { $Path }; return ($p -eq $script:root -or $p -eq 'C:\Denied' -or $p -in $script:paths) }
function Get-ChildItem {
    param($LiteralPath, [switch]$Directory, [switch]$Force, $ErrorAction)
    if ($LiteralPath -eq $script:root) { return @([pscustomobject]@{ Name='Default'; FullName="$script:root\Default" }, [pscustomobject]@{ Name='Profile 1'; FullName="$script:root\Profile 1" }) }
    throw 'Unexpected directory access'
}
$script:denyBrowserMeasure = $false
function Get-Item { param($Path, $LiteralPath, [switch]$Force, $ErrorAction) if ($script:denyBrowserMeasure -and $Path -like '*Cookies') { throw 'Synthetic access denied' }; [pscustomobject]@{ PSIsContainer=$false; Length=1024; Attributes=[IO.FileAttributes]::Normal } }
function Assert-CleanupCategoriesAllowed { param($Categories, [switch]$AllowDefaultExcluded) if (-not $AllowDefaultExcluded) { throw 'Missing consent' } }
$script:removed = @()
function Remove-ItemSecure { param($Path) $script:removed += $Path }
$scan = Get-BrowserFootprints
if ($scan.error -or $scan.totalBrowsers -ne 1 -or @($scan.browsers[0].artifacts).Count -ne 2) { throw 'Browser scan omitted a numbered profile.' }
if (@($scan.browsers[0].artifacts | Where-Object { $_.profilePath -eq "$script:root\Profile 1" -and $_.name -eq 'Cookies' }).Count -ne 1) { throw 'Browser scan lost artifact profile identity.' }
$script:denyBrowserMeasure = $true
$partial = Get-BrowserFootprints
$denied = @($partial.browsers[0].artifacts | Where-Object { $_.name -eq 'Cookies' })
if (-not $partial.partial -or $partial.status -ne 'partial' -or $denied.Count -ne 1 -or $null -ne $denied[0].sizeKB -or @($partial.browsers[0].artifacts).Count -ne 2) { throw 'An inaccessible browser artifact must remain visible with an unknown size and incomplete status.' }
$script:denyBrowserMeasure = $false
$script:includeDeniedBrowser = $true
$partialBrowser = Get-BrowserFootprints
if ($partialBrowser.error -or -not $partialBrowser.partial -or $partialBrowser.totalBrowsers -ne 1 -or @($partialBrowser.browsers[0].artifacts).Count -ne 2) { throw 'An inaccessible later browser must preserve the earlier browser records and mark the scan incomplete.' }
$script:includeDeniedBrowser = $false
$clear = Clear-BrowserFootprints -Browser 'Chrome' -ProfilePath $script:root -AllowDefaultExcluded $true
if (-not $clear.success -or $script:removed.Count -ne 2 -or @($script:paths | Where-Object { $_ -notin $script:removed }).Count) { throw 'Manual clear did not match the scanned profiles.' }
Write-Output 'Passed: synthetic browser scans and individual clear cover the same numbered profiles.'

$otherProfile = 'C:\FakeUser\Other'
$script:root = Join-Path $otherProfile 'AppData\Local\Google\Chrome\User Data'
$otherExe = Join-Path $otherProfile 'AppData\Local\Google\Chrome\Application\chrome.exe'
$script:paths = @($otherExe, "$script:root\Default\History", "$script:root\Profile 1\Cookies")
$script:BrowserMap = @(@{
    Name='Chrome'; Engine='Chromium'; UserDataDir=(Join-Path $env:LOCALAPPDATA 'Google\Chrome\User Data')
    ExePaths=@((Join-Path $env:LOCALAPPDATA 'Google\Chrome\Application\chrome.exe'))
})
$otherBrowsers = @(Get-CleanupSummaryBrowserInventory -ProfilePath $otherProfile)
if ($otherBrowsers.Count -ne 1 -or $otherBrowsers[0].UserDataDir -ne $script:root) { throw 'Browser inventory did not use the selected account.' }
$otherScan = Get-CleanupBrowserFootprints -Browsers $otherBrowsers
if ($otherScan.error -or $otherScan.totalBrowsers -ne 1 -or @($otherScan.browsers[0].artifacts).Count -ne 2) { throw 'Selected-account browser artifacts were omitted.' }
if (@($otherScan.browsers[0].artifacts | Where-Object { $_.profilePath -eq "$script:root\Profile 1" -and $_.name -eq 'Cookies' }).Count -ne 1) { throw 'Selected-account browser profile identity was lost.' }
Write-Output 'Passed: another account uses its own browser inventory and full profile details.'
