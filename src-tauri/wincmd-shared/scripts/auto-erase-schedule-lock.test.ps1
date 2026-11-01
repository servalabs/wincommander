$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot 'auto-erase.ps1'
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $path), [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Scheduler source did not parse.' }
$definition = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke-AutoEraseScheduleLock' }, $true).Extent.Text
$lockId = 'Synthetic' + [Guid]::NewGuid().ToString('N')
$worker = {
    param($Source,$Id)
    . ([scriptblock]::Create($Source))
    function Get-AutoEraseCategoryCode { param($CategoryId) $CategoryId }
    Invoke-AutoEraseScheduleLock -CategoryId $Id -Operation {
        $start = [DateTime]::UtcNow.Ticks
        Start-Sleep -Milliseconds 1200
        @{ start=$start; end=[DateTime]::UtcNow.Ticks }
    }
}
$jobs = @(Start-Job -ScriptBlock $worker -ArgumentList $definition,$lockId; Start-Job -ScriptBlock $worker -ArgumentList $definition,$lockId)
try {
    $results = @($jobs | Receive-Job -Wait -ErrorAction Stop)
    if ($results.Count -ne 2 -or $results[0].error -or $results[1].error) { throw 'Synthetic schedule lock workers did not complete.' }
    if (-not ($results[0].end -le $results[1].start -or $results[1].end -le $results[0].start)) { throw 'Two updates ran concurrently for one cleanup category.' }
} finally { $jobs | Remove-Job -Force -ErrorAction SilentlyContinue }
Write-Output 'Passed: two synthetic scheduling updates for one category are serialized.'
