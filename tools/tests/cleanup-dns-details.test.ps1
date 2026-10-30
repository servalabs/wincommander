$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '..\..\src-tauri\commander-free\scripts\modules\privacy\cleanup.ps1'
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $source).Path, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Could not parse cleanup module.' }
$definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-DnsCacheEntries' }, $true)
. ([scriptblock]::Create($definition.Extent.Text))
function Test-Path { param($Path) return $true }
function Get-Content { param($Path, $ErrorAction) return @('# WINCOMMANDER_BLOCKLIST_START', '0.0.0.0 blocked.fixture.invalid', '# WINCOMMANDER_BLOCKLIST_END') }
function Get-DnsClientCache {
    param($ErrorAction)
    if ($script:denyDns) { throw 'DNS provider unavailable' }
    foreach ($name in @('ordinary.fixture.invalid', 'sub.blocked.fixture.invalid', '1.0.0.127.in-addr.arpa', '0.0.0.0', '.')) {
        [pscustomobject]@{ Entry=$name; Data='fixture'; RecordType=1; Status=0; TimeToLive=30; Section=1; DataLength=4; TimeStamp=0 }
    }
}
$local = Get-DnsCacheEntries
if ($local.error -or @($local.entries).Count -ne 5) { throw 'Local DNS details did not retain every returned entry.' }
$fleet = Get-DnsCacheEntries -FleetFiltered
if ($fleet.error -or @($fleet.entries).Count -ne 1 -or $fleet.entries[0].name -ne 'ordinary.fixture.invalid') { throw 'Fleet DNS filtering changed.' }
$script:denyDns = $true
if (-not (Get-DnsCacheEntries).error) { throw 'An unavailable DNS provider was reported as an empty cache.' }
$fleetCollector = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FleetForensicProjection' }, $true)
if (-not $fleetCollector -or $fleetCollector.Extent.Text -notmatch 'Get-DnsCacheEntries\s+-FleetFiltered') { throw 'Fleet must request its existing filtered scope.' }
Write-Output 'Passed: local DNS details retain records and Fleet keeps its prior scope.'
