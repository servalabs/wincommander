param(
    [Parameter(Mandatory = $true)]
    [string]$Engine,
    [string]$SshKey = (Join-Path $env:USERPROFILE '.ssh\key'),
    [string]$Administrator = 'Administrator',
    [string]$Partner = 'Partner1',
    [string]$GroupMember = 'Sales1'
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
# Test-only local group used to prove group-principal grants (CheckTokenMembership
# against a group SID), tracked and removed exactly like $createdGroups below.
# The name is distinctive on purpose so it can never collide with a service-owned
# "WC-Vault-*" managed group or with a real "WC_Sales" / "WC_Accounting" / "WC_Partner"
# group that already exists on the target machine.
$testGroupName = "WCLiveTest-GroupGrant-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
$testCreatedGroup = $null

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

function Expand-ListResult($Value) {
    if ($null -eq $Value) { return }
    $items = if ($Value -is [System.Array]) {
        $Value
    } elseif ($Value.PSObject.Properties.Match('entries').Count -eq 1) {
        @($Value.entries)
    } elseif ($Value.PSObject.Properties.Match('items').Count -eq 1) {
        @($Value.items)
    } else {
        @($Value)
    }
    foreach ($item in $items) {
        Write-Output $item
    }
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

# Fail fast, next to the drive-letter and existing-policy guards above/below,
# rather than surfacing a missing account as a confusing Add-LocalGroupMember
# error deep into the run after containers have already been created.
Assert-True ($Partner -ne $GroupMember) "-Partner and -GroupMember must name different accounts (both are currently '$Partner')."
Assert-True ($null -ne (Get-LocalUser -Name $Partner -ErrorAction SilentlyContinue)) "-Partner account '$Partner' does not exist locally on $env:COMPUTERNAME."
Assert-True ($null -ne (Get-LocalUser -Name $GroupMember -ErrorAction SilentlyContinue)) "-GroupMember account '$GroupMember' does not exist locally on $env:COMPUTERNAME."

$random = [Security.Cryptography.RandomNumberGenerator]::Create()
$passwordBytes = New-Object byte[] 32
$random.GetBytes($passwordBytes)
$random.Dispose()
$password = [Convert]::ToBase64String($passwordBytes)
[Array]::Clear($passwordBytes, 0, $passwordBytes.Length)

$adminPrincipal = "$env:COMPUTERNAME\$Administrator"
$partnerPrincipal = "$env:COMPUTERNAME\$Partner"
$testGroupPrincipal = "$env:COMPUTERNAME\$testGroupName"
Assert-True ([Security.Principal.WindowsIdentity]::GetCurrent().Name -ieq $adminPrincipal) "Run this harness from the active $Administrator session."
$entries = @(
    @{ id = 'live-shared'; label = 'Shared Vault'; folder = 'shared'; file = 'shared.hc'; presentation = 'machine'; letter = 'Q'; grants = @($adminPrincipal, $partnerPrincipal); unmountUser = $Partner },
    @{ id = 'live-decoy-a'; label = 'Admin Decoy A'; folder = 'decoy-a'; file = 'decoy-a.hc'; presentation = 'per-user'; letter = 'R'; grants = @($adminPrincipal) },
    @{ id = 'live-decoy-b'; label = 'Admin Decoy B'; folder = 'decoy-b'; file = 'decoy-b.hc'; presentation = 'per-user'; letter = 'S'; grants = @($adminPrincipal) },
    # Grant is a GROUP principal (not a user), exercising the CheckTokenMembership-against-
    # a-group-SID path that a user-principal-only policy never touches.
    @{ id = 'live-group'; label = 'Group Grant Vault'; folder = 'group'; file = 'group.hc'; presentation = 'per-user'; letter = 'T'; grants = @($testGroupPrincipal); unmountUser = $GroupMember }
)

try {
    foreach ($letter in @('Q', 'R', 'S', 'T')) {
        Assert-True (-not (Test-Path "$letter`:")) "Drive letter $letter is already in use."
    }
    New-Item -ItemType Directory -Path $testRoot | Out-Null

    Assert-True (-not (Get-LocalGroup -Name $testGroupName -ErrorAction SilentlyContinue)) "Local group $testGroupName already exists; refusing to reuse it."
    New-LocalGroup -Name $testGroupName -Description 'WinCommander live vault-access harness (group-principal grant coverage). Safe to delete if found stale.' | Out-Null
    $testCreatedGroup = $testGroupName
    Add-LocalGroupMember -Group $testGroupName -Member "$env:COMPUTERNAME\$GroupMember"

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
    Assert-True ($createdGroups.Count -eq 4) "Expected four new write groups (one per policy entry, including the group-grant entry's owner group), found $($createdGroups.Count)."

    # This PowerShell process logged on before the managed groups were created.
    # Explicit user grants must still take effect immediately without a relog.
    $immediateAdminList = @(Expand-ListResult (Invoke-Client 'list' $null $null))
    $immediateAdminIds = @($immediateAdminList | ForEach-Object entry_id) -join ', '
    Assert-True ($immediateAdminList.Count -eq 4) "The existing Administrator token saw $($immediateAdminList.Count) entries instead of four: $immediateAdminIds"

    $adminList = @(Expand-ListResult (Invoke-SshClient $Administrator 'list' $null $null))
    $partnerList = @(Expand-ListResult (Invoke-SshClient $Partner 'list' $null $null))
    $groupMemberList = @(Expand-ListResult (Invoke-SshClient $GroupMember 'list' $null $null))
    Assert-True ($adminList.Count -eq 4) "Administrator saw $($adminList.Count) entries instead of four."
    Assert-True ($partnerList.Count -eq 1 -and $partnerList[0].entry_id -eq 'live-shared') 'Partner projection was not limited to the shared Vault.'
    Assert-True ($groupMemberList.Count -eq 1 -and $groupMemberList[0].entry_id -eq 'live-group') "Group member projection was not limited to the group-granted Vault entry (saw $($groupMemberList.Count) entries)."

    $sharedPath = Join-Path (Join-Path $testRoot 'shared') 'shared.hc'
    $decoyPath = Join-Path (Join-Path $testRoot 'decoy-a') 'decoy-a.hc'
    Assert-True ((Invoke-SshPowerShell $Partner "if(Test-Path -LiteralPath '$sharedPath'){exit 0}else{exit 7}") -eq 0) 'Partner could not read the shared container path.'
    Assert-True ((Invoke-SshPowerShell $Partner "if(Test-Path -LiteralPath '$decoyPath' -ErrorAction SilentlyContinue){exit 8}else{exit 0}") -eq 0) 'Partner could see an admin decoy container.'

    $denied = Invoke-SshClient $Partner 'mount' 'live-decoy-a' $password
    Assert-True ($denied.state -eq 'denied' -and $denied.reason -eq 'not_authorized') 'Partner decoy mount did not fail closed.'

    # Partner is not a member of $testGroupName, so the group-granted entry must
    # deny them too. This proves the group-SID membership check fails closed for
    # a real user who is simply not in the granted group.
    $groupDenied = Invoke-SshClient $Partner 'mount' 'live-group' $password
    Assert-True ($groupDenied.state -eq 'denied' -and $groupDenied.reason -eq 'not_authorized') "A user outside the granted group ($testGroupName) was not denied for the group-granted Vault entry: $(ConvertTo-Json -InputObject $groupDenied -Compress)"

    $sharedMount = Invoke-SshClient $Partner 'mount' 'live-shared' $password
    Assert-True ($sharedMount.state -eq 'mounted' -and $sharedMount.drive_letter -eq 'Q:') "Partner shared mount failed: $(ConvertTo-Json -InputObject $sharedMount -Compress)"
    Assert-True ((Invoke-SshPowerShell $Partner "Set-Content -LiteralPath 'Q:\partner.txt' -Value 'partner-write' -NoNewline; if((Get-Content -LiteralPath 'Q:\partner.txt' -Raw) -eq 'partner-write'){exit 0}else{exit 9}") -eq 0) 'Partner shared write/read failed.'
    Assert-True ((Get-Content -LiteralPath 'Q:\partner.txt' -Raw) -eq 'partner-write') 'Administrator could not read the shared mount.'
    Set-Content -LiteralPath 'Q:\admin.txt' -Value 'admin-write' -NoNewline
    Assert-True ((Get-Content -LiteralPath 'Q:\admin.txt' -Raw) -eq 'admin-write') 'Administrator could not write the shared mount.'
    # Keep this mount active for the SCM shutdown cleanup case below.

    # --- Group-principal grant, happy path -------------------------------------
    # $GroupMember is a member of $testGroupName, which is the sole grant on
    # 'live-group'. This proves CheckTokenMembership against a group SID grants
    # access end to end (mount, then real read/write over the SSH path), exactly
    # the path a group-friendly-name-vs-Windows-group-name mismatch would break.
    $groupMount = Invoke-SshClient $GroupMember 'mount' 'live-group' $password
    Assert-True ($groupMount.state -eq 'mounted' -and $groupMount.drive_letter -eq 'T:') "Group member mount failed for the group-granted Vault entry: $(ConvertTo-Json -InputObject $groupMount -Compress)"
    Assert-True ((Invoke-SshPowerShell $GroupMember "Set-Content -LiteralPath 'T:\group-member.txt' -Value 'group-write' -NoNewline; if((Get-Content -LiteralPath 'T:\group-member.txt' -Raw) -eq 'group-write'){exit 0}else{exit 10}") -eq 0) 'Group member write/read failed on the group-granted Vault entry.'
    $groupUnmount = Invoke-SshClient $GroupMember 'unmount' 'live-group' $null
    Assert-True ($groupUnmount.state -eq 'unmounted') 'Group member could not unmount the group-granted Vault entry.'

    # --- Missing group principal, negative --------------------------------------
    # Apply a policy revision whose only change is a grant naming a group that
    # does not exist. The apply must be rejected (fail closed, no partial
    # application) and the error must name the rejected principal rather than
    # returning the old, unenriched "vault principal resolution failed" constant.
    $missingGroupName = "WCLiveTest-Missing-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    $missingGroupPrincipal = "$env:COMPUTERNAME\$missingGroupName"
    $missingGroupPolicy = $policy | ConvertTo-Json -Depth 20 | ConvertFrom-Json
    $missingGroupPolicy.version = 2
    $missingGroupPolicy.expected_previous_version = 1
    $missingGroupTargetEntry = @($missingGroupPolicy.entries | Where-Object id -eq 'live-group')[0]
    Assert-True ($null -ne $missingGroupTargetEntry) 'Could not locate the live-group entry to mutate for the missing-group negative test.'
    $missingGroupTargetEntry.grants = @($missingGroupTargetEntry.grants) + [pscustomobject]@{ principal_name = $missingGroupPrincipal; access = 'write' }
    $missingGroupPolicyFile = Join-Path $testRoot 'policy-missing-group.json'
    [IO.File]::WriteAllText($missingGroupPolicyFile, ($missingGroupPolicy | ConvertTo-Json -Depth 20))

    $missingGroupApplySucceeded = $false
    $missingGroupError = $null
    try {
        & $client -Action apply -PolicyPath $missingGroupPolicyFile | Out-Null
        $missingGroupApplySucceeded = $true
    } catch {
        $missingGroupError = $_.Exception.Message
    }
    Assert-True (-not $missingGroupApplySucceeded) 'Apply with a nonexistent group principal unexpectedly succeeded; the service should reject unresolved principals before committing.'
    Assert-True (-not [string]::IsNullOrEmpty($missingGroupError)) 'Missing-group apply failed without any error detail from the service.'
    Assert-True ($missingGroupError -like "*$missingGroupName*") "Missing-group apply error did not name the rejected principal '$missingGroupName' (got: '$missingGroupError'). If this is exactly the old 'vault principal resolution failed' constant, VaultError::PrincipalResolution is still not carrying the principal name through pipe.rs."

    $statusAfterMissingGroup = Invoke-Client 'get-status' $null $null
    Assert-True ($statusAfterMissingGroup.validation_state -eq 'current') 'Policy validation state was not current after a rejected apply; a failed apply must not leave the store partially applied.'

    foreach ($id in @('live-decoy-a', 'live-decoy-b')) {
        $mounted = Invoke-Client 'mount' $id $password
        Assert-True ($mounted.state -eq 'mounted') "Administrator could not mount $id."
        $drive = $mounted.drive_letter
        Set-Content -LiteralPath "$drive\admin-only.txt" -Value $id -NoNewline
        Assert-True ((Get-Content -LiteralPath "$drive\admin-only.txt" -Raw) -eq $id) "Administrator write/read failed for $id."
        Assert-True ((Invoke-SshPowerShell $Partner "if(Test-Path -LiteralPath '$drive\admin-only.txt' -ErrorAction SilentlyContinue){exit 12}else{exit 0}") -eq 0) "Partner could read $id."
        $unmounted = Invoke-Client 'unmount' $id $null
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
        PolicyEntries = 4
        AdministratorProjection = $adminList.Count
        PartnerProjection = $partnerList.Count
        GroupMemberProjection = $groupMemberList.Count
        PartnerSharedWriteRead = 'verified'
        PartnerDecoyDenied = 'verified'
        AdministratorDecoysWriteRead = 'verified'
        GroupGrantMountWriteRead = 'verified'
        GroupGrantUnauthorizedDenied = 'verified'
        GroupGrantMissingPrincipalRejected = 'verified'
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
            try {
                if ($entry.unmountUser) {
                    Invoke-SshClient $entry.unmountUser 'unmount' $entry.id $null | Out-Null
                } else {
                    Invoke-Client 'unmount' $entry.id $null | Out-Null
                }
            } catch {}
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
    # $testCreatedGroup is only ever the group this run created itself
    # (tracked at creation time, before any grant referenced it), so removal
    # here never touches a pre-existing group. Independent of policy apply
    # state so a group left over from a step that failed before apply is
    # still cleaned up.
    if ($testCreatedGroup) {
        try {
            if (Get-LocalGroup -Name $testCreatedGroup -ErrorAction SilentlyContinue) {
                Remove-LocalGroup -Name $testCreatedGroup
            }
        } catch {
            Write-Warning "Test group cleanup failed: $($_.Exception.Message)"
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
