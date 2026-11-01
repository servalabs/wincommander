$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$date = [datetime]::SpecifyKind([datetime]'2026-10-30T14:25:00', [DateTimeKind]::Local)
if ((ConvertTo-AutoEraseTimestamp $date) -ne $date.ToString('o', [Globalization.CultureInfo]::InvariantCulture)) { throw 'Task dates must use stable ISO format.' }
foreach ($value in @($null, [datetime]::MinValue, [datetime]'1899-12-30', [datetime]'1999-11-30', 'invalid')) {
    if ($null -ne (ConvertTo-AutoEraseTimestamp $value)) { throw 'Missing or default task dates must be unavailable.' }
}
Write-Output 'Passed: task time ISO formatting and missing/default timestamps.'
