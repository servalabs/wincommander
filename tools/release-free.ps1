<#
  Manual WinCommander Free publisher.

  This is the local equivalent of .github/workflows/release.yml. Publication
  requires a source commit that is clean, exactly version-aligned, at
  origin/main, and already tagged vX.Y.Z. StageOnly permits a clean,
  version-aligned untagged commit for local installer testing. It deliberately
  does not edit versions, move tags, or claim a GitHub Actions provenance
  attestation.

  Examples:
    pwsh -File .\tools\release-free.ps1 -Version 3.4.6 -StageOnly
    pwsh -File .\tools\release-free.ps1 -Version 3.4.6
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$')]
    [string]$Version,

    # Build and validate local artifacts, but do not contact R2 or GitHub.
    [switch]$StageOnly,

    # Re-use an already-built, signed NSIS setup for the requested version.
    [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Root = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$script:Tag = "v$Version"
$script:WorkRoot = Join-Path ([IO.Path]::GetTempPath()) ("wincommander-free-release-$Version-" + [guid]::NewGuid().ToString('N'))
$script:RcloneExe = $null

function Stop-Release {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw "[FAIL] $Message"
}

function Import-ReleaseEnvironment {
    $envFile = Join-Path $script:Root '.env'
    if (-not (Test-Path -LiteralPath $envFile -PathType Leaf)) {
        Stop-Release "Missing private release environment file at $envFile."
    }

    foreach ($line in Get-Content -LiteralPath $envFile) {
        if ($line -notmatch '^\s*([^#\s][^=]*?)\s*=\s*(.*)$') { continue }
        $name = $Matches[1].Trim()
        $value = $Matches[2].Trim()
        if ($value.Length -ge 2 -and (($value.StartsWith('"') -and $value.EndsWith('"')) -or ($value.StartsWith("'") -and $value.EndsWith("'")))) {
            $value = $value.Substring(1, $value.Length - 2)
        }
        Set-Item -Path "env:$name" -Value $value
    }
}

function Require-EnvironmentValue {
    param([Parameter(Mandatory = $true)][string]$Name)
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        Stop-Release "The private release environment does not provide $Name."
    }
    return $value.Trim()
}

function Normalize-UpdaterSigningKey {
    $key = Require-EnvironmentValue -Name 'TAURI_SIGNING_PRIVATE_KEY'
    try {
        $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($key))
    }
    catch [FormatException] {
        if ($key -notmatch '^untrusted comment:') {
            Stop-Release 'TAURI_SIGNING_PRIVATE_KEY must be an outer Base64 signing key or raw signing-key file.'
        }
        $key = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($key))
        $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($key))
    }
    if ($decoded -notmatch '^untrusted comment:') {
        Stop-Release 'TAURI_SIGNING_PRIVATE_KEY is not an updater signing key. Do not use the public key from tauri.conf.json.'
    }
    $env:TAURI_SIGNING_PRIVATE_KEY = $key
    $null = Require-EnvironmentValue -Name 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD'
}

function Get-FileVersionFromJson {
    param([Parameter(Mandatory = $true)][string]$Path)
    return ((Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json).version).ToString()
}

function Get-CargoPackageVersion {
    param([Parameter(Mandatory = $true)][string]$Path, [Parameter(Mandatory = $true)][string]$Package)
    $content = Get-Content -LiteralPath $Path -Raw
    $pattern = '(?ms)\[\[package\]\]\s*name\s*=\s*"' + [regex]::Escape($Package) + '"\s*version\s*=\s*"(?<version>[^"]+)"'
    $matches = [regex]::Matches($content, $pattern)
    if ($matches.Count -ne 1) { Stop-Release "Expected exactly one $Package package record in $Path; found $($matches.Count)." }
    return $matches[0].Groups['version'].Value
}

