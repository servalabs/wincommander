$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$catalogOutput = & bun -e "import { STANDARD_CATEGORIES, DEEP_DFIR_CATEGORIES } from './src/panels/cleanup/cleanupCategories.ts'; console.log(JSON.stringify([...STANDARD_CATEGORIES, ...DEEP_DFIR_CATEGORIES].map(c => ({id:c.id,schedulable:!!c.schedulable,schedulerId:c.schedulerCategoryId,min:c.minIntervalMinutes,system:!!c.schedulerRunAsSystem}))))"
if ($LASTEXITCODE -ne 0) { throw 'Could not read the public cleanup catalog.' }
$catalog = $catalogOutput | ConvertFrom-Json
$manualOnlyOutput = & bun -e "import { MANUAL_ONLY_SCHEDULE_IDS } from './src/panels/cleanup/cleanupManualPolicy.ts'; console.log(JSON.stringify(MANUAL_ONLY_SCHEDULE_IDS))"
if ($LASTEXITCODE -ne 0) { throw 'Could not read the manual-only scheduling policy.' }
$frontendManualOnly = @($manualOnlyOutput | ConvertFrom-Json)
if (@(Compare-Object ($frontendManualOnly | Sort-Object) ($script:AutoEraseManualOnlyCategories | Sort-Object)).Count) { throw 'Frontend/backend manual-only schedule policies differ.' }
$protectedOutput = & bun -e "import { DEFAULT_BULK_CLEAR_EXCLUDES } from './src/panels/cleanup/cleanupManualPolicy.ts'; console.log(JSON.stringify(DEFAULT_BULK_CLEAR_EXCLUDES))"
if ($LASTEXITCODE -ne 0) { throw 'Could not read the default-excluded scheduling policy.' }
$frontendProtected = [string[]]($protectedOutput | ConvertFrom-Json)
if ($frontendProtected.Count -ne 10 -or @(Compare-Object ($frontendProtected | Sort-Object) ($script:AutoEraseProtectedCategories | Sort-Object)).Count) { throw 'Frontend/backend default-excluded schedule policies differ.' }
$catalogIds = @($catalog | ForEach-Object id)
$support = Get-AutoEraseSupportedCategories
$manual = @($support.schedulableCategories)
foreach ($id in $frontendProtected) {
    $card = @($catalog | Where-Object id -eq $id)
    if ($card.Count -ne 1 -or -not $card[0].schedulable -or $id -notin $manual -or -not $script:AutoEraseScripts.ContainsKey($id)) { throw "Default-excluded card cannot be individually scheduled: $id" }
}
$unsupported = @($support.unsupportedCategories | ForEach-Object categoryId)
if ($catalog.Count -ne 80) { throw 'Expected all 80 scan-and-clear catalog entries.' }
$classified = @($manual) + @($unsupported)
if (@(Compare-Object ($catalogIds | Sort-Object) ($classified | Sort-Object)).Count) { throw 'A cleanup card is missing, duplicated, or invented in scheduling coverage.' }
foreach ($card in $catalog) {
    $id = $card.id
    $schedulerId = if ($id -eq 'clipboardHistory') { 'clipboard' } else { $id }
    if (-not $script:AutoEraseCategoryCodes.Contains($schedulerId)) { throw "A catalog card has no stable task code: $id" }
    if ($id -in $manual -and -not $script:AutoEraseScripts.ContainsKey($schedulerId)) { throw "An advertised category has no payload: $id" }
    if ($card.schedulable -ne ($id -in $manual)) { throw "Frontend/backend schedule availability disagrees for $id" }
    if ($card.schedulable) {
        if ($card.schedulerId -ne $schedulerId) { throw "Frontend scheduler identity disagrees for $id" }
        if ($card.system -ne ($schedulerId -in $script:AutoEraseSystemCategories)) { throw "Frontend execution account disagrees for $id" }
        $backendMinimum = if ($schedulerId -in $script:AutoEraseHourlyCategories) { 60 } else { 1 }
        if ($card.min -ne $backendMinimum) { throw "Frontend/backend interval floor disagrees for $id" }
    }
}
if (@($script:AutoEraseCategoryCodes.Values | Group-Object | Where-Object Count -gt 1).Count) { throw 'Task codes are duplicated.' }
foreach ($id in $script:AutoEraseScripts.Keys) {
    $parseErrors = $null
    $null = [Management.Automation.Language.Parser]::ParseInput($script:AutoEraseScripts[$id], [ref]$null, [ref]$parseErrors)
    if ($parseErrors.Count) { throw "Payload syntax is invalid for $id" }
}
if (@($support.categories | Where-Object { $_ -in $script:AutoEraseProtectedCategories }).Count) { throw 'Auto-set advertises a default-excluded card.' }
if (@($support.categories | Where-Object { $_ -in $script:AutoEraseManualOnlyCategories }).Count) { throw 'Auto-set advertises an individual-only card.' }
Write-Output "Passed: all 80 cards classified; $($manual.Count) manual payloads, $($support.categories.Count) auto-set, $($unsupported.Count) explicit unsupported reasons; unique task codes and valid payload syntax."
