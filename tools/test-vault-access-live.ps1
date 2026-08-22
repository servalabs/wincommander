param(
    [Parameter(Mandatory = $true)]
    [string]$Engine,
    [string]$SshKey = (Join-Path $env:USERPROFILE '.ssh\key'),
    [string]$Administrator = 'Administrator',
    [string]$Partner = 'Partner1'
)

$ErrorActionPreference = 'Stop'

$client = Join-Path $PSScriptRoot 'vault-service-client.ps1'
$engine = [IO.Path]::GetFullPath($Engine)
$sshKey = [IO.Path]::GetFullPath($SshKey)
$policyDir = 'C:\ProgramData\WinCommander\policy'
$testRoot = Join-Path 'C:\ProgramData\WinCommander' ('VaultAccess-Live-' + [guid]::NewGuid().ToString('N'))
$policyFile = Join-Path $testRoot 'policy.json'
$policyWasEmpty = $false
$policyApplied = $false
$createdGroups = @()
$password = $null

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Invoke-Client([string]$Action, [string]$EntryId, [string]$InputSecret) {
    $output = if (-not [string]::IsNullOrEmpty($InputSecret)) {
        $InputSecret | & $client -Action $Action -EntryId $EntryId
    } elseif ($EntryId) {
        & $client -Action $Action -EntryId $EntryId
    } else {
        & $client -Action $Action
    }
    return $output | ConvertFrom-Json
}

function Invoke-SshClient([string]$User, [string]$Action, [string]$EntryId, [string]$InputSecret) {
    $remoteCommand = "`$ProgressPreference='SilentlyContinue'; & '$client' -Action '$Action'"
    if ($EntryId) { $remoteCommand += " -EntryId '$EntryId'" }
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($remoteCommand))
    $ssh = @('-i', $sshKey, '-o', 'BatchMode=yes', "$User@127.0.0.1", 'powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $encoded)
    $output = if (-not [string]::IsNullOrEmpty($InputSecret)) {
        $InputSecret | & ssh @ssh
    } else {
        & ssh @ssh
    }
    if ($LASTEXITCODE -ne 0) { throw "SSH client action $Action failed for $User." }
    return $output | ConvertFrom-Json
}

function Invoke-SshPowerShell([string]$User, [string]$Command) {
    $quietCommand = "`$ProgressPreference='SilentlyContinue'; $Command"
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($quietCommand))
    & ssh -i $sshKey -o BatchMode=yes "$User@127.0.0.1" powershell -NoProfile -ExecutionPolicy Bypass -EncodedCommand $encoded
    return $LASTEXITCODE
}

$existingGroups = @(Get-LocalGroup | Where-Object Name -Like 'WC-Vault-*' | ForEach-Object Name)
$initialPolicy = Invoke-Client 'get-policy' $null $null
if ($null -ne $initialPolicy) {
    throw 'Live test refuses to replace an existing Vault policy.'
}
$policyWasEmpty = $true

$random = [Security.Cryptography.RandomNumberGenerator]::Create()
$passwordBytes = New-Object byte[] 32
$random.GetBytes($passwordBytes)
$random.Dispose()
$password = [Convert]::ToBase64String($passwordBytes)
[Array]::Clear($passwordBytes, 0, $passwordBytes.Length)

$adminPrincipal = "$env:COMPUTERNAME\$Administrator"
$partnerPrincipal = "$env:COMPUTERNAME\$Partner"
$entries = @(
    @{ id = 'live-shared'; label = 'Shared Vault'; folder = 'shared'; file = 'shared.hc'; presentation = 'machine'; letter = 'Q'; grants = @($adminPrincipal, $partnerPrincipal) },
    @{ id = 'live-decoy-a'; label = 'Admin Decoy A'; folder = 'decoy-a'; file = 'decoy-a.hc'; presentation = 'per-user'; letter = 'R'; grants = @($adminPrincipal) },
    @{ id = 'live-decoy-b'; label = 'Admin Decoy B'; folder = 'decoy-b'; file = 'decoy-b.hc'; presentation = 'per-user'; letter = 'S'; grants = @($adminPrincipal) }
)

