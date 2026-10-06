$ErrorActionPreference = 'Stop'

$modulePath = Join-Path $PSScriptRoot 'volumes.ps1'

# This narrowly replaces the Storage cmdlet only inside this test process. It
# proves a provider failure cannot become the same empty result as a genuine
# machine with no eligible partitions.
function global:Get-Partition {
    param([Parameter(ValueFromRemainingArguments = $true)] $Arguments)
    throw 'simulated storage provider failure'
}

. $modulePath

try {
    Get-EncryptionPartitions | Out-Null
    throw 'expected partition discovery to fail'
}
catch {
    if ($_.Exception.Message -ne 'vault_partition_list_unavailable') {
        throw
    }
}

function global:Get-Partition {
    param([Parameter(ValueFromRemainingArguments = $true)] $Arguments)
    @()
}

$empty = Get-EncryptionPartitions
if (@($empty.partitions).Count -ne 0) {
    throw 'an actual empty partition list must remain an empty result'
}

Write-Host 'PASS: partition discovery distinguishes provider failure from an empty list'
