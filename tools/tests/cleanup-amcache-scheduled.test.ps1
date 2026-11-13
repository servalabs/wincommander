$ErrorActionPreference = 'Stop'
$source = Get-Content (Join-Path $PSScriptRoot '../../src-tauri/wincmd-shared/scripts/auto-erase.ps1') -Raw
$tokens = $null; $errors = $null
$outer = [System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Auto-erase source does not parse.' }
$assignment = $outer.Find({ param($node) $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$script:EraseFunctions' }, $true)
if (-not $assignment) { throw 'Auto-erase function template not found.' }
$template = $assignment.Right.Extent.Text
if (-not $template.StartsWith("@'") -or -not $template.EndsWith("'@")) { throw 'Unexpected function template.' }
$functions = $template.Substring(2, $template.Length - 4)
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseInput($functions, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Auto-erase function template does not parse.' }
foreach ($name in @('Test-AutoErasePendingDelete', 'Invoke-AutoEraseAmcache')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $true)
    if (-not $definition) { throw "Missing $name." }
    . ([scriptblock]::Create($definition.Extent.Text))
}
if ($source -notmatch "(?s)'amcache'\s*=\s*@'\s*Invoke-AutoEraseAmcache\s*'@") { throw 'Amcache schedule does not use the deferred handler.' }
$script:mockFiles = @(
    [pscustomobject]@{ Name='Amcache.hve'; FullName='C:\Windows\AppCompat\Programs\Amcache.hve' },
    [pscustomobject]@{ Name='Amcache.hve.LOG1'; FullName='C:\Windows\AppCompat\Programs\Amcache.hve.LOG1' },
    [pscustomobject]@{ Name='Amcache.hve.backup'; FullName='C:\Windows\AppCompat\Programs\Amcache.hve.backup' }
)
$script:ops = @('\??\C:\unrelated.tmp', '')
$script:addCount=0; $script:failAt=0; $script:dropReadback=$false
$script:AutoEraseDeferred=0; $script:AutoEraseFailed=0; $script:AutoEraseCoverageIncomplete=$false
function Get-AutoEraseFiles { param($Path, $Filter) return $script:mockFiles }
function Get-AutoErasePendingOperations { return $script:ops }
function Register-AutoErasePendingDelete([string]$Path) {
    $script:addCount++
    if ($script:failAt -eq $script:addCount) { throw 'Synthetic scheduling failure' }
    if (-not $script:dropReadback) { $script:ops += ('\??\' + $Path); $script:ops += '' }
}
Invoke-AutoEraseAmcache
if ($script:AutoEraseDeferred -ne 2 -or $script:AutoEraseFailed -ne 0 -or $script:ops.Count -ne 6 -or $script:ops[0] -ne '\??\C:\unrelated.tmp' -or $script:addCount -ne 2) { throw 'Verified scheduling did not preserve unrelated operations or excluded backups.' }
Invoke-AutoEraseAmcache
if ($script:addCount -ne 2) { throw 'Already scheduled operations were duplicated.' }
$script:ops=@('\??\C:\unrelated.tmp', ''); $script:addCount=0; $script:failAt=2
$script:AutoEraseDeferred=0; $script:AutoEraseFailed=0; $script:AutoEraseCoverageIncomplete=$false
Invoke-AutoEraseAmcache
if ($script:AutoEraseDeferred -ne 1 -or $script:AutoEraseFailed -ne 1 -or -not $script:AutoEraseCoverageIncomplete) { throw 'Partial scheduling was not reported as incomplete.' }
$script:ops=@(); $script:addCount=0; $script:failAt=0; $script:dropReadback=$true
$script:AutoEraseDeferred=0; $script:AutoEraseFailed=0; $script:AutoEraseCoverageIncomplete=$false
Invoke-AutoEraseAmcache
if ($script:AutoEraseDeferred -ne 0 -or $script:AutoEraseFailed -ne 1 -or -not $script:AutoEraseCoverageIncomplete) { throw 'An unconfirmed deletion was accepted.' }
$script:mockFiles=@($script:mockFiles | Where-Object { $_.Name -ne 'Amcache.hve' }); $script:addCount=0
$script:AutoEraseDeferred=0; $script:AutoEraseFailed=0; $script:AutoEraseCoverageIncomplete=$false
Invoke-AutoEraseAmcache
if ($script:addCount -ne 0 -or $script:AutoEraseFailed -ne 1 -or -not $script:AutoEraseCoverageIncomplete) { throw 'A missing hive did not fail closed.' }
$migration = $outer.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-AutoEraseMigrationCore' }, $true)
if (-not $migration) { throw 'Migration handler not found.' }
. ([scriptblock]::Create($migration.Extent.Text))
function Assert-AutoEraseAdmin {}
function Assert-AutoEraseKnownOwners { param($Tasks,$CategoryId) }
function Initialize-SystemMaintenanceTaskFolder {}
function Get-ScheduledTask { [CmdletBinding()] param($TaskPath,$TaskName) return [pscustomobject]@{ Description='legacy' } }
function Get-AutoEraseTaskIdentity { param($Task,[switch]$AllowUnknownOwner) return @{ categoryId='amcache'; ownerSid='S-1-5-18'; taskName='existing' } }
function Test-AutoEraseCurrentPayload { param($Task,$Identity) return $script:currentPayload }
function Disable-ScheduledTask { throw 'A live task must not be changed.' }
function Register-ScheduledTask { throw 'A live task must not be changed.' }
function Unregister-ScheduledTask { throw 'A live task must not be changed.' }
$script:currentPayload=$false
$legacy = Invoke-AutoEraseMigrationCore -CategoryId 'amcache'
if (-not $legacy.error -or $legacy.message -notmatch 'may still run' -or $legacy.message -notmatch 'left unchanged') { throw 'A legacy Amcache schedule was not surfaced without mutation.' }
$script:currentPayload=$true
$current = Invoke-AutoEraseMigrationCore -CategoryId 'amcache'
if ($current.error -or $current.status -ne 'ok') { throw 'An already-current Amcache schedule was not left unchanged.' }
Write-Output 'Passed: scheduled Amcache queues only verified deferred deletions.'