function Assert-ReleaseSource {
    param([switch]$AllowUntaggedStage)

    if (@(git -C $script:Root status --porcelain).Count -ne 0) {
        Stop-Release 'Refusing to release from a dirty worktree.'
    }

    if (-not $AllowUntaggedStage) {
        $head = (git -C $script:Root rev-parse HEAD).Trim()
        $tagHead = (git -C $script:Root rev-parse "$script:Tag^{commit}" 2>$null).Trim()
        if ($LASTEXITCODE -ne 0 -or $tagHead -ne $head) {
            Stop-Release "$script:Tag must exist locally and point exactly at HEAD."
        }

        git -C $script:Root fetch --quiet origin "refs/heads/main:refs/remotes/origin/main" "refs/tags/${script:Tag}:refs/tags/${script:Tag}"
        if ($LASTEXITCODE -ne 0) { Stop-Release 'Could not refresh origin/main and the release tag.' }
        $remoteTag = (git -C $script:Root rev-parse "$script:Tag^{commit}").Trim()
        $originMain = (git -C $script:Root rev-parse origin/main).Trim()
        if ($remoteTag -ne $head) { Stop-Release "$script:Tag does not match its remote tag target." }
        if ($originMain -ne $head) { Stop-Release 'The release tag must point exactly at origin/main.' }
    }

    $versions = @{
        'package.json' = Get-FileVersionFromJson (Join-Path $script:Root 'package.json')
        'Free Tauri config' = Get-FileVersionFromJson (Join-Path $script:Root 'src-tauri\commander-free\tauri.conf.json')
        'Free Cargo manifest' = ([regex]::Match((Get-Content -LiteralPath (Join-Path $script:Root 'src-tauri\commander-free\Cargo.toml') -Raw), '(?m)^version\s*=\s*"(?<version>[^"]+)"')).Groups['version'].Value
        'Free Cargo lock record' = Get-CargoPackageVersion (Join-Path $script:Root 'src-tauri\Cargo.lock') 'commander-free'
    }
    foreach ($entry in $versions.GetEnumerator()) {
        if ($entry.Value -ne $Version) { Stop-Release "$($entry.Key) version '$($entry.Value)' does not equal requested version '$Version'." }
    }
}

function Find-Rclone {
    $command = Get-Command rclone.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    $command = Get-Command rclone -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    foreach ($candidate in @(
        (Join-Path $env:LOCALAPPDATA 'Microsoft\WinGet\Links\rclone.exe'),
        (Join-Path $env:ProgramFiles 'rclone\rclone.exe')
    )) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    return $null
}

function Initialize-R2 {
    $script:RcloneExe = Find-Rclone
    if (-not $script:RcloneExe) {
        Stop-Release 'rclone is required to publish. Install Rclone.Rclone first, then re-run this command.'
    }
    $env:RCLONE_CONFIG_R2_TYPE = 's3'
    $env:RCLONE_CONFIG_R2_PROVIDER = 'Cloudflare'
    $env:RCLONE_CONFIG_R2_ENDPOINT = Require-EnvironmentValue -Name 'CF_R2_API'
    $env:RCLONE_CONFIG_R2_ACCESS_KEY_ID = Require-EnvironmentValue -Name 'R2_KEY'
    $env:RCLONE_CONFIG_R2_SECRET_ACCESS_KEY = Require-EnvironmentValue -Name 'R2_SECRET'
    $env:RCLONE_CONFIG_R2_REGION = 'auto'
}

function Invoke-Rclone {
    param([Parameter(Mandatory = $true)][string[]]$Arguments, [Parameter(Mandatory = $true)][string]$Failure)
    & $script:RcloneExe @Arguments
    if ($LASTEXITCODE -ne 0) { Stop-Release $Failure }
}

function Get-Syft {
    $pinnedVersion = '1.50.0'
    $pinnedArchiveHash = '815ee6973ec5dff6a671d7f41b0e78835a8c45b91d5a39f4743ea1cee833d3be'
    $command = Get-Command syft.exe -ErrorAction SilentlyContinue
    if (-not $command) { $command = Get-Command syft -ErrorAction SilentlyContinue }
    if ($command) {
        $reported = (& $command.Source version 2>&1 | Out-String)
        if ($reported -match [regex]::Escape($pinnedVersion)) { return $command.Source }
    }

    $toolDir = Join-Path $script:WorkRoot "syft-$pinnedVersion"
    New-Item -ItemType Directory -Path $toolDir -Force | Out-Null
    $archive = Join-Path $toolDir "syft_${pinnedVersion}_windows_amd64.zip"
    $url = "https://github.com/anchore/syft/releases/download/v$pinnedVersion/syft_${pinnedVersion}_windows_amd64.zip"
    Invoke-WebRequest -Uri $url -OutFile $archive
    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $pinnedArchiveHash) { Stop-Release 'Downloaded Syft archive does not match the pinned SHA-256.' }
    Expand-Archive -LiteralPath $archive -DestinationPath $toolDir -Force
    $syft = Get-ChildItem -LiteralPath $toolDir -Filter syft.exe -File -Recurse | Select-Object -First 1
    if ($null -eq $syft) { Stop-Release 'Pinned Syft archive did not contain syft.exe.' }
    return $syft.FullName
}

