$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
. ([scriptblock]::Create($script:ScheduledDockerFunctions))
function Assert-WCDockerTest($Condition, $Message) { if (-not $Condition) { throw $Message } }

$originalHost = $env:DOCKER_HOST; $originalContext = $env:DOCKER_CONTEXT; $originalProxy = $env:HTTPS_PROXY
try {
    $env:DOCKER_HOST = 'tcp://fixture-remote:2376'; $env:DOCKER_CONTEXT = 'fixture-remote'; $env:HTTPS_PROXY = 'http://fixture-proxy'
    $start = New-WCDockerStartInfo -Executable 'C:\fixture\docker.exe' -ConfigPath 'C:\fixture\isolated' -Arguments @('info') -Mutation $false
    Assert-WCDockerTest ($start.Arguments.Contains('npipe:////./pipe/dockerDesktopLinuxEngine') -and $start.Arguments.Contains('"--config"')) 'Only the local Docker Desktop named pipe and isolated config are allowed.'
    Assert-WCDockerTest (-not $start.EnvironmentVariables.ContainsKey('DOCKER_HOST') -and -not $start.EnvironmentVariables.ContainsKey('DOCKER_CONTEXT') -and -not $start.EnvironmentVariables.ContainsKey('HTTPS_PROXY')) 'Remote context, host and proxy environment must not reach the provider.'
} finally { $env:DOCKER_HOST = $originalHost; $env:DOCKER_CONTEXT = $originalContext; $env:HTTPS_PROXY = $originalProxy }

$script:scenario = 'valid'
$script:identityReads = 0
$script:mutations = [Collections.Generic.List[string]]::new()
$script:containers = @('a' * 64)
$script:images = @('sha256:' + ('b' * 64))
$script:volumes = @('fixture-volume')
$script:volumeKinds = @{}
$script:remainders = 1
function Invoke-WCDocker {
    param([string]$ConfigPath, [string[]]$Arguments, [bool]$Mutation)
    if ($Arguments[0] -eq 'info') {
        $script:identityReads++
        if ($script:scenario -eq 'remote') { return '{"ID":"fixture","OSType":"linux","OperatingSystem":"Remote Linux"}' }
        if ($script:scenario -eq 'changed' -and $script:identityReads -gt 1) { return '{"ID":"other-engine","OSType":"linux","OperatingSystem":"Docker Desktop"}' }
        return '{"ID":"engine-one","OSType":"linux","OperatingSystem":"Docker Desktop"}'
    }
    if ($Mutation) {
        $script:mutations.Add(($Arguments -join ' '))
        if ($script:scenario -eq 'denied') { throw 'Synthetic permission denial' }
        if ($Arguments[0] -eq 'container') { $script:containers = @() }
        elseif ($Arguments[0] -eq 'system') { $script:images = @() }
        elseif ($Arguments[0] -eq 'volume' -and $script:scenario -ne 'retained') { $script:volumes = @($script:volumes | Where-Object { $_ -cne $Arguments[2] }) }
        return @()
    }
    if ($Arguments[0] -eq 'container') { return $script:containers }
    if ($Arguments[0] -eq 'image') { return $script:images }
    if ($Arguments[0] -eq 'volume' -and $Arguments[1] -eq 'inspect') {
        $name = $Arguments[4]
        $kind = $script:volumeKinds[$name]
        if ($kind -eq 'plugin') { return (@{ Name=$name; Driver='remote-plugin'; Scope='global'; Options=$null; Mountpoint='/remote/data' } | ConvertTo-Json -Compress) }
        if ($kind -eq 'remote-mount') { return (@{ Name=$name; Driver='local'; Scope='local'; Options=@{ type='nfs'; device='remote:/export' }; Mountpoint="/var/lib/docker/volumes/$name/_data" } | ConvertTo-Json -Compress) }
        if ($kind -eq 'external-path') { return (@{ Name=$name; Driver='local'; Scope='local'; Options=$null; Mountpoint='/other/data' } | ConvertTo-Json -Compress) }
        return (@{ Name=$name; Driver='local'; Scope='local'; Options=$null; Mountpoint="/var/lib/docker/volumes/$name/_data" } | ConvertTo-Json -Compress)
    }
    if ($Arguments[0] -eq 'volume') { return $script:volumes }
    if ($Arguments[0] -eq 'system' -and $Arguments[1] -eq 'df') {
        if ($script:scenario -eq 'cache') { return '{"Type":"Build Cache","TotalCount":"2"}' }
        return '{"Type":"Build Cache","TotalCount":"0"}'
    }
    throw 'Unexpected Docker provider command.'
}
function Get-WCDockerArtifactRemainders { return $script:remainders }

$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.removed -eq 3 -and $result.failed -gt 0) 'Docker resources may be removed, but a retained VHDX or log must make the result incomplete.'
Assert-WCDockerTest ($script:mutations.Count -eq 3 -and $script:mutations[0] -eq ('container rm --force ' + ('a' * 64)) -and $script:mutations[2] -eq 'volume rm fixture-volume') 'Only verified container and local volume identities may be removed.'
Assert-WCDockerTest (@($script:mutations | Where-Object { $_ -match '--volumes|volume prune' }).Count -eq 0) 'Container removal and pruning must never implicitly remove volumes.'

