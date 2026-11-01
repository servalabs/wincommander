$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'auto-erase.ps1')
$savedProgramData = $env:ProgramData
try {
    $env:ProgramData = 'C:\FixtureProgramData'
    $script:mode = 'valid'
    function Get-Item {
        param([string]$LiteralPath, [switch]$Force, $ErrorAction)
        if ($LiteralPath -like '*.ps1') {
            if ($script:mode -eq 'missing') { throw [System.Management.Automation.ItemNotFoundException]::new('Synthetic absent payload.') }
            if ($script:mode -eq 'denied') { throw [UnauthorizedAccessException]::new('Synthetic denied payload.') }
            return [pscustomobject]@{ PSIsContainer=$false; Attributes=if ($script:mode -eq 'reparse') { [IO.FileAttributes]::ReparsePoint } else { [IO.FileAttributes]::Normal }; Length=100 }
        }
        return [pscustomobject]@{ PSIsContainer=$true; Attributes=[IO.FileAttributes]::Directory }
    }
    function Get-Content {
        param([string]$LiteralPath, [switch]$Raw, $ErrorAction)
        if ($script:mode -eq 'unreadable') { throw [UnauthorizedAccessException]::new('Synthetic denied read.') }
        if ($script:mode -eq 'wrongCategory') { return '$categoryId = ''browserFootprints''' }
        return '$categoryId = ''dnsCache'''
    }
    $task = [pscustomobject]@{ Actions=@([pscustomobject]@{ Arguments='-NoProfile -File "C:\FixtureProgramData\WinCommander\auto-erase\scripts\dnsCache.user-Test.ps1"' }) }
    $identity = @{ categoryId='dnsCache' }
    if (-not (Test-AutoEraseCurrentPayload $task $identity)) { throw 'Valid current payload was not verified.' }
    $script:mode = 'missing'
    if (Test-AutoEraseCurrentPayload $task $identity) { throw 'A missing payload was treated as verified.' }
    foreach ($case in @('denied','reparse','unreadable','wrongCategory')) {
        $script:mode = $case; $failed = $false
        try { $null = Test-AutoEraseCurrentPayload $task $identity } catch { $failed = $true }
        if (-not $failed) { throw "Unsafe payload case '$case' did not fail closed." }
    }
    Write-Output 'Passed: current task payload existence, ownership, readability and category verification.'
} finally { $env:ProgramData = $savedProgramData }
