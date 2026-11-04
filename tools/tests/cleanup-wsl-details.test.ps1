$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-WSLDataInfo' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
$helper = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Test-CleanupWslUnavailable' }, $true)
. ([scriptblock]::Create($helper.Extent.Text))

# Synthetic provider inventory; do not read or mutate real WSL data.
$script:missingDisks = $false
function Test-Path { param($Path, $PathType, $ErrorAction) if ($script:missingDisks -and $PathType -eq 'Leaf') { return $false }; return $true }
$script:denyPackages = $false
$script:noRecords = $false
function Get-ChildItem {
    param($Path, [switch]$Directory, [switch]$Force, $ErrorAction)
    if ($script:denyPackages -and $Path -like '*Packages') { throw 'Synthetic access denied' }
    if ($script:noRecords) { return }
    if ($Path -like '*Packages') { [pscustomobject]@{ Name='StoreFixture'; FullName='C:\Fixture\StoreFixture' } }
    else { [pscustomobject]@{ PSPath='fixtureRegistry'; PSChildName='fixtureId' } }
}
function Get-ItemProperty { param($Path, $ErrorAction) [pscustomobject]@{ DistributionName='ImportedFixture'; BasePath='C:\Fixture\Imported' } }
$script:denyRegisteredDisk = $false
function Get-Item {
    param($LiteralPath, [switch]$Force, $ErrorAction)
    if ($script:denyRegisteredDisk -and $LiteralPath -like '*Imported*') { throw 'Synthetic disk access denied' }
    [pscustomobject]@{ FullName=$LiteralPath; Length=1024; LastWriteTime=[datetime]'2026-01-02' }
}
$script:wslPresent = $false
$script:wslUnknown = $false
$script:wslMode = 'failure'
$script:verboseCalls = 0
function Get-CleanupWslExecutable { if ($script:wslPresent) { return @{ available=$true; verifiedAbsent=$false; path='Invoke-TestWsl' } }; return @{ available=$false; verifiedAbsent=(-not $script:wslUnknown) } }
function Invoke-TestWsl {
    if ($args -contains '-v') { $script:verboseCalls++ }
    if ($script:wslMode -eq 'quietEmpty' -and $args -contains '-q') { $global:LASTEXITCODE = 0; return }
    if ($script:wslMode -eq 'localizedVerbose' -and $args -contains '-v') { $global:LASTEXITCODE = 0; 'NOM  ETAT  VERSION'; 'erreur locale'; return }
    if ($script:wslMode -eq 'englishVerbose' -and $args -contains '-v') { $global:LASTEXITCODE = 0; '  NAME      STATE    VERSION'; '* Ubuntu    Stopped  2'; return }
    $global:LASTEXITCODE = 1; 'Synthetic Windows failure output'
}
$result = Get-WSLDataInfo
if ($result.error -or $result.total -ne 2) { throw 'WSL synthetic disks were not returned.' }
if (-not $result.partial -or $result.message -notmatch 'WSL command is unavailable') { throw 'A missing WSL command must identify why coverage is partial.' }
if (@($result.files | Where-Object { $_.path -eq 'C:\Fixture\StoreFixture\LocalState\ext4.vhdx' }).Count -ne 1 -or @($result.files | Where-Object { $_.path -eq 'C:\Fixture\Imported\ext4.vhdx' }).Count -ne 1) { throw 'WSL disk paths were lost.' }
$script:denyPackages = $true
$partial = Get-WSLDataInfo
if (-not $partial.partial -or $partial.status -ne 'partial' -or $partial.total -ne 1 -or @($partial.files | Where-Object { $_.path -eq 'C:\Fixture\Imported\ext4.vhdx' }).Count -ne 1) { throw 'WSL access failure must keep available rows and report incomplete.' }
$script:denyPackages = $false
$script:wslPresent = $true
$failedList = Get-WSLDataInfo
if (-not $failedList.partial -or $failedList.total -ne 2 -or $failedList.message -notmatch 'could not list WSL distributions') { throw 'A failed WSL listing must not invent a distro from its output and must explain partial coverage.' }
$script:missingDisks = $true
$registrationOnly = Get-WSLDataInfo
if (-not $registrationOnly.partial -or $registrationOnly.total -ne 1 -or $registrationOnly.files[0].name -ne 'Registered WSL distro: ImportedFixture') { throw 'An observed WSL registration must remain visible when its virtual disk is absent and the WSL query fails.' }
$script:missingDisks = $false
$script:denyRegisteredDisk = $true
$inaccessibleDisk = Get-WSLDataInfo
if (-not $inaccessibleDisk.partial -or @($inaccessibleDisk.files | Where-Object { $_.name -eq 'Registered WSL distro: ImportedFixture' }).Count -ne 1) { throw 'A verified WSL registration must remain visible if its disk metadata is inaccessible.' }
$script:denyRegisteredDisk = $false
$script:noRecords = $true
$script:featureState = 2
$script:hasWslRuntime = $false
$script:hasWslPackage = $false
function Get-CimInstance {
    [CmdletBinding()] param($ClassName, $Filter)
    if ($ClassName -eq 'Win32_OptionalFeature') { [pscustomobject]@{ InstallState = $script:featureState } }
    elseif ($script:hasWslRuntime) { [pscustomobject]@{ Name = 'WslService' } }
}
function Get-AppxPackage { [CmdletBinding()] param($Name) if ($script:hasWslPackage) { [pscustomobject]@{ Name = $Name } } }
$absent = Get-WSLDataInfo
if ($absent.partial -or $absent.status -ne 'complete' -or $absent.total -ne 0 -or $absent.coverage -ne 'limited') { throw 'Verified disabled WSL with no runtime, registration or selected disks should report a scoped empty inventory.' }
$script:wslPresent = $false; $script:wslUnknown = $true
$unknownExecutable = Get-WSLDataInfo
if (-not $unknownExecutable.partial -or $unknownExecutable.message -notmatch 'could not be verified') { throw 'An unverified system executable must not be treated as absent.' }
$script:wslPresent = $true; $script:wslUnknown = $false
$script:featureState = 1
if (-not (Get-WSLDataInfo).partial) { throw 'An enabled WSL feature with a failed query must stay partial.' }
$script:wslMode = 'quietEmpty'; $script:verboseCalls = 0
$quietEmpty = Get-WSLDataInfo
if ($quietEmpty.error -or $quietEmpty.partial -or $quietEmpty.total -ne 0 -or $script:verboseCalls -ne 0) { throw 'A successful empty quiet listing must not parse verbose output as a distribution.' }
$script:wslMode = 'localizedVerbose'
$localized = Get-WSLDataInfo
if (-not $localized.partial -or $localized.total -ne 0) { throw 'An unrecognized verbose header must not invent a distribution.' }
$script:wslMode = 'englishVerbose'
$fallback = Get-WSLDataInfo
if (-not $fallback.partial -or $fallback.total -ne 1 -or $fallback.files[0].name -ne 'Registered WSL distro: Ubuntu') { throw 'A recognized verbose listing must retain its observed distro while reporting the failed quiet query.' }
$script:wslMode = 'failure'
$script:featureState = 2; $script:hasWslRuntime = $true
if (-not (Get-WSLDataInfo).partial) { throw 'A present WSL runtime with a failed query must stay partial.' }
$script:hasWslRuntime = $false; $script:hasWslPackage = $true
if (-not (Get-WSLDataInfo).partial) { throw 'A present Store WSL package with a failed query must stay partial.' }
$script:hasWslPackage = $false; $script:denyPackages = $true
if (-not (Get-WSLDataInfo).partial) { throw 'An unreadable WSL storage location must not be treated as a verified empty inventory.' }
Write-Output 'Passed: WSL Store and imported disk paths are retained.'
