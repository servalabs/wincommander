# Ensures the developer tools required by the WinCommander dev server exist.
# Called from package.json after Bun starts, and by tools/dev.ps1 before Bun exists.

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$toolchainFile = Join-Path $repoRoot "rust-toolchain.toml"
$bunVersion = "1.4.0"
$rustupInitUrl = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe"

function Add-ProcessPathEntry {
    param([Parameter(Mandatory)][string]$PathEntry)

    if ((Test-Path -LiteralPath $PathEntry) -and -not (($env:Path -split ";") -contains $PathEntry)) {
        $env:Path = "$PathEntry;$env:Path"
    }
}

function Find-Application {
    param([Parameter(Mandatory)][string]$Name)

    $command = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($command) {
        return $command.Path
    }

    return $null
}

function Get-TomlString {
    param(
        [Parameter(Mandatory)][string]$Toml,
        [Parameter(Mandatory)][string]$Key
    )

    $pattern = '(?m)^\s*' + [regex]::Escape($Key) + '\s*=\s*"([^"]+)"'
    $match = [regex]::Match($Toml, $pattern)
    if (-not $match.Success) {
        throw "rust-toolchain.toml is missing '$Key'."
    }

    return $match.Groups[1].Value
}

function Get-TomlStringArray {
    param(
        [Parameter(Mandatory)][string]$Toml,
        [Parameter(Mandatory)][string]$Key
    )

    $pattern = '(?ms)^\s*' + [regex]::Escape($Key) + '\s*=\s*\[(.*?)\]'
    $match = [regex]::Match($Toml, $pattern)
    if (-not $match.Success) {
        throw "rust-toolchain.toml is missing '$Key'."
    }

    return @([regex]::Matches($match.Groups[1].Value, '"([^\"]+)"') |
        ForEach-Object { $_.Groups[1].Value })
}

function Ensure-Bun {
    $bunInstallRoot = if ($env:BUN_INSTALL) {
        $env:BUN_INSTALL
    } else {
        Join-Path $env:USERPROFILE ".bun"
    }
    $bunBin = Join-Path $bunInstallRoot "bin"
    Add-ProcessPathEntry $bunBin

    $bun = Find-Application "bun"
    $installedVersion = if ($bun) { (& $bun --version).Trim() } else { $null }
    if ($installedVersion -ne $bunVersion) {
        $state = if ($installedVersion) { "Bun $installedVersion is installed" } else { "Bun is not installed" }
        Write-Host "$state. Installing Bun $bunVersion..."
        $installer = Invoke-RestMethod -Uri "https://bun.com/install.ps1"
        & ([scriptblock]::Create($installer)) -Version $bunVersion
        Add-ProcessPathEntry $bunBin
        # Get-Command can retain an earlier WinGet shim after PATH changes.
        $bun = Join-Path $bunBin "bun.exe"
    }

    if (-not $bun) {
        throw "Bun installation completed but bun.exe was not found. Restart PowerShell and run tools/dev.ps1 again."
    }

    $installedVersion = (& $bun --version).Trim()
    if ($installedVersion -ne $bunVersion) {
        throw "Bun $bunVersion is required, but '$installedVersion' was found at $bun. Restart PowerShell and run tools/dev.ps1 again."
    }

    Write-Host "Using Bun: $installedVersion"
    return $bun
}

function Ensure-Rustup {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    Add-ProcessPathEntry $cargoBin

    $rustup = Find-Application "rustup"
    if (-not $rustup) {
        # rustup dispatches by argv[0]; any name other than rustup-init is treated as a proxy.
        $rustupInit = Join-Path ([IO.Path]::GetTempPath()) "rustup-init.exe"
        Write-Host "rustup is not installed. Installing the Rust toolchain manager..."
        try {
            Invoke-WebRequest -Uri $rustupInitUrl -OutFile $rustupInit
            & $rustupInit -y --profile minimal --default-toolchain none | Out-Host
            if ($LASTEXITCODE -ne 0) {
                throw "rustup-init.exe exited with code $LASTEXITCODE."
            }
        } finally {
            Remove-Item -LiteralPath $rustupInit -Force -ErrorAction SilentlyContinue
        }

        Add-ProcessPathEntry $cargoBin
        $rustup = Find-Application "rustup"
    }

    if (-not $rustup) {
        throw "rustup installation completed but rustup.exe was not found. Restart PowerShell and run tools/dev.ps1 again."
    }

    return $rustup
}

