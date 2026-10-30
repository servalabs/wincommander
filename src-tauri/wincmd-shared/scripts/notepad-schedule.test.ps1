$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# All paths and provider functions below are fixtures; no user files are read.
$originalLocalAppData = $env:LOCALAPPDATA
try {
    $env:LOCALAPPDATA = 'C:\Fixture\Local'
    $script:fakeFiles = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [void]$script:fakeFiles.Add('C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_fixture\LocalState\TabState\top.bin')
    [void]$script:fakeFiles.Add('C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_fixture\LocalState\TabState\nested\tab.bin')
    $script:removedFiles = [Collections.Generic.List[string]]::new()
    function Get-AutoEraseDirectories {
        param([string]$Path, [string]$Filter)
        if ($Path -ne 'C:\Fixture\Local\Packages' -or $Filter -ne 'Microsoft.WindowsNotepad_*') { throw 'Unexpected package query.' }
        [pscustomobject]@{ FullName = 'C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_fixture' }
    }
    function Get-AutoEraseFiles {
        param([string]$Path, [switch]$Recurse)
        if (-not $Recurse -or $Path -ne 'C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_fixture\LocalState\TabState') { throw 'Expected recursive scan of the selected tab-state directory.' }
        @($script:fakeFiles | ForEach-Object { [pscustomobject]@{ FullName = $_ } })
    }
    function Erase-FileSet {
        param([object[]]$Files)
        foreach ($file in $Files) { $script:removedFiles.Add($file.FullName); [void]$script:fakeFiles.Remove($file.FullName) }
    }
    Invoke-Expression $script:AutoEraseScripts['notepadState']
    if ($script:removedFiles.Count -ne 2 -or $script:fakeFiles.Count -ne 0) { throw 'Nested Notepad records were not removed.' }

    [void]$script:fakeFiles.Add('C:\Fixture\Local\Packages\Microsoft.WindowsNotepad_fixture\LocalState\TabState\nested\locked.bin')
    function Erase-FileSet { param([object[]]$Files) }
    $failed = $false
    try { Invoke-Expression $script:AutoEraseScripts['notepadState'] } catch { $failed = $true }
    if (-not $failed) { throw 'Remaining tab-state records were reported as cleared.' }

    function Get-AutoEraseFiles { param([string]$Path, [switch]$Recurse) throw 'Synthetic access denied.' }
    $failed = $false
    try { Invoke-Expression $script:AutoEraseScripts['notepadState'] } catch { $failed = $true }
    if (-not $failed) { throw 'An inaccessible tab-state directory was reported as cleared.' }
    Write-Output 'Passed: nested Notepad tab-state cleanup, readback and access failure fixtures.'
} finally {
    $env:LOCALAPPDATA = $originalLocalAppData
}
