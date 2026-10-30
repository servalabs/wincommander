$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. ([scriptblock]::Create($script:EraseFunctions))

# The real payload and Erase-Dir execute against a synthetic in-memory tree.
# Every filesystem operation they can invoke is replaced by a rejecting mock.
$saved = @{ APPDATA=$env:APPDATA; LOCALAPPDATA=$env:LOCALAPPDATA; USERPROFILE=$env:USERPROFILE }
try {
    $env:APPDATA='C:\Fixture\Roaming'; $env:LOCALAPPDATA='C:\Fixture\Local'; $env:USERPROFILE='C:\Fixture\User'
    $script:tree = @{}; $script:rejectRoot = ''
    function Get-Item {
        [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
        if (-not $script:tree.ContainsKey($LiteralPath)) { throw [Management.Automation.ItemNotFoundException]::new('Synthetic absent item') }
        return [pscustomobject]@{ FullName=$LiteralPath; PSIsContainer=$script:tree[$LiteralPath].directory; Attributes=0 }
    }
    function Assert-AutoEraseAncestors { param($Path) if (-not $Path.StartsWith('C:\Fixture\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected fixture path.' } }
    function Get-AutoEraseFiles {
        [CmdletBinding()] param([string]$Path, [switch]$Recurse)
        if ($Path -eq $script:rejectRoot) { throw 'Synthetic rejected tree.' }
        foreach ($key in @($script:tree.Keys)) { if (-not $script:tree[$key].directory -and $key.StartsWith($Path + '\', [StringComparison]::OrdinalIgnoreCase)) { [pscustomobject]@{ FullName=$key } } }
    }
    function Get-AutoEraseDirectories {
        [CmdletBinding()] param([string]$Path, [switch]$Recurse)
        foreach ($key in @($script:tree.Keys)) { if ($script:tree[$key].directory -and $key.StartsWith($Path + '\', [StringComparison]::OrdinalIgnoreCase)) { [pscustomobject]@{ FullName=$key; Attributes=0 } } }
    }
    function Get-ChildItem {
        [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
        foreach ($key in @($script:tree.Keys)) { if ([IO.Path]::GetDirectoryName($key) -eq $LiteralPath) { [pscustomobject]@{ FullName=$key } } }
    }
    function Erase-OneFile {
        param([string]$Path)
        if (-not $script:tree.ContainsKey($Path) -or $script:tree[$Path].directory) { return $false }
        $script:tree.Remove($Path); $script:AutoEraseRemoved++; return $true
    }
    function Remove-Item {
        [CmdletBinding()] param([string]$LiteralPath, [switch]$Force)
        if (-not $script:tree.ContainsKey($LiteralPath) -or -not $script:tree[$LiteralPath].directory -or @(Get-ChildItem -LiteralPath $LiteralPath).Count) { throw 'Only an empty synthetic directory can be removed.' }
        $script:tree.Remove($LiteralPath)
    }
    function Test-Path { [CmdletBinding()] param([string]$LiteralPath) return $script:tree.ContainsKey($LiteralPath) }
    foreach ($case in @(
        @{ id='developerCaches'; root=(Join-Path $env:LOCALAPPDATA 'pip\Cache') },
        @{ id='passwordManagerCaches'; root=(Join-Path $env:APPDATA 'Bitwarden\Cache') }
    )) {
        $root = $case.root; $child = Join-Path $root 'child'; $file = Join-Path $child 'synthetic.txt'
        $script:tree = @{}; $script:tree[$root]=@{directory=$true}; $script:tree[$child]=@{directory=$true}; $script:tree[$file]=@{directory=$false}
        $script:AutoEraseFailed=0; $script:AutoEraseRemoved=0; $script:AutoEraseCoverageIncomplete=$false
        & ([scriptblock]::Create($script:AutoEraseScripts[$case.id]))
        if ($script:AutoEraseFailed -ne 0 -or $script:tree.Count -ne 0 -or $script:AutoEraseRemoved -ne 1 -or -not $script:AutoEraseCoverageIncomplete) { throw "Synthetic $($case.id) cleanup did not remove only its in-memory tree." }
    }
    $root = Join-Path $env:LOCALAPPDATA 'pip\Cache'; $file = Join-Path $root 'must-remain.txt'
    $script:tree=@{}; $script:tree[$root]=@{directory=$true}; $script:tree[$file]=@{directory=$false}; $script:rejectRoot=$root; $script:AutoEraseFailed=0
    & ([scriptblock]::Create($script:AutoEraseScripts['developerCaches']))
    if ($script:AutoEraseFailed -eq 0 -or -not $script:tree.ContainsKey($file)) { throw 'Rejected synthetic tree was not retained with a failure.' }
    Write-Output 'Passed: protected cache payloads remove only synthetic entries and preserve rejected trees.'
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