if (-not (Test-Path -LiteralPath $toolchainFile)) {
    throw "Missing pinned toolchain file: $toolchainFile"
}

$toolchainToml = Get-Content -LiteralPath $toolchainFile -Raw
$channel = Get-TomlString -Toml $toolchainToml -Key "channel"
$components = Get-TomlStringArray -Toml $toolchainToml -Key "components"
$targets = Get-TomlStringArray -Toml $toolchainToml -Key "targets"

Ensure-Bun | Out-Null
$rustup = Ensure-Rustup
$hostToolchain = "$channel-x86_64-pc-windows-msvc"
$installedToolchains = @(& $rustup toolchain list | ForEach-Object { ($_ -split "\s+")[0] })

$needsInstall = $installedToolchains -notcontains $hostToolchain
if (-not $needsInstall) {
    $installedComponents = @(& $rustup component list --toolchain $channel)
    $installedTargets = @(& $rustup target list --toolchain $channel)
    $needsInstall = @($components | Where-Object {
        $escaped = [regex]::Escape($_)
        -not ($installedComponents -match "^$escaped(?:-[^\s]+)?\s+\(installed\)$")
    }).Count -gt 0 -or @($targets | Where-Object {
        $escaped = [regex]::Escape($_)
        -not ($installedTargets -match "^$escaped\s+\(installed\)$")
    }).Count -gt 0
}

if ($needsInstall) {
    Write-Host "Installing Rust $channel with the repository's required components and targets..."
    $installArgs = @("toolchain", "install", $channel, "--profile", "minimal")
    foreach ($component in $components) {
        $installArgs += "--component", $component
    }
    foreach ($target in $targets) {
        $installArgs += "--target", $target
    }
    & $rustup @installArgs
    if ($LASTEXITCODE -ne 0) {
        throw "rustup failed to install Rust $channel (exit code $LASTEXITCODE)."
    }
} else {
    Write-Host "Rust $channel and its required components are already installed."
}

Write-Host "Using Rust: $(& $rustup run $channel rustc --version)"

$assetsDir = Join-Path $repoRoot "assets"
$assetsGitDir = Join-Path $assetsDir ".git"
if ((Test-Path -LiteralPath $assetsDir) -and -not (Test-Path -LiteralPath $assetsGitDir)) {
    throw "The assets directory exists but is not the repository submodule. Move it aside, then run tools/dev.ps1 again."
}

Write-Host "Checking out the pinned assets submodule..."
& git -C $repoRoot submodule update --init --recursive -- assets
if ($LASTEXITCODE -ne 0) {
    throw "Failed to initialize the pinned assets submodule."
}

$requiredAssets = @(
    (Join-Path $assetsDir "softwares\calc.png"),
    (Join-Path $assetsDir "products\wincommander\logo.png")
)
foreach ($requiredAsset in $requiredAssets) {
    if (-not (Test-Path -LiteralPath $requiredAsset)) {
        throw "The assets submodule is missing required file: $requiredAsset"
    }
}

# Explorer's secure-delete verb uses a small asInvoker helper instead of the
# main Tauri executable. Build it during every developer bootstrap so a
# running `tauri dev` instance can enable and test the context menu without
# falling back to an installed/release EXE (which could prompt for UAC).
$contextShredManifest = Join-Path $repoRoot "src-tauri\Cargo.toml"
Write-Host "Building the development context-delete helper..."
& $rustup run $channel cargo build --manifest-path $contextShredManifest -p commander-context-shred
if ($LASTEXITCODE -ne 0) {
    throw "Failed to build the WinCommander development context-delete helper."
}
