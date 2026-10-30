$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')

# Real child processes emit only synthetic text and never access the SSH agent.
if ((Invoke-WCSshProcess -Executable $env:ComSpec -Arguments '/c "echo synthetic 1>&2 & exit /b 0"') -ne 0) { throw 'Successful stderr output must not cause failure.' }
if ((Invoke-WCSshProcess -Executable $env:ComSpec -Arguments '/c "echo synthetic 1>&2 & exit /b 2"') -ne 2) { throw 'A child failure must retain its exit code.' }
$timedOut = $false
try { $null = Invoke-WCSshProcess -Executable (Join-Path $PSHOME 'powershell.exe') -Arguments '-NoProfile -Command "Start-Sleep -Seconds 5"' -TimeoutMilliseconds 100 } catch { $timedOut = $_.Exception.Message -eq 'SSH agent tool timed out.' }
if (-not $timedOut) { throw 'Timed-out SSH tools must fail without exposing their output.' }
if (-not $script:ScheduledSshFunctions.Contains('OpenSSH\ssh-add.exe') -or $script:ScheduledSshFunctions -match 'Get-Command') { throw 'SSH tool selection must be rooted in the Windows OpenSSH directory.' }
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('wincommander-ssh-schedule-test-' + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $fixtureRoot -ErrorAction Stop
$savedStorage = ${function:Initialize-AutoEraseStorage}
function Initialize-AutoEraseStorage { return @{ scripts = $fixtureRoot } }
try {
    $null = ConvertTo-AutoEraseTaskArgument -CategoryId 'sshState' -IntervalMinutes 60 -Script $script:AutoEraseScripts['sshState'] -ExecutionScope 'fixture-user' -FirstDueUtc ([DateTime]::UtcNow.ToString('o')) -AllowDefaultExcluded $true
    $generatedItems = @(Get-ChildItem -LiteralPath $fixtureRoot -Filter 'sshState.fixture-user.g*.ps1' -File)
    if ($generatedItems.Count -ne 1) { throw 'Exactly one generated SSH task must exist.' }
    $generated = $generatedItems[0].FullName
    $source = [IO.File]::ReadAllText($generated)
    if (-not $source.Contains('function Invoke-WCSshAdd') -or -not $source.Contains('function Invoke-WCSshProcess')) { throw 'Generated SSH tasks must embed their helpers.' }
    $tokens = $null; $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile($generated, [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw 'Generated SSH task must parse.' }
} finally {
    ${function:Initialize-AutoEraseStorage} = $savedStorage
    $absolute = [IO.Path]::GetFullPath($fixtureRoot); $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $absolute.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($absolute) -notlike 'wincommander-ssh-schedule-test-*') { throw 'Unexpected fixture path.' }
    Remove-Item -LiteralPath $absolute -Recurse -Force -ErrorAction Stop
}

# Mock both targets; never inspect the real SSH agent or known_hosts.
$savedUserProfile = $env:USERPROFILE
try {
    $env:USERPROFILE = 'C:\Fixture\User'
    $script:agentDenied = $false
    $script:toolAvailable = $true
    $script:knownHostsRemain = $false
    $script:erased = $false
    function Invoke-WCSshAdd { param([string]$Option) if (-not $script:toolAvailable) { throw 'Unavailable' }; if ($script:agentDenied) { return 2 }; if ($Option -eq '-D') { return 0 }; return 1 }
    function Erase-OneFile { param([string]$Path) $script:erased = $true; return $true }
    function Test-Path { param([string]$LiteralPath, [string]$PathType, $ErrorAction) return $script:knownHostsRemain }

    $payload = $script:AutoEraseScripts['sshState']

    $script:AutoEraseFailed = 0
    Invoke-Expression $payload
    if ($script:AutoEraseFailed -ne 0 -or -not $script:erased) { throw 'Complete SSH cleanup was not recorded.' }

    $script:AutoEraseFailed = 0; $script:agentDenied = $true; $script:erased = $false
    Invoke-Expression $payload
    if ($script:AutoEraseFailed -eq 0 -or -not $script:erased) { throw 'Agent failure must still clear known_hosts and report incomplete.' }

    $script:AutoEraseFailed = 0; $script:agentDenied = $false; $script:toolAvailable = $false; $script:erased = $false
    Invoke-Expression $payload
    if ($script:AutoEraseFailed -eq 0 -or -not $script:erased) { throw 'Missing Windows OpenSSH must fail without searching an alternate executable.' }

    $script:AutoEraseFailed = 0; $script:toolAvailable = $true; $script:knownHostsRemain = $true
    Invoke-Expression $payload
    if ($script:AutoEraseFailed -eq 0) { throw 'A remaining known_hosts file must report incomplete.' }
    Write-Output 'Passed: independent SSH cleanup targets and readback fixtures.'
} finally { $env:USERPROFILE = $savedUserProfile }
