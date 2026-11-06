. "$PSScriptRoot/../src-tauri/commander-free/nsis/paired-service-image.ps1" -Action Backup -ServicePath x -BackupPath y
$root = Join-Path $env:TEMP ("wc-service-fixture-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $old = Join-Path $root 'old.exe'; $backup = Join-Path $root 'backup.exe'
    [IO.File]::WriteAllText($old, 'old service fixture')
    $hash = Copy-VerifiedServiceImage $old $backup
    [IO.File]::WriteAllText($old, 'new service fixture')
    [void](Copy-VerifiedServiceImage $backup $old $hash)
    if ([IO.File]::ReadAllText($old) -cne 'old service fixture') { throw 'Restore failed.' }
    [IO.File]::WriteAllText($backup, 'tampered')
    try { Copy-VerifiedServiceImage $backup $old $hash; throw 'Tampered backup accepted.' } catch { if ($_.Exception.Message -eq 'Tampered backup accepted.') { throw } }
    if ([IO.File]::ReadAllText($old) -cne 'old service fixture') { throw 'Tamper changed original.' }
    Write-Host 'Service image backup and tamper fixture passed.'
} finally {
    Remove-Item -LiteralPath $old, $backup -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $root -Force -ErrorAction SilentlyContinue
}
