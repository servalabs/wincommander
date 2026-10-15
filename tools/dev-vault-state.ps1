[CmdletBinding()]
param([switch]$InspectOnly)

function Get-WcDevelopmentVaultState {
    # This is a preparation decision, never proof of native driver readiness.
    try {
        $registryPath = Join-Path $env:ProgramData 'WinCommander\policy\vault-active-mounts-v1.json'
        if (-not (Test-Path -LiteralPath $registryPath -PathType Leaf -ErrorAction Stop)) {
            $service = Get-Service -Name WinCommanderSvc -ErrorAction SilentlyContinue
            $desktop = Get-Process wincommander-free, wincommander-pro, WinCommander -ErrorAction SilentlyContinue
            if ($service -or $desktop) {
                return [pscustomobject]@{ state = 'unknown'; mountedCount = 0 }
            }
            return [pscustomobject]@{ state = 'free'; mountedCount = 0 }
        }
        $file = [IO.FileInfo]::new($registryPath)
        if ($file.Length -gt 4MB) { throw 'Unbounded mount journal' }
        $registry = Get-Content -LiteralPath $registryPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
        if ($null -eq $registry -or $null -eq $registry.PSObject.Properties['mounts'] -or
            $registry.mounts -isnot [pscustomobject]) { throw 'Invalid mount journal' }
        $count = @($registry.mounts.PSObject.Properties).Count
        if ($count -gt 26) { throw 'Invalid mount count' }
        return [pscustomobject]@{ state = $(if ($count -gt 0) { 'mounted' } else { 'free' }); mountedCount = $count }
    }
    catch {
        return [pscustomobject]@{ state = 'unknown'; mountedCount = 0 }
    }
}

if ($InspectOnly) { Get-WcDevelopmentVaultState | ConvertTo-Json -Compress }