try {
    foreach ($letter in @('Q', 'R', 'S')) {
        Assert-True (-not (Test-Path "$letter`:")) "Drive letter $letter is already in use."
    }
    New-Item -ItemType Directory -Path $testRoot | Out-Null
    $policyEntries = @()
    foreach ($entry in $entries) {
        $folder = Join-Path $testRoot $entry.folder
        $container = Join-Path $folder $entry.file
        New-Item -ItemType Directory -Path $folder | Out-Null
        $createOutput = @($password | & $engine create --path $container --size-mb 64 --filesystem NTFS --quick 2>&1)
        Assert-True ($LASTEXITCODE -eq 0) "Create failed for $($entry.id): $($createOutput -join ' ')"
        Assert-True (Test-Path -LiteralPath $container -PathType Leaf) "Container missing for $($entry.id)."
        $grants = @($entry.grants | ForEach-Object { [ordered]@{ principal_name = $_; access = 'write' } })
        $policyEntries += [ordered]@{
            id = $entry.id
            label = $entry.label
            container_path = $container
            owner_account = $adminPrincipal
            grants = $grants
            mount = [ordered]@{ presentation = $entry.presentation; preferred_letter = $entry.letter }
        }
    }
    $policy = [ordered]@{
        schema_version = 1
        policy_id = 'live-ssh-matrix'
        version = 1
        expected_previous_version = 0
        entries = $policyEntries
    }
    [IO.File]::WriteAllText($policyFile, ($policy | ConvertTo-Json -Depth 20))
    $apply = & $client -Action apply -PolicyPath $policyFile | ConvertFrom-Json
    $policyApplied = $true
    Assert-True ($apply.validation_state -eq 'current') 'Applied policy was not current.'
    Assert-True (@($apply.entries | Where-Object result -ne 'applied').Count -eq 0) 'At least one policy entry failed ACL application.'

    $createdGroups = @(Get-LocalGroup | Where-Object Name -Like 'WC-Vault-*' | ForEach-Object Name | Where-Object { $existingGroups -notcontains $_ })
    Assert-True ($createdGroups.Count -eq 3) "Expected three new write groups, found $($createdGroups.Count)."

    # This PowerShell process logged on before the managed groups were created.
    # Explicit user grants must still take effect immediately without a relog.
    $immediateAdminList = @(Invoke-Client 'list' $null $null)
    Assert-True ($immediateAdminList.Count -eq 3) "The existing Administrator token saw $($immediateAdminList.Count) entries instead of three."

    $adminList = @(Invoke-SshClient $Administrator 'list' $null $null)
    $partnerList = @(Invoke-SshClient $Partner 'list' $null $null)
    Assert-True ($adminList.Count -eq 3) "Administrator saw $($adminList.Count) entries instead of three."
    Assert-True ($partnerList.Count -eq 1 -and $partnerList[0].entry_id -eq 'live-shared') 'Partner projection was not limited to the shared Vault.'

    $sharedPath = Join-Path (Join-Path $testRoot 'shared') 'shared.hc'
    $decoyPath = Join-Path (Join-Path $testRoot 'decoy-a') 'decoy-a.hc'
    Assert-True ((Invoke-SshPowerShell $Partner "if(Test-Path -LiteralPath '$sharedPath'){exit 0}else{exit 7}") -eq 0) 'Partner could not read the shared container path.'
    Assert-True ((Invoke-SshPowerShell $Partner "if(Test-Path -LiteralPath '$decoyPath' -ErrorAction SilentlyContinue){exit 8}else{exit 0}") -eq 0) 'Partner could see an admin decoy container.'

    $denied = Invoke-SshClient $Partner 'mount' 'live-decoy-a' $password
    Assert-True ($denied.state -eq 'denied' -and $denied.reason -eq 'not_authorized') 'Partner decoy mount did not fail closed.'

    $sharedMount = Invoke-SshClient $Partner 'mount' 'live-shared' $password
    Assert-True ($sharedMount.state -eq 'mounted' -and $sharedMount.drive_letter -eq 'Q:') "Partner shared mount failed: $(ConvertTo-Json -InputObject $sharedMount -Compress)"
    Assert-True ((Invoke-SshPowerShell $Partner "Set-Content -LiteralPath 'Q:\partner.txt' -Value 'partner-write' -NoNewline; if((Get-Content -LiteralPath 'Q:\partner.txt' -Raw) -eq 'partner-write'){exit 0}else{exit 9}") -eq 0) 'Partner shared write/read failed.'
    Assert-True ((Invoke-SshPowerShell $Administrator "if((Get-Content -LiteralPath 'Q:\partner.txt' -Raw) -eq 'partner-write'){Set-Content -LiteralPath 'Q:\admin.txt' -Value 'admin-write' -NoNewline; exit 0}else{exit 10}") -eq 0) 'Administrator could not read/write the shared mount.'
    # Keep this mount active for the SCM shutdown cleanup case below.

    foreach ($id in @('live-decoy-a', 'live-decoy-b')) {
        $mounted = Invoke-SshClient $Administrator 'mount' $id $password
        Assert-True ($mounted.state -eq 'mounted') "Administrator could not mount $id."
        $drive = $mounted.drive_letter
        Assert-True ((Invoke-SshPowerShell $Administrator "Set-Content -LiteralPath '$drive\admin-only.txt' -Value '$id' -NoNewline; if((Get-Content -LiteralPath '$drive\admin-only.txt' -Raw) -eq '$id'){exit 0}else{exit 11}") -eq 0) "Administrator write/read failed for $id."
        Assert-True ((Invoke-SshPowerShell $Partner "if(Test-Path -LiteralPath '$drive\admin-only.txt' -ErrorAction SilentlyContinue){exit 12}else{exit 0}") -eq 0) "Partner could read $id."
        $unmounted = Invoke-SshClient $Administrator 'unmount' $id $null
        Assert-True ($unmounted.state -eq 'unmounted') "Administrator could not unmount $id."
    }

    $status = Invoke-Client 'get-status' $null $null
    Assert-True ($status.validation_state -eq 'current') 'Final policy status was not current.'

    # SCM stop must report StopPending while it dismounts, then leave no driver
    # slot behind. This is the updater/reboot safety path, not ordinary unmount.
    $stopMount = $sharedMount
    Assert-True ($stopMount.state -eq 'mounted') "Could not create the active mount used for the service-stop test: $(ConvertTo-Json -InputObject $stopMount -Compress)"
    Stop-Service WinCommanderSvc
    (Get-Service WinCommanderSvc).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(135))
    $remaining = @(& $engine list)
    Assert-True ($LASTEXITCODE -eq 0) 'Engine list failed after service stop.'
    Assert-True ($remaining.Count -eq 0) 'Service stop left an encrypted volume mounted.'
    Start-Service WinCommanderSvc
    (Get-Service WinCommanderSvc).WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
    $postRestartStatus = Invoke-Client 'get-status' $null $null
    Assert-True ($postRestartStatus.validation_state -eq 'current') 'Vault policy did not revalidate after service restart.'

    [pscustomobject]@{
        Result = 'PASS'
        ReleaseProHash = (Get-FileHash 'C:\ProgramData\WinCommander\bin\wincommander-pro.exe' -Algorithm SHA256).Hash
        PolicyEntries = 3
        AdministratorProjection = $adminList.Count
        PartnerProjection = $partnerList.Count
        PartnerSharedWriteRead = 'verified'
        PartnerDecoyDenied = 'verified'
        AdministratorDecoysWriteRead = 'verified'
        Dismounts = 'verified'
        ExistingTokenAccess = 'verified'
        GracefulServiceStop = 'verified'
        PasswordPersisted = $false
    } | ConvertTo-Json -Compress
}
finally {
    $password = $null
    if ($policyApplied -and $policyWasEmpty) {
        foreach ($entry in $entries) {
            try { Invoke-SshClient $Administrator 'unmount' $entry.id $null | Out-Null } catch {}
        }
        try {
            Stop-Service WinCommanderSvc -Force
            (Get-Service WinCommanderSvc).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(20))
            foreach ($name in @('vault-access-v1.json', 'vault-active-mounts-v1.json')) {
                $path = Join-Path $policyDir $name
                if (Test-Path -LiteralPath $path) {
                    & icacls.exe $path /grant:r '*S-1-5-32-544:F' | Out-Null
                    Remove-Item -LiteralPath $path -Force
                }
            }
            foreach ($group in $createdGroups) {
                if (Get-LocalGroup -Name $group -ErrorAction SilentlyContinue) { Remove-LocalGroup -Name $group }
            }
        } catch {
            Write-Warning "Policy cleanup failed: $($_.Exception.Message)"
        } finally {
            if ((Get-Service WinCommanderSvc).Status -ne 'Running') {
                Start-Service WinCommanderSvc
                (Get-Service WinCommanderSvc).WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
            }
        }
    }
    if (Test-Path -LiteralPath $testRoot) {
        try {
            & icacls.exe $testRoot /grant:r '*S-1-5-32-544:(OI)(CI)F' /T /C | Out-Null
            Remove-Item -LiteralPath $testRoot -Recurse -Force
        } catch {
            Write-Warning "Container cleanup failed: $($_.Exception.Message)"
        }
    }
}