function New-Sbom {
    param([Parameter(Mandatory = $true)][string]$Output)
    $syft = Get-Syft
    & $syft scan "dir:$script:Root" '-o' "cyclonedx-json=$Output"
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $Output -PathType Leaf)) {
        Stop-Release 'Syft could not create the CycloneDX SBOM.'
    }
    $document = Get-Content -LiteralPath $Output -Raw | ConvertFrom-Json
    if ($document.bomFormat -ne 'CycloneDX') { Stop-Release 'The generated SBOM is not CycloneDX JSON.' }
}

function Get-ReleaseAssets {
    $setups = @(Get-ChildItem -LiteralPath (Join-Path $script:Root 'src-tauri\target\release\bundle\nsis') -Filter "WinCommander*${Version}*_x64-setup.exe" -File | Where-Object { $_.Name -notlike '*.sig*' })
    if ($setups.Count -ne 1) { Stop-Release "Expected exactly one NSIS setup for v$Version; found $($setups.Count)." }
    $setup = $setups[0]
    $signaturePath = "$($setup.FullName).sig"
    if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) { Stop-Release "Tauri updater signature not found: $signaturePath" }
    $signature = (Get-Content -LiteralPath $signaturePath -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($signature)) { Stop-Release 'Tauri updater signature is empty.' }
    try { $signatureText = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($signature)) }
    catch [FormatException] { Stop-Release 'Tauri updater signature is not valid Base64.' }
    if ($signatureText -notmatch [Regex]::Escape($setup.Name)) { Stop-Release "The updater signature does not name $($setup.Name)." }

    $assetDirectory = Join-Path $script:WorkRoot 'assets'
    New-Item -ItemType Directory -Path $assetDirectory -Force | Out-Null
    $domain = (Require-EnvironmentValue -Name 'CF_UPDATE_DOMAIN').TrimEnd('/')
    if ($domain -notmatch '^https?://') { $domain = "https://$domain" }
    $manifestPath = Join-Path $assetDirectory 'latest.json'
    $checksumPath = Join-Path $assetDirectory 'SHA256SUMS.txt'
    $sbomPath = Join-Path $assetDirectory 'wincommander-free.sbom.cdx.json'
    $manifest = @{
        version = $Version
        notes = "WinCommander Free v$Version"
        pub_date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = @{ 'windows-x86_64' = @{ signature = $signature; url = "$domain/free/v$Version/$([Uri]::EscapeDataString($setup.Name))" } }
    } | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText($manifestPath, $manifest, [Text.UTF8Encoding]::new($false))
    $sha256 = (Get-FileHash -LiteralPath $setup.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($checksumPath, "$sha256 *$($setup.Name)`n", [Text.UTF8Encoding]::new($false))
    New-Sbom -Output $sbomPath
    return [pscustomobject]@{ Setup = $setup.FullName; Signature = $signaturePath; Manifest = $manifestPath; Checksum = $checksumPath; Sbom = $sbomPath }
}

