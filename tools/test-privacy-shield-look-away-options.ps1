$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot '..\src-tauri\commander-free\scripts\modules\privacy\privacy_shield.ps1')
$base = 'python --wc-privacy-shield --model-level medium --confidence 0.5 --overlay-opacity 200 --wake-delay-ms 150 --device-wake-multiplier 5 --multi-face-wake-multiplier 5 --buffer-frames 2 --capture-speed 1 --check-gaze --blur-gaze'
$old = Get-PrivacyShieldNativeOptions -CommandLine $base
if ($null -eq $old -or $old.ContainsKey('lookAwayDelaySeconds')) { throw 'An older detector must report the delay as unknown.' }
$updated = Get-PrivacyShieldNativeOptions -CommandLine "$base --look-away-delay-seconds 10"
if ($null -eq $updated -or $updated.lookAwayDelaySeconds -ne 10) { throw 'The detector delay was not read back.' }
$zero = Get-PrivacyShieldNativeOptions -CommandLine "$base --look-away-delay-seconds 0"
if ($null -eq $zero -or $zero.lookAwayDelaySeconds -ne 0) { throw 'An explicit immediate delay was lost.' }
if ($null -ne (Get-PrivacyShieldNativeOptions -CommandLine "$base --look-away-delay-seconds 10 --look-away-delay-seconds 20")) { throw 'Duplicate delay flags must be rejected.' }
if ($null -ne (Get-PrivacyShieldNativeOptions -CommandLine "$base --look-away-delay-seconds invalid")) { throw 'Malformed delay must be rejected.' }
$noChecks = $base -replace ' --check-gaze', ''
if (-not (Get-PrivacyShieldNativeOptions -CommandLine $noChecks).checkGaze) { throw 'Legacy no-option launch must report its default gaze detector.' }
if ((Get-PrivacyShieldNativeOptions -CommandLine "$noChecks --detector-options-explicit").checkGaze) { throw 'Explicitly disabled gaze must remain disabled in readback.' }
if ($null -ne (Get-PrivacyShieldNativeOptions -CommandLine "$noChecks --detector-options-explicit --detector-options-explicit")) { throw 'Duplicate detector options marker must be rejected.' }
Write-Host 'Privacy Shield look-away native-option checks passed.'
