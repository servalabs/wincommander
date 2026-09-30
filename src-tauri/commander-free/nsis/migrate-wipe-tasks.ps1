param([Parameter(Mandatory)][string]$ModulePath)
$ErrorActionPreference = 'Stop'
try {
    # This module defines scheduling helpers only. Never launch a cleanup task.
    . $ModulePath
    $result = Invoke-AutoEraseMigration
    if ($result.error) { throw $result.message }
    Write-Output ('Scheduled cleanup task migration completed: {0}' -f @($result.migrated).Count)
    exit 0
} catch {
    Write-Output ('Scheduled cleanup task migration failed: {0}' -f $_.Exception.Message)
    exit 1
}
