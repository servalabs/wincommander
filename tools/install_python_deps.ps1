# install_python_deps.ps1
# Automates the installation of Python dependencies for WinCommander AI features.

# Function to locate Python executable
function Resolve-PythonPath {
    # Helper to validate Python is real (not Windows Store stub)
    function Test-RealPython {
        param([string]$exePath)
        if (-not $exePath -or -not (Test-Path $exePath)) { return $false }
        # Windows Store stub is in WindowsApps folder
        if ($exePath -like "*WindowsApps*") { return $false }
        try {
            $versionOutput = & $exePath --version 2>&1 | Out-String
            return ($versionOutput -match 'Python \d')
        } catch {
            return $false
        }
    }

    # 1. Check if 'python' is in PATH (but validate it's real)
    $cmd = Get-Command "python" -ErrorAction SilentlyContinue
    if ($cmd -and (Test-RealPython $cmd.Source)) { return $cmd.Source }

    $cmd = Get-Command "python3" -ErrorAction SilentlyContinue
    if ($cmd -and (Test-RealPython $cmd.Source)) { return $cmd.Source }

    # 2. Check Registry (HKLM/HKCU)
    $pyVersions = @("3.15", "3.14", "3.13", "3.12", "3.11", "3.10")
    $regRoots = @("HKLM:\SOFTWARE\Python\PythonCore", "HKCU:\SOFTWARE\Python\PythonCore")

    foreach ($ver in $pyVersions) {
        foreach ($root in $regRoots) {
            $path = "$root\$ver\InstallPath"
            if (Test-Path $path) {
                $installPath = (Get-ItemProperty -Path $path -Name "(default)" -ErrorAction SilentlyContinue)." (default)"
                if ($installPath) {
                    $exe = Join-Path $installPath "python.exe"
                    if (Test-Path $exe) { return $exe }
                }
            }
        }
    }

    # 3. Check specific common locations
    $manualPaths = @()
    foreach ($ver in $pyVersions) {
        $verStr = $ver.Replace(".", "")
        $manualPaths += "$env:LOCALAPPDATA\Programs\Python\Python$verStr\python.exe"
        $manualPaths += "C:\Python$verStr\python.exe"
    }

    foreach ($p in $manualPaths) {
        if (Test-Path $p -and (Test-RealPython $p)) { return $p }
    }

    return $null
}

# Main Execution
Write-Host "🔍 checking for Python installation..." -ForegroundColor Cyan

$pythonExe = Resolve-PythonPath

if (-not $pythonExe) {
    Write-Host "⚠️ Python not found. Attempting to install Python 3.12 via Winget..." -ForegroundColor Yellow
    
    if (Get-Command "winget" -ErrorAction SilentlyContinue) {
        Write-Host "📥 Installing Python 3.12 (this may take a few minutes)..." -ForegroundColor Cyan
        winget install --id Python.Python.3.12 --exact --scope machine --accept-source-agreements --accept-package-agreements
        
        if ($LASTEXITCODE -ne 0) {
            Write-Error "❌ Failed to install Python via Winget."
            exit 1
        }
        
        Write-Host "⏳ Waiting for Python installation to complete..." -ForegroundColor Yellow
        
        # Wait for Python to become available (polling with timeout)
        $maxWaitTime = 300 # 5 minutes
        $waitInterval = 5 # Check every 5 seconds
        $elapsed = 0
        $pythonExe = $null
        
        while ($elapsed -lt $maxWaitTime -and -not $pythonExe) {
            Start-Sleep -Seconds $waitInterval
            $elapsed += $waitInterval
            
            # Refresh PATH
            $env:Path = [System.Environment]::GetEnvironmentVariable("Path", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path", "User")
            
            # Try to find Python
            $pythonExe = Resolve-PythonPath
            
            if (-not $pythonExe) {
                Write-Host "   Still waiting... ($elapsed seconds)" -ForegroundColor Gray
            }
        }
        
        if (-not $pythonExe) {
            Write-Error "❌ Python installation completed but Python could not be found after $maxWaitTime seconds. Please restart your terminal and try again."
            exit 1
        }
        
        Write-Host "✅ Python installation completed!" -ForegroundColor Green
    } else {
        Write-Error "❌ Python not found and Winget is not available. Please install Python manually."
        exit 1
    }
}

if (-not $pythonExe) {
    Write-Error "❌ Python installation could not be verified."
    exit 1
}

# Final validation that Python is real and ready
try {
    $versionOutput = & $pythonExe --version 2>&1 | Out-String
    if ($versionOutput -notmatch 'Python \d') {
        Write-Error "❌ Found Python at $pythonExe but it appears to be invalid (Windows Store stub?)."
        exit 1
    }
    Write-Host "✅ Python found at: $pythonExe (Version: $($versionOutput.Trim()))" -ForegroundColor Green
} catch {
    Write-Error "❌ Could not verify Python installation at $pythonExe"
    exit 1
}

# Small delay to ensure Python is fully ready
Start-Sleep -Seconds 2

Write-Host "`n📦 Starting pip package installation..." -ForegroundColor Cyan
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Gray

# Ensure pip is up to date
Write-Host "📦 Ensuring pip is up to date..." -ForegroundColor Cyan
& $pythonExe -m ensurepip --default-pip *>$null
if ($LASTEXITCODE -ne 0) {
    Write-Warning "⚠️ ensurepip had issues, continuing anyway..."
}
& $pythonExe -m pip install --upgrade pip --quiet *>$null
if ($LASTEXITCODE -eq 0) {
    Write-Host "✅ pip is ready" -ForegroundColor Green
} else {
    Write-Warning "⚠️ pip upgrade had issues, continuing anyway..."
}

# Define dependencies
$packages = @("mediapipe", "valkey", "opencv-python", "PyQt6", "numpy", "Pillow")
$importMap = @{
    "opencv-python" = "cv2"
    "Pillow"        = "PIL"
}

Write-Host "`n📦 Checking and installing Python packages..." -ForegroundColor Cyan
$packageCount = 0
$totalPackages = $packages.Count

foreach ($pkg in $packages) {
    $packageCount++
    $importName = if ($importMap.ContainsKey($pkg)) { $importMap[$pkg] } else { $pkg.Split('-')[0] }
    
    # Check if installed
    & $pythonExe -c "import $importName" *>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[$packageCount/$totalPackages] ⬇️ Installing $pkg (this may take a while)..." -ForegroundColor Yellow
        & $pythonExe -m pip install $pkg --quiet --no-warn-script-location
        
        if ($LASTEXITCODE -ne 0) {
            Write-Error "❌ Failed to install $pkg (exit code: $LASTEXITCODE)"
            exit 1
        }
        
        # Verify installation
        & $pythonExe -c "import $importName" *>$null
        if ($LASTEXITCODE -ne 0) {
             Write-Error "❌ $pkg was installed but cannot be imported. Installation may be incomplete."
             exit 1
        }
        Write-Host "    ✅ Successfully installed $pkg" -ForegroundColor Green
    } else {
        Write-Host "[$packageCount/$totalPackages] ✅ $pkg is already installed" -ForegroundColor Gray
    }
}

Write-Host "🎉 All Python dependencies are installed and ready!" -ForegroundColor Green
exit 0
