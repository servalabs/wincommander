$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# The task receives an isolated fake filesystem: no real browser is scanned or removed.
$oldEnv = @{}
foreach ($key in @('USERPROFILE','LOCALAPPDATA','APPDATA','ProgramFiles','ProgramFiles(x86)')) {
    $oldEnv[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
}
try {
    $env:USERPROFILE = 'C:\Fixture\User'
    $env:LOCALAPPDATA = 'C:\Fixture\User\AppData\Local'
    $env:APPDATA = 'C:\Fixture\User\AppData\Roaming'
    $env:ProgramFiles = 'C:\Fixture\Program Files'
    ${env:ProgramFiles(x86)} = 'C:\Fixture\Program Files (x86)'
    $script:files = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $script:dirs = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    function Add-FakePath([string]$Path, [bool]$Directory) {
        if ($Directory) { [void]$script:dirs.Add($Path) } else { [void]$script:files.Add($Path) }
        $parent = [IO.Path]::GetDirectoryName($Path)
        while ($parent) { [void]$script:dirs.Add($parent); $parent = [IO.Path]::GetDirectoryName($parent) }
    }
    $chrome = 'C:\Fixture\User\AppData\Local\Google\Chrome\User Data\Default'
    $firefox = 'C:\Fixture\User\AppData\Roaming\Mozilla\Firefox'
    Add-FakePath 'C:\Fixture\Program Files\Google\Chrome\Application\chrome.exe' $false
    Add-FakePath 'C:\Fixture\Program Files\Mozilla Firefox\firefox.exe' $false
    Add-FakePath "$chrome\History" $false
    $chromeProfile = 'C:\Fixture\User\AppData\Local\Google\Chrome\User Data\Profile 1'
    Add-FakePath "$chromeProfile\Cookies" $false
    Add-FakePath "$chrome\Network" $true
    Add-FakePath "$firefox\profiles.ini" $false
    Add-FakePath "$firefox\Profiles\test\places.sqlite" $false
    Add-FakePath 'C:\Fixture\User\AppData\Local\BraveSoftware\Brave-Browser\User Data\Default\History' $false
    function Test-Path { param([string]$LiteralPath, [string]$PathType) if ($PathType -eq 'Leaf') { return $script:files.Contains($LiteralPath) }; if ($PathType -eq 'Container') { return $script:dirs.Contains($LiteralPath) }; return $script:files.Contains($LiteralPath) -or $script:dirs.Contains($LiteralPath) }
    function Get-Item { param([string]$LiteralPath, [switch]$Force, $ErrorAction) if (-not (Test-Path -LiteralPath $LiteralPath)) { throw 'missing fake path' }; [pscustomobject]@{ Attributes=[IO.FileAttributes]::Normal; PSIsContainer=$script:dirs.Contains($LiteralPath) } }
    function Get-Content { param([string]$LiteralPath, $ErrorAction) @('[Profile0]','IsRelative=1','Path=Profiles/test') }
    function Get-AutoEraseDirectories { param([string]$Path) @($script:dirs | Where-Object { [IO.Path]::GetDirectoryName($_) -eq $Path } | ForEach-Object { [pscustomobject]@{ Name=[IO.Path]::GetFileName($_); FullName=$_ } }) }
    $script:erased = [Collections.Generic.List[string]]::new()
    function Erase-OneFile($Path) { $script:erased.Add($Path); [void]$script:files.Remove($Path); $script:AutoEraseRemoved++ }
    function Erase-Dir($Path) { $script:erased.Add($Path); [void]$script:dirs.Remove($Path); $script:AutoEraseRemoved++ }
    $script:AutoEraseRemoved = 0; $script:AutoEraseFailed = 0; $script:AutoEraseCoverageIncomplete = $false
    Invoke-Expression $script:AutoEraseScripts['browserFootprints']
    if (-not $script:AutoEraseCoverageIncomplete) { throw 'Uninspected browsers and custom paths must be reported as incomplete coverage.' }
    if ($script:AutoEraseFailed -or $script:erased.Count -ne 4 -or -not $script:erased.Contains("$chrome\History") -or -not $script:erased.Contains("$chrome\Network") -or -not $script:erased.Contains("$chromeProfile\Cookies") -or -not $script:erased.Contains("$firefox\Profiles\test\places.sqlite")) { throw 'Installed browser profile data was not correctly scoped and removed.' }
    if ($script:erased | Where-Object { $_ -like '*Brave*' }) { throw 'Uninstalled browser data was unexpectedly targeted.' }

    Add-FakePath "$chrome\History" $false
    function Erase-OneFile($Path) { $script:erased.Add($Path) }
    $script:AutoEraseFailed = 0
    Invoke-Expression $script:AutoEraseScripts['browserFootprints']
    if ($script:AutoEraseFailed -lt 1) { throw 'An artifact remaining after cleanup was reported as successful.' }
    [void]$script:files.Remove("$chrome\History")

    # An external custom profile may be legitimate, but an untrusted profiles.ini
    # must never direct a recurring wipe outside the selected user's home.
    function Get-Content { param([string]$LiteralPath, $ErrorAction) @('[Profile0]','IsRelative=0','Path=C:\OtherUser\profile') }
    Add-FakePath 'C:\OtherUser\profile' $true
    $script:AutoEraseFailed = 0
    Invoke-Expression $script:AutoEraseScripts['browserFootprints']
    if ($script:AutoEraseFailed -lt 1) { throw 'External profile path was not reported as unsupported.' }
    if ($script:erased | Where-Object { $_ -like 'C:\OtherUser\*' }) { throw 'An external profile was targeted.' }

    $catalog = Get-AutoEraseSupportedCategories
    if ('browserFootprints' -notin $catalog.schedulableCategories -or 'browserFootprints' -in $catalog.categories) { throw 'Browser must be individual-only.' }
    if (-not (Set-AutoEraseSchedule -CategoryId 'browserFootprints' -IntervalMinutes 60).error) { throw 'Browser consent was not enforced.' }
    if (-not (Set-AutoEraseSchedule -CategoryId 'browserFootprints' -IntervalMinutes 60 -AllowDefaultExcluded -ManagedByAutoSet).error) { throw 'Bulk scheduling accepted protected browser data.' }
    Write-Output 'Passed: mocked installed-browser discovery, scoped profiles, readback and manual-only policy.'
} finally {
    foreach ($key in $oldEnv.Keys) { [Environment]::SetEnvironmentVariable($key, $oldEnv[$key], 'Process') }
}
