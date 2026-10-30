$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
foreach ($name in @('Get-CleanupChromiumProfiles', 'Get-BrowserFootprints', 'Clear-BrowserFootprints')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name" }
    . ([scriptblock]::Create($definition.Extent.Text))
}
$script:root = 'C:\FakeUser\Chrome\User Data'
$script:paths = @("$script:root\Default\History", "$script:root\Profile 1\Cookies")
function Get-InstalledBrowsers { @(@{ Name='Chrome'; Engine='Chromium'; UserDataDir=$script:root }) }
function Test-Path { param($Path) return ($Path -eq $script:root -or $Path -in $script:paths) }
function Get-ChildItem {
    param($LiteralPath, [switch]$Directory, [switch]$Force, $ErrorAction)
    if ($LiteralPath -eq $script:root) { return @([pscustomobject]@{ Name='Default'; FullName="$script:root\Default" }, [pscustomobject]@{ Name='Profile 1'; FullName="$script:root\Profile 1" }) }
    throw 'Unexpected directory access'
}
function Get-Item { param($Path, $LiteralPath, [switch]$Force, $ErrorAction) [pscustomobject]@{ PSIsContainer=$false; Length=1024; Attributes=[IO.FileAttributes]::Normal } }
function Assert-CleanupCategoriesAllowed { param($Categories, [switch]$AllowDefaultExcluded) if (-not $AllowDefaultExcluded) { throw 'Missing consent' } }
$script:removed = @()
function Remove-ItemSecure { param($Path) $script:removed += $Path }
$scan = Get-BrowserFootprints
if ($scan.error -or $scan.totalBrowsers -ne 1 -or @($scan.browsers[0].artifacts).Count -ne 2) { throw 'Browser scan omitted a numbered profile.' }
if (@($scan.browsers[0].artifacts | Where-Object { $_.profilePath -eq "$script:root\Profile 1" -and $_.name -eq 'Cookies' }).Count -ne 1) { throw 'Browser scan lost artifact profile identity.' }
$clear = Clear-BrowserFootprints -Browser 'Chrome' -ProfilePath $script:root -AllowDefaultExcluded $true
if (-not $clear.success -or $script:removed.Count -ne 2 -or @($script:paths | Where-Object { $_ -notin $script:removed }).Count) { throw 'Manual clear did not match the scanned profiles.' }
Write-Output 'Passed: synthetic browser scans and individual clear cover the same numbered profiles.'
