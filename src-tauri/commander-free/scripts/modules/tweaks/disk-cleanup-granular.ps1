# ============================================================================
# TWEAKS - DISK CLEANUP (GRANULAR)
# Per-category disk cleanup.
# Commander already ships BleachBit + Invoke-DiskCleanup for broad cleanup;
# this gives a per-card view (Temp / Windows Update / Prefetch / Thumbs /
# Crash dumps / Recycle Bin / Windows.old).
# ============================================================================

function _Get-FolderSize {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    try {
        $items = Get-ChildItem -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue
        $sum = ($items | Where-Object { -not $_.PSIsContainer } | Measure-Object -Property Length -Sum).Sum
        if ($null -eq $sum) { return 0 }
        return [int64]$sum
    } catch { return 0 }
}

function _Get-FileCount {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 0 }
    try {
        $cnt = (Get-ChildItem -LiteralPath $Path -Recurse -Force -ErrorAction SilentlyContinue |
            Where-Object { -not $_.PSIsContainer }).Count
        if ($null -eq $cnt) { return 0 }
        return [int]$cnt
    } catch { return 0 }
}

# ── SCAN ─────────────────────────────────────────────────────────────────
# Returns size/count for every category; UI shows checkboxes per row.

function Get-DiskCleanupScan {
    Assert-IsAdmin
    $categories = @(
        @{ id = "tempUser";       label = "User Temp Files";         path = $env:TEMP                                          },
        @{ id = "tempSystem";     label = "System Temp Files";       path = "$env:SystemRoot\Temp"                            },
        @{ id = "windowsUpdate";  label = "Windows Update Cache";    path = "$env:SystemRoot\SoftwareDistribution\Download"   },
        @{ id = "prefetch";       label = "Prefetch Files";          path = "$env:SystemRoot\Prefetch"                        },
        @{ id = "thumbnailCache"; label = "Thumbnail Cache";         path = "$env:LOCALAPPDATA\Microsoft\Windows\Explorer"    },
        @{ id = "crashDumps";     label = "Crash Dumps";             path = "$env:LOCALAPPDATA\CrashDumps"                    },
        @{ id = "winErr";         label = "Error Reports";           path = "$env:LOCALAPPDATA\Microsoft\Windows\WER"         },
        @{ id = "deliveryOpt";    label = "Delivery Optimization";   path = "$env:SystemRoot\SoftwareDistribution\DeliveryOptimization" },
        @{ id = "windowsOld";     label = "Windows.old (previous installation)"; path = "$env:SystemDrive\Windows.old"        }
    )
    $results = @()
    foreach ($c in $categories) {
        $size = _Get-FolderSize -Path $c.path
        $count = _Get-FileCount -Path $c.path
        $results += [PSCustomObject]@{
            Id          = $c.id
            Label       = $c.label
            Path        = $c.path
            Exists      = (Test-Path -LiteralPath $c.path)
            FileCount   = $count
            SizeBytes   = $size
            SizeMb      = [Math]::Round($size / 1MB, 2)
        }
    }

    # Recycle Bin: estimate via Shell.Application (folder size walk fails on the special folder)
    $rbBytes = 0
    $rbCount = 0
    try {
        $shell = New-Object -ComObject Shell.Application
        $recycleBin = $shell.Namespace(0xA)
        if ($recycleBin) {
            foreach ($item in $recycleBin.Items()) {
                $rbBytes += $item.Size
                $rbCount++
            }
        }
    } catch {}
    $results += [PSCustomObject]@{
        Id        = "recycleBin"
        Label     = "Recycle Bin"
        Path      = "shell:RecycleBinFolder"
        Exists    = $rbCount -gt 0
        FileCount = $rbCount
        SizeBytes = $rbBytes
        SizeMb    = [Math]::Round($rbBytes / 1MB, 2)
    }

    @{ categories = $results }
}

# ── CLEAN ────────────────────────────────────────────────────────────────
# Takes a comma-separated id list (frontend stringifies arrays for the
# Tauri serialiser). Returns per-category bytes-freed counts.

function Invoke-DiskCleanupCategories {
    param([string]$Ids)
    Assert-IsAdmin
    if ([string]::IsNullOrWhiteSpace($Ids)) {
        return @{ error = $true; message = "No categories selected" }
    }
    $idList = $Ids -split ','

    $freedTotal = [int64]0
    $perCategory = @{}

    foreach ($id in $idList) {
        $id = $id.Trim()
        if ([string]::IsNullOrWhiteSpace($id)) { continue }

        $path = switch ($id) {
            "tempUser"       { $env:TEMP }
            "tempSystem"     { "$env:SystemRoot\Temp" }
            "windowsUpdate"  { "$env:SystemRoot\SoftwareDistribution\Download" }
            "prefetch"       { "$env:SystemRoot\Prefetch" }
            "thumbnailCache" { "$env:LOCALAPPDATA\Microsoft\Windows\Explorer" }
            "crashDumps"     { "$env:LOCALAPPDATA\CrashDumps" }
            "winErr"         { "$env:LOCALAPPDATA\Microsoft\Windows\WER" }
            "deliveryOpt"    { "$env:SystemRoot\SoftwareDistribution\DeliveryOptimization" }
            "windowsOld"     { "$env:SystemDrive\Windows.old" }
            "recycleBin"     { "__recycleBin__" }
            default          { $null }
        }
        if (-not $path) { continue }

        [int64]$before = 0
        [int64]$freed = 0

        if ($id -eq "recycleBin") {
            try {
                $shell = New-Object -ComObject Shell.Application
                $recycleBin = $shell.Namespace(0xA)
                if ($recycleBin) {
                    foreach ($item in $recycleBin.Items()) { $before += $item.Size }
                }
                # Clear-RecycleBin handles all drives + bypasses confirm
                Clear-RecycleBin -Force -ErrorAction SilentlyContinue
                $freed = $before
            } catch {}
        }
        elseif ($id -eq "thumbnailCache") {
            # Only purge thumbcache_*.db files — don't nuke the whole Explorer folder
            try {
                Get-ChildItem -LiteralPath $path -Filter "thumbcache_*.db" -ErrorAction SilentlyContinue |
                    ForEach-Object {
                        $before += $_.Length
                        Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue
                        if (-not (Test-Path -LiteralPath $_.FullName)) { $freed += $_.Length }
                    }
            } catch {}
        }
        elseif (Test-Path -LiteralPath $path) {
            $before = _Get-FolderSize -Path $path
            try {
                # Walk and delete; we don't remove the top-level dir itself
                Get-ChildItem -LiteralPath $path -Recurse -Force -ErrorAction SilentlyContinue |
                    Sort-Object -Property FullName -Descending |
                    ForEach-Object {
                        try { Remove-Item -LiteralPath $_.FullName -Force -Recurse -ErrorAction SilentlyContinue } catch {}
                    }
            } catch {}
            [int64]$after = _Get-FolderSize -Path $path
            $freed = [Math]::Max([int64]0, $before - $after)
        }

        $freedTotal += $freed
        $perCategory[$id] = [PSCustomObject]@{
            FreedBytes = $freed
            FreedMb    = [Math]::Round($freed / 1MB, 2)
        }
    }

    @{
        success     = $true
        freedTotal  = $freedTotal
        freedTotalMb = [Math]::Round($freedTotal / 1MB, 2)
        perCategory = $perCategory
    }
}