function Assert-RemoteHash {
    param([Parameter(Mandatory = $true)][string]$RemotePath, [Parameter(Mandatory = $true)][string]$LocalPath, [Parameter(Mandatory = $true)][string]$Label)
    $downloadDirectory = Join-Path $script:WorkRoot 'remote-verification'
    New-Item -ItemType Directory -Path $downloadDirectory -Force | Out-Null
    $download = Join-Path $downloadDirectory ([guid]::NewGuid().ToString('N') + '-' + (Split-Path -Leaf $RemotePath))
    Invoke-Rclone -Arguments @('copyto', "r2:windows/$RemotePath", $download, '--s3-no-check-bucket') -Failure "Could not download $Label for verification."
    $expected = (Get-FileHash -LiteralPath $LocalPath -Algorithm SHA256).Hash
    $actual = (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash
    if ($actual -ne $expected) { Stop-Release "$Label does not match the locally staged file." }
}

function Publish-ReleaseAssets {
    param([Parameter(Mandatory = $true)][object]$Assets)
    $setupName = Split-Path -Leaf $Assets.Setup
    $immutable = @(
        @{ Local = $Assets.Setup; Remote = "free/v$Version/$setupName"; Label = 'versioned setup' },
        @{ Local = $Assets.Signature; Remote = "free/v$Version/$setupName.sig"; Label = 'versioned updater signature' },
        @{ Local = $Assets.Checksum; Remote = "free/v$Version/SHA256SUMS.txt"; Label = 'versioned checksum' },
        @{ Local = $Assets.Sbom; Remote = "free/v$Version/wincommander-free.sbom.cdx.json"; Label = 'versioned SBOM' }
    )
    foreach ($asset in $immutable) {
        Invoke-Rclone -Arguments @('copyto', $asset.Local, "r2:windows/$($asset.Remote)", '--s3-no-check-bucket') -Failure "Could not upload $($asset.Label) to R2."
    }
    foreach ($asset in $immutable) { Assert-RemoteHash -RemotePath $asset.Remote -LocalPath $asset.Local -Label $asset.Label }

    # The mutable pointers are deliberately promoted only after all immutable
    # artifacts have independently round-tripped. latest.json is last because
    # it is the updater's trust entrypoint.
    Invoke-Rclone -Arguments @('copyto', $Assets.Setup, 'r2:windows/free/latest.exe', '--ignore-times', '--s3-no-check-bucket') -Failure 'Could not promote the latest setup to R2.'
    Assert-RemoteHash -RemotePath 'free/latest.exe' -LocalPath $Assets.Setup -Label 'latest setup'
    Invoke-Rclone -Arguments @('copyto', $Assets.Manifest, 'r2:windows/free/latest.json', '--ignore-times', '--s3-no-check-bucket') -Failure 'Could not publish free/latest.json to R2.'
    Assert-RemoteHash -RemotePath 'free/latest.json' -LocalPath $Assets.Manifest -Label 'latest updater manifest'
}

function Publish-GitHubArchive {
    param([Parameter(Mandatory = $true)][object]$Assets)
    & gh auth status -h github.com 2>$null
    if ($LASTEXITCODE -ne 0) { Stop-Release 'GitHub CLI is not authenticated; cannot publish the GitHub Release archive.' }
    $releaseAssets = @($Assets.Setup, $Assets.Signature, $Assets.Manifest, $Assets.Checksum, $Assets.Sbom)
    # Local publication cannot make the GitHub Actions/OIDC provenance claim.
    $notes = 'Manual Free NSIS setup release. No GitHub Actions provenance attestation was generated for this local publication.'
    $existingJson = (& gh release view $script:Tag --json isDraft 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -eq 0) {
        $existing = $existingJson | ConvertFrom-Json
        & gh release upload $script:Tag @releaseAssets --clobber
        if ($LASTEXITCODE -ne 0) { Stop-Release 'Could not update GitHub Release assets.' }
        if ($existing.isDraft) {
            & gh release edit $script:Tag --draft=false --title $script:Tag --notes $notes
            if ($LASTEXITCODE -ne 0) { Stop-Release 'Could not publish the recovered GitHub Release draft.' }
        }
        return
    }
    $arguments = @('release', 'create', $script:Tag, '--title', $script:Tag, '--notes', $notes)
    if ($script:Tag.Contains('-')) { $arguments += '--prerelease' }
    $arguments += $releaseAssets
    & gh @arguments
    if ($LASTEXITCODE -ne 0) { Stop-Release 'Could not create the GitHub Release archive.' }
}

Push-Location $script:Root
try {
    Import-ReleaseEnvironment
    Assert-ReleaseSource -AllowUntaggedStage:$StageOnly
    Normalize-UpdaterSigningKey
    New-Item -ItemType Directory -Path $script:WorkRoot -Force | Out-Null

    if (-not $SkipBuild) {
        & bun run build:free:release-installer
        if ($LASTEXITCODE -ne 0) { Stop-Release 'Free NSIS release build failed.' }
    }
    $assets = Get-ReleaseAssets
    if ($StageOnly) {
        Write-Host "[ok] v$Version passed local manual-release staging. No R2 or GitHub publication was attempted." -ForegroundColor Green
        exit 0
    }

    Initialize-R2
    Publish-ReleaseAssets -Assets $assets
    Publish-GitHubArchive -Assets $assets
    Write-Host "[ok] WinCommander Free v$Version was published to R2 and GitHub Release archive." -ForegroundColor Green
}
finally {
    Pop-Location
}