$script:scenario = 'valid'; $script:identityReads = 0; $script:mutations.Clear(); $script:containers = @(); $script:images = @(); $script:remainders = 0
$script:volumes = @('safe-volume', 'plugin-volume', 'nfs-volume', 'external-volume')
$script:volumeKinds = @{ 'plugin-volume' = 'plugin'; 'nfs-volume' = 'remote-mount'; 'external-volume' = 'external-path' }
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0 -and $script:mutations.Contains('volume rm safe-volume')) 'Verified local volume removal must report incomplete when other volumes remain.'
Assert-WCDockerTest (@($script:mutations | Where-Object { $_ -match 'plugin-volume|nfs-volume|external-volume|--volumes|volume prune' }).Count -eq 0) 'Plugin, remote-backed and external-path volumes must never be removed.'
$script:volumeKinds = @{}

$script:scenario = 'cache'; $script:identityReads = 0; $script:mutations.Clear(); $script:remainders = 0
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0) 'Remaining build cache must make the result incomplete.'

$script:scenario = 'retained'; $script:identityReads = 0; $script:mutations.Clear(); $script:volumes = @('unremoved-volume')
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0) 'Remaining Docker volumes must make the result incomplete.'

$script:scenario = 'valid'; $script:identityReads = 0; $script:mutations.Clear(); $script:volumes = @()
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -eq 0) 'Only a fully verified empty provider and artifact inventory may succeed.'

$script:scenario = 'denied'; $script:identityReads = 0; $script:mutations.Clear(); $script:containers = @('a' * 64)
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0) 'Denied Docker actions must report failure.'

$script:scenario = 'remote'; $script:identityReads = 0; $script:mutations.Clear()
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0 -and $script:mutations.Count -eq 0) 'A non-Desktop engine must not be modified.'

$script:scenario = 'changed'; $script:identityReads = 0; $script:mutations.Clear()
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0 -and $script:mutations.Count -eq 0) 'A changing engine identity must prevent mutation.'

$script:scenario = 'valid'; $script:identityReads = 0; $script:mutations.Clear(); $script:containers = @('invalid-container-id')
$result = Invoke-WCDockerDesktopCleanup
Assert-WCDockerTest ($result.failed -gt 0 -and $script:mutations.Count -eq 0) 'Invalid container identities must fail closed.'

$script:scenario = 'valid'; $script:identityReads = 0; $script:mutations.Clear(); $script:containers = @(); $script:images = @(); $script:volumes = @(); $script:remainders = 1
$script:AutoEraseRemoved = 0; $script:AutoEraseFailed = 0
Invoke-Expression $script:AutoEraseScripts['dockerDesktopData']
Assert-WCDockerTest ($script:AutoEraseFailed -gt 0) 'The scheduled payload must preserve an incomplete cleanup result.'
$capabilities = Get-AutoEraseSupportedCategories
Assert-WCDockerTest ('dockerDesktopData' -in $capabilities.schedulableCategories -and 'dockerDesktopData' -notin $capabilities.categories) 'Docker cleanup must support individual schedules without bulk auto-set.'
Assert-WCDockerTest ('dockerDesktopData' -in $script:AutoEraseHourlyCategories -and 'dockerDesktopData' -notin $script:AutoEraseSystemCategories) 'Docker schedules must be user-scoped and no more frequent than hourly.'
$unconfirmed = Set-AutoEraseScheduleCore -CategoryId 'dockerDesktopData' -IntervalMinutes 60
Assert-WCDockerTest $unconfirmed.error 'Docker schedules must require explicit per-card authorization.'
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('wincommander-docker-schedule-test-' + [guid]::NewGuid())
$null = New-Item -ItemType Directory -Path $fixtureRoot -ErrorAction Stop
function Initialize-AutoEraseStorage { return @{ scripts = $fixtureRoot } }
try {
    $null = ConvertTo-AutoEraseTaskArgument -CategoryId 'dockerDesktopData' -IntervalMinutes 60 -Script $script:AutoEraseScripts['dockerDesktopData'] -ExecutionScope 'fixture-user' -FirstDueUtc ([DateTime]::UtcNow.ToString('o')) -AllowDefaultExcluded $true
    $generated = Join-Path $fixtureRoot 'dockerDesktopData.fixture-user.ps1'
    $source = [IO.File]::ReadAllText($generated)
    Assert-WCDockerTest ($source.Contains('function Invoke-WCDockerDesktopCleanup') -and $source.Contains('Invoke-WCDockerDesktopCleanup')) 'Scheduled script must embed the Docker provider and action.'
    $tokens = $null; $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile($generated, [ref]$tokens, [ref]$errors)
    Assert-WCDockerTest ($errors.Count -eq 0) 'Scheduled Docker script must parse without errors.'
} finally {
    $absolute = [IO.Path]::GetFullPath($fixtureRoot)
    $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (-not $absolute.StartsWith($temp, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($absolute) -notlike 'wincommander-docker-schedule-test-*') { throw 'Unexpected test fixture path.' }
    $generated = Join-Path $absolute 'dockerDesktopData.fixture-user.ps1'
    if ([IO.File]::Exists($generated)) { [IO.File]::Delete($generated) }
    [IO.Directory]::Delete($absolute, $false)
}
Write-Output 'Passed: local Desktop identity, isolated environment, provider cleanup and truthful incomplete status fixtures.'
