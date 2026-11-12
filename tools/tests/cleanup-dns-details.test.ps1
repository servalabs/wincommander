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
        $data = if ($script:dnsValue) { $script:dnsValue } else { 'fixture' }
        [pscustomobject]@{ Entry=$name; Data=$data; RecordType=1; Status=0; TimeToLive=30; Section=1; DataLength=4; TimeStamp=0 }
    }
}
$local = Get-DnsCacheEntries
if ($local.error -or @($local.entries).Count -ne 5) { throw 'Local DNS details did not retain every returned entry.' }
$fleet = Get-DnsCacheEntries -FleetFiltered
if ($fleet.error -or @($fleet.entries).Count -ne 1 -or $fleet.entries[0].name -ne 'ordinary.fixture.invalid') { throw 'Fleet DNS filtering changed.' }
$script:denyDns = $true
if (-not (Get-DnsCacheEntries).error) { throw 'An unavailable DNS provider was reported as an empty cache.' }
$fleetCollector = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-FleetForensicProjection' }, $true)
$projection = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'New-FleetForensicProjection' }, $true)
if (-not $fleetCollector -or -not $projection) { throw 'Fleet projection functions not found.' }
. ([scriptblock]::Create($projection.Extent.Text))
. ([scriptblock]::Create($fleetCollector.Extent.Text))
$script:denyDns = $false
$script:dnsValue = ([string][char]0x00E9) * 300
$fleetView = Get-FleetForensicProjection -Category 'dns_cache'
if ($fleetView.error -or $fleetView.total -ne 5 -or $fleetView.shown -ne 5) { throw 'Fleet did not retain the local DNS collector scope.' }
if ($fleetView.records[0].data.Length -gt 256 -or -not $fleetView.records[0].data.EndsWith([string][char]0x2026)) { throw 'Long values must fit the protocol limit and visibly indicate truncation.' }
Write-Output 'Passed: Fleet DNS matches the local collector scope and bounds long values.'
