$ErrorActionPreference = 'Stop'

$testDataRoot = Join-Path $env:TEMP ("WinCommander-PS7InstallTest-" + [guid]::NewGuid().ToString('N'))
$originalProgramData = $env:ProgramData
$originalProgramFiles = $env:ProgramFiles
$originalProgramW6432 = $env:ProgramW6432
$originalProgramFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)', 'Process')
$originalProcessorArchitecture = $env:PROCESSOR_ARCHITECTURE
$originalProcessorArchW6432 = $env:PROCESSOR_ARCHITEW6432
$env:ProgramData = $testDataRoot

try {
    $repoRoot = Split-Path -Parent $PSScriptRoot
    $dependencyModule = Join-Path $repoRoot 'src-tauri\commander-free\scripts\modules\dependencies\dependencies.ps1'
    . $dependencyModule

    # An MSI installs to Program Files. Verify discovery succeeds from that
    # path even when the already-running process has no pwsh PATH entry.
    $pathProbeRoot = Join-Path $testDataRoot 'ProgramFiles'
    $pathProbeExe = Join-Path $pathProbeRoot 'PowerShell\7\pwsh.exe'
    New-Item -ItemType Directory -Path (Split-Path $pathProbeExe -Parent) -Force | Out-Null
    Set-Content -LiteralPath $pathProbeExe -Value 'stub executable for path discovery contract'
    [Environment]::SetEnvironmentVariable('ProgramFiles', $pathProbeRoot, 'Process')
    [Environment]::SetEnvironmentVariable('ProgramW6432', $pathProbeRoot, 'Process')
    [Environment]::SetEnvironmentVariable('ProgramFiles(x86)', $pathProbeRoot, 'Process')
    $pathProbeStatus = Test-PowerShell7Installed
    if (-not $pathProbeStatus.installed) {
        throw 'PowerShell 7 MSI path was not discoverable without a refreshed PATH.'
    }
    [Environment]::SetEnvironmentVariable('ProgramFiles', $originalProgramFiles, 'Process')
    [Environment]::SetEnvironmentVariable('ProgramW6432', $originalProgramW6432, 'Process')
    [Environment]::SetEnvironmentVariable('ProgramFiles(x86)', $originalProgramFilesX86, 'Process')

    $script:mockPowerShellInstalled = $false
    $script:mockInstallRegistersPowerShell = $true
    $script:mockWingetArgs = @()
    $script:mockWingetExitCode = 0
    $script:mockWingetOutput = 'simulated winget output'
    $script:mockWingetAvailable = $true
    $script:mockAdmin = $true
    $script:mockReleaseMode = 'valid'
    $script:mockReleaseRequests = 0
    $script:mockDownloadUri = $null
    $script:mockMsiInstallCalls = 0
    $script:mockMsiExitCode = 0
    $script:mockSignatureStatus = 'Valid'
    $script:mockMsiBytes = [byte[]](1, 2, 3, 4)

    function Test-IsAdmin { return [bool]$script:mockAdmin }
    function Assert-IsAdmin {
        if (-not (Test-IsAdmin)) { throw 'Administrator privileges required.' }
    }
    function Start-Sleep {
        [CmdletBinding()]
        param([int]$Milliseconds)
    }
    function Test-PowerShell7Installed {
        return @{
            installed = [bool]$script:mockPowerShellInstalled
            version = if ($script:mockPowerShellInstalled) { '7.6.6' } else { $null }
        }
    }
    function Resolve-WingetPath {
        if (-not $script:mockWingetAvailable) { return $null }
        return {
            $script:mockWingetArgs = @($args)
            $script:mockPowerShellInstalled = [bool]$script:mockInstallRegistersPowerShell
            $global:LASTEXITCODE = $script:mockWingetExitCode
            $script:mockWingetOutput
        }
    }
    function Invoke-RestMethod {
        [CmdletBinding()]
        param([string]$Uri, [hashtable]$Headers, [int]$TimeoutSec)
        $script:mockReleaseRequests++
        if ($Uri -cne 'https://api.github.com/repos/PowerShell/PowerShell/releases/latest') {
            throw "Unexpected release API URL: $Uri"
        }
        $assetName = 'PowerShell-7.6.6-win-arm64.msi'
        $assetUrl = "https://github.com/PowerShell/PowerShell/releases/download/v7.6.6/$assetName"
        if ($script:mockReleaseMode -eq 'wrongUrl') { $assetUrl = 'https://example.invalid/fake.msi' }
        return [PSCustomObject]@{
            tag_name = 'v7.6.6'
            prerelease = $false
            assets = @([PSCustomObject]@{
                name = $assetName
                size = $script:mockMsiBytes.Length
                browser_download_url = $assetUrl
            })
        }
    }
    function Invoke-WebRequest {
        [CmdletBinding()]
        param([string]$Uri, [string]$OutFile, [switch]$UseBasicParsing, [int]$TimeoutSec)
        $script:mockDownloadUri = $Uri
        [System.IO.File]::WriteAllBytes($OutFile, $script:mockMsiBytes)
    }
    function Get-AuthenticodeSignature {
        [CmdletBinding()]
        param([string]$FilePath)
        return [PSCustomObject]@{
            Status = $script:mockSignatureStatus
            SignerCertificate = [PSCustomObject]@{ Subject = 'CN=Microsoft Corporation, O=Microsoft Corporation' }
        }
    }
    function Start-Process {
        [CmdletBinding()]
        param(
            [string]$FilePath,
            [string]$ArgumentList,
            [switch]$Wait,
            [switch]$PassThru,
            [string]$WindowStyle
        )
        $script:mockMsiInstallCalls++
        $script:mockMsiArguments = $ArgumentList
        if ($script:mockMsiExitCode -in @(0, 3010)) { $script:mockPowerShellInstalled = $true }
        return [PSCustomObject]@{ ExitCode = $script:mockMsiExitCode }
    }

    $installed = Install-PowerShell7
    if (-not $installed.success -or $installed.message -notmatch 'v7\.6\.6') {
        throw 'PowerShell 7 install did not return the verified version.'
    }
    if ($script:mockWingetArgs -notcontains '--scope' -or $script:mockWingetArgs -notcontains 'machine') {
        throw 'PowerShell 7 install must use the machine MSI scope.'
    }
    if ($script:mockWingetArgs -notcontains '--installer-type' -or $script:mockWingetArgs -notcontains 'wix') {
        throw 'PowerShell 7 install must select the supported Wix/MSI package explicitly.'
    }
    if ($script:mockWingetArgs -notcontains '--source' -or $script:mockWingetArgs -notcontains 'winget') {
        throw 'PowerShell 7 install must select the winget source non-interactively.'
    }

    $script:mockPowerShellInstalled = $false
    $script:mockInstallRegistersPowerShell = $false
    $verificationError = $null
    try {
        $null = Install-PowerShell7
    }
    catch {
        $verificationError = $_.Exception.Message
    }
    if ($verificationError -notmatch 'still not detected') {
        throw 'PowerShell 7 install incorrectly reported success without a detected runtime.'
    }

    $script:mockInstallRegistersPowerShell = $true
    $script:mockWingetExitCode = 1603
    $script:mockWingetOutput = 'simulated installer failure detail'
    $exitError = $null
    try {
        $null = Install-PowerShell7
    }
    catch {
        $exitError = $_.Exception.Message
    }
    if ($exitError -notmatch 'exit code 1603' -or $exitError -notmatch 'simulated installer failure detail') {
        throw 'PowerShell 7 install did not preserve winget failure details.'
    }

    $script:mockPowerShellInstalled = $false
    $script:mockWingetAvailable = $false
    $script:mockWingetExitCode = 0
    $script:mockAdmin = $true
    $script:mockSignatureStatus = 'Valid'
    $env:PROCESSOR_ARCHITECTURE = 'AMD64'
    $env:PROCESSOR_ARCHITEW6432 = 'ARM64'
    $fallbackInstall = Install-PowerShell7
    if (-not $fallbackInstall.success -or $script:mockReleaseRequests -ne 1) {
        throw 'PowerShell 7 install did not use the official release fallback when WinGet was unavailable.'
    }
    if ($script:mockDownloadUri -cne 'https://github.com/PowerShell/PowerShell/releases/download/v7.6.6/PowerShell-7.6.6-win-arm64.msi') {
        throw 'PowerShell 7 fallback did not select the architecture-specific official MSI.'
    }
    if ($script:mockMsiInstallCalls -ne 1 -or $script:mockMsiArguments -notmatch '/qn' -or $script:mockMsiArguments -notmatch 'ADD_PATH=1') {
        throw 'PowerShell 7 fallback did not run the verified MSI silently with PATH registration enabled.'
    }

    $script:mockPowerShellInstalled = $false
    $script:mockMsiInstallCalls = 0
    $script:mockSignatureStatus = 'NotSigned'
    $signatureError = $null
    try { $null = Install-PowerShell7 } catch { $signatureError = $_.Exception.Message }
    if ($signatureError -notmatch 'valid Microsoft Corporation signature' -or $script:mockMsiInstallCalls -ne 0) {
        throw 'PowerShell 7 fallback did not reject an untrusted MSI before running it.'
    }

    $script:mockSignatureStatus = 'Valid'
    $script:mockReleaseMode = 'wrongUrl'
    $sourceError = $null
    try { $null = Install-PowerShell7 } catch { $sourceError = $_.Exception.Message }
    if ($sourceError -notmatch 'unexpected installer URL' -or $script:mockMsiInstallCalls -ne 0) {
        throw 'PowerShell 7 fallback did not reject an installer URL outside the official release path.'
    }

    $script:mockReleaseMode = 'valid'
    $script:mockReleaseRequests = 0
    $env:PROCESSOR_ARCHITECTURE = 'IA64'
    $env:PROCESSOR_ARCHITEW6432 = $null
    $architectureError = $null
    try { $null = Install-PowerShell7 } catch { $architectureError = $_.Exception.Message }
    if ($architectureError -notmatch 'only for Windows x64 and ARM64' -or $script:mockReleaseRequests -ne 0) {
        throw 'PowerShell 7 fallback did not stop safely for an unsupported architecture.'
    }

    $script:mockAdmin = $false
    $script:mockPowerShellInstalled = $true
    $wingetCallsBeforeInstalledProbe = $script:mockWingetArgs.Count
    $alreadyInstalled = Install-PowerShell7
    if (-not $alreadyInstalled.success -or $alreadyInstalled.message -notmatch 'already installed' -or $script:mockWingetArgs.Count -ne $wingetCallsBeforeInstalledProbe) {
        throw 'An existing PowerShell 7 installation should return success without administrator approval or another install.'
    }
    $dispatcherInstalled = Install-Dependency -Id 'powershell7'
    if (-not $dispatcherInstalled.success -or $dispatcherInstalled.message -notmatch 'already installed') {
        throw 'Install-Dependency should return an existing PowerShell 7 runtime before the generic admin check.'
    }

    $script:mockPowerShellInstalled = $false
    $permissionError = $null
    try { $null = Install-Dependency -Id 'powershell7' } catch { $permissionError = $_.Exception.Message }
    if ($permissionError -notmatch 'requires Administrator approval') {
        throw 'PowerShell 7 install did not explain its machine-wide elevation requirement.'
    }

    Write-Output 'PowerShell 7 install contract passed (WinGet Wix machine MSI, signed official MSI fallback, architecture/source/elevation checks, and runtime verification).'
}
finally {
    $env:ProgramData = $originalProgramData
    [Environment]::SetEnvironmentVariable('ProgramFiles', $originalProgramFiles, 'Process')
    [Environment]::SetEnvironmentVariable('ProgramW6432', $originalProgramW6432, 'Process')
    [Environment]::SetEnvironmentVariable('ProgramFiles(x86)', $originalProgramFilesX86, 'Process')
    $env:PROCESSOR_ARCHITECTURE = $originalProcessorArchitecture
    $env:PROCESSOR_ARCHITEW6432 = $originalProcessorArchW6432

    $resolvedTempRoot = [System.IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
    $resolvedTestRoot = [System.IO.Path]::GetFullPath($testDataRoot)
    if ($resolvedTestRoot.StartsWith($resolvedTempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}
