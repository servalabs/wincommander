# Fetches brand SVG/PNG icons for app cards into assets/softwares/
# (the public repository's vendored runtime-asset directory).
# Tries multiple CDN sources per slug; first hit wins. Run with:
#   pwsh tools/fetch-app-icons.ps1
# Re-running is safe — existing files are skipped unless -Force is passed.

[CmdletBinding()]
param(
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$outDir = Join-Path $repoRoot 'assets/softwares'
if (-not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir | Out-Null }

# slug -> ordered list of candidate URLs.
# Slug names match ID_TO_SLUG in src/panels/apps/components/appIcons.ts.
$catalog = @(
    @{ slug = 'peazip'; urls = @(
            'https://peazip.github.io/favicon.ico'
        ) },
    @{ slug = 'everything'; urls = @(
            'https://www.voidtools.com/voidtools9.png',
            'https://www.voidtools.com/favicon.ico'
        ) },
    @{ slug = 'imageglass'; urls = @(
            'https://raw.githubusercontent.com/d2phap/ImageGlass/develop/Assets/Logo/New/iglogo.svg'
        ) },
    @{ slug = 'screenbox'; urls = @(
            'https://screenbox.app/favicon.ico'
        ) },
    @{ slug = 'driver-booster'; urls = @(
            'https://www.iobit.com/tpl/images/product-icons/db_96.png'
        ) },
    @{ slug = 'nilesoft-shell'; urls = @(
            'https://nilesoft.org/favicon.ico'
        ) },
    @{ slug = 'github-desktop'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/github-desktop.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/github.svg'
        ) },
    @{ slug = 'pgadmin'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/pgadmin.svg'
        ) },
    @{ slug = 'cursor'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/cursor.svg',
            'https://www.cursor.com/apple-touch-icon.png'
        ) },
    @{ slug = 'antigravity'; urls = @(
            'https://antigravity.google/favicon.ico',
            'https://www.google.com/s2/favicons?domain=antigravity.google&sz=128'
        ) },
    @{ slug = 'windows-terminal'; urls = @(
            'https://cdn.jsdelivr.net/gh/selfhst/icons/svg/windows-terminal.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/terminal.svg'
        ) },
    @{ slug = 'nodejs'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/nodejs.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/nodedotjs.svg'
        ) },
    @{ slug = 'bun'; urls = @(
            'https://bun.sh/logo.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/bun.svg'
        ) },
    @{ slug = 'cloudflare'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/cloudflare.svg'
        ) },
    @{ slug = 'powershell'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/powershell.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/powershell.svg'
        ) },
    @{ slug = 'git'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/git.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/git.svg'
        ) },
    @{ slug = 'java'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/java.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/openjdk.svg'
        ) },
    @{ slug = 'python'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/python.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/python.svg'
        ) },
    @{ slug = 'mobaxterm'; urls = @(
            'https://mobaxterm.mobatek.net/favicon.ico'
        ) },
    @{ slug = 'ffmpeg'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/ffmpeg.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/ffmpeg.svg'
        ) },
    @{ slug = 'pdfgear'; urls = @(
            'https://www.pdfgear.com/img/icon.png'
        ) },
    @{ slug = 'wiztree'; urls = @(
            'https://diskanalyzer.com/favicon.ico'
        ) },
    @{ slug = 'teracopy'; urls = @(
            'https://www.codesector.com/favicon.ico'
        ) },
    @{ slug = 'vivaldi'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/vivaldi.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/vivaldi.svg'
        ) },
    @{ slug = 'fdm'; urls = @(
            'https://www.freedownloadmanager.org/favicon.ico'
        ) },
    @{ slug = 'bcu'; urls = @(
            'https://raw.githubusercontent.com/Klocman/Bulk-Crap-Uninstaller/master/source/BulkCrapUninstaller/Resources/logo.ico'
        ) },
    @{ slug = 'flux'; urls = @(
            'https://justgetflux.com/favicon.ico'
        ) },
    @{ slug = 'hwinfo'; urls = @(
            'https://www.hwinfo.com/images/hwi_logo_flat_square_192.png'
        ) },
    @{ slug = 'crystaldiskinfo'; urls = @(
            'https://crystalmark.info/favicon.ico'
        ) },
    @{ slug = 'crystaldiskmark'; urls = @(
            'https://crystalmark.info/favicon.ico'
        ) },
    @{ slug = 'smartmontools'; urls = @(
            'https://raw.githubusercontent.com/smartmontools/smartmontools/master/www/smart_logo.gif'
        ) },
    @{ slug = 'system-informer'; urls = @(
            'https://systeminformer.sourceforge.io/favicon.ico'
        ) },
    @{ slug = 'whocrashed'; urls = @(
            'https://www.resplendence.com/images/whocrashed.ico'
        ) },
    @{ slug = 'advanced-ip-scanner'; urls = @(
            'https://www.advanced-ip-scanner.com/img/logo.svg',
            'https://www.advanced-ip-scanner.com/favicon.ico'
        ) },
    @{ slug = 'vcredist'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/visual-studio-code.svg',
            'https://upload.wikimedia.org/wikipedia/commons/2/2a/Microsoft_Visual_C%2B%2B.png'
        ) },
    @{ slug = 'dotnet'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/dotnet.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/dotnet.svg'
        ) },
    @{ slug = 'quick-share'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/quickshare.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/googlenearby.svg'
        ) },
    @{ slug = 'localsend'; urls = @(
            'https://cdn.jsdelivr.net/gh/selfhst/icons/svg/localsend.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/localsend.svg'
        ) },
    @{ slug = 'affinity'; urls = @(
            'https://affinity.serif.com/favicon.ico',
            'https://www.google.com/s2/favicons?domain=affinity.serif.com&sz=128'
        ) },
    @{ slug = 'obsidian'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/obsidian.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/obsidian.svg'
        ) },
    @{ slug = 'ntlite'; urls = @(
            'https://www.ntlite.com/favicon.ico'
        ) },
    @{ slug = 'syncthing'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/syncthing.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/syncthing.svg'
        ) },
    @{ slug = 'balena-etcher'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/balena-etcher.svg',
            'https://cdn.jsdelivr.net/gh/balena-io/etcher/assets/icon.png'
        ) },
    @{ slug = 'fluent-search'; urls = @(
            'https://www.fluentsearch.net/favicon.ico'
        ) },
    @{ slug = 'floorp'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/floorp.svg',
            'https://floorp.app/icon.png'
        ) },
    @{ slug = 'powertoys'; urls = @(
            'https://cdn.jsdelivr.net/gh/selfhst/icons/svg/microsoft-powertoys.svg'
        ) },
    @{ slug = 'activitywatch'; urls = @(
            'https://raw.githubusercontent.com/ActivityWatch/activitywatch/master/media/logo/logo.png',
            'https://activitywatch.net/img/logo.png'
        ) },
    @{ slug = 'sharex'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/sharex.svg',
            'https://getsharex.com/img/ShareX_Logo.png'
        ) },
    @{ slug = 'espanso'; urls = @(
            'https://espanso.org/img/logo.svg',
            'https://espanso.org/img/logo.png'
        ) },
    @{ slug = 'autohotkey'; urls = @(
            'https://www.autohotkey.com/static/ahk_logo_no_text.svg',
            'https://www.autohotkey.com/favicon.ico'
        ) },
    @{ slug = 'quicklook'; urls = @(
            'https://cdn.jsdelivr.net/gh/QL-Win/QuickLook/QuickLook/Resources/app.png'
        ) },
    @{ slug = 'copyq'; urls = @(
            'https://cdn.jsdelivr.net/gh/hluk/CopyQ/src/images/icon.svg'
        ) },
    @{ slug = 'stremio'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/stremio.svg'
        ) },
    @{ slug = 'winaero-tweaker'; urls = @(
            'https://winaero.com/blog/wp-content/uploads/2024/07/WinaeroTweaker163.png'
        ) },
    @{ slug = 'claude'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/claude.svg'
        ) },
    @{ slug = 'proton-drive'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/proton-drive.svg'
        ) },
    @{ slug = 'proton-pass'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/proton-pass.svg'
        ) },
    @{ slug = 'proton-mail'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/proton-mail.svg'
        ) },
    @{ slug = 'proton-vpn'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/proton-vpn.svg'
        ) },
    @{ slug = 'tailscale'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/tailscale.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/tailscale.svg'
        ) },
    @{ slug = 'signal'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/signal.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/signal.svg'
        ) },
    @{ slug = 'brave'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/brave.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/brave.svg'
        ) },
    @{ slug = 'librewolf'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/librewolf.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/librewolf.svg'
        ) },
    @{ slug = 'cryptomator'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/cryptomator.svg',
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/cryptomator.svg'
        ) },
    @{ slug = 'ferdium'; urls = @(
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/ferdium.svg',
            'https://cdn.jsdelivr.net/gh/ferdium/ferdium-app/build-helpers/images/icon.png'
        ) },
    @{ slug = 'bleachbit'; urls = @(
            'https://www.bleachbit.org/favicon.ico'
        ) },
    # Apple support bundles (iTunes, AppleMobileDeviceSupport, etc.) embed
    # generic Windows icons in their EXEs — use the brand SVG instead.
    @{ slug = 'apple'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/apple.svg'
        ) },
    @{ slug = 'itunes'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/itunes.svg'
        ) },

    # ── Previously loaded from cdn.simpleicons.org at runtime (L8 removal) ──
    # These slugs were in SIMPLE_ICONS_SLUG in appIcons.ts and in
    # BROWSER_ICON_SLUG in BrowserHardeningSection.tsx. They are now
    # bundled at build time so runtime zero-telemetry is preserved.
    @{ slug = 'firefox'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/firefoxbrowser.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/firefox.svg'
        ) },
    @{ slug = 'edge'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/microsoftedge.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/microsoft-edge.svg'
        ) },
    @{ slug = 'discord'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/discord.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/discord.svg'
        ) },
    @{ slug = 'telegram'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/telegram.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/telegram.svg'
        ) },
    @{ slug = 'whatsapp'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/whatsapp.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/whatsapp.svg'
        ) },
    @{ slug = 'slack'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/slack.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/slack.svg'
        ) },
    @{ slug = 'teams'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/microsoftteams.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/microsoft-teams.svg'
        ) },
    @{ slug = 'zoom'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/zoom.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/zoom.svg'
        ) },
    @{ slug = 'thunderbird'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/thunderbird.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/thunderbird.svg'
        ) },
    @{ slug = 'vlc'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/vlcmediaplayer.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/vlc.svg'
        ) },
    @{ slug = 'spotify'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/spotify.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/spotify.svg'
        ) },
    @{ slug = 'obs'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/obsstudio.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/obs-studio.svg'
        ) },
    @{ slug = 'plex'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/plex.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/plex.svg'
        ) },
    @{ slug = 'vscode'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/visualstudiocode.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/visual-studio-code.svg'
        ) },
    @{ slug = 'intellij'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/intellijidea.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/intellij-idea.svg'
        ) },
    @{ slug = 'notepad-plus-plus'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/notepadplusplus.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/notepad-plus-plus.svg'
        ) },
    @{ slug = 'docker'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/docker.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/docker.svg'
        ) },
    @{ slug = 'onedrive'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/microsoftonedrive.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/onedrive.svg'
        ) },
    @{ slug = 'notion'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/notion.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/notion.svg'
        ) },
    @{ slug = 'acrobat-reader'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/adobeacrobatreader.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/adobe-acrobat-reader.svg'
        ) },
    @{ slug = 'icloud'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/icloud.svg'
        ) },
    @{ slug = 'dropbox'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/dropbox.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/dropbox.svg'
        ) },
    @{ slug = 'google-drive'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/googledrive.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/google-drive.svg'
        ) },
    @{ slug = '7zip'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/sevenzip.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/7-zip.svg'
        ) },
    @{ slug = 'steam'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/steam.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/steam.svg'
        ) },
    @{ slug = 'epic-games'; urls = @(
            'https://cdn.jsdelivr.net/npm/simple-icons@latest/icons/epicgames.svg',
            'https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/epic-games.svg'
        ) }
)

function Get-ExtensionFromUrl([string]$url) {
    $clean = ($url -split '\?')[0]
    $ext = [System.IO.Path]::GetExtension($clean).ToLower()
    if ([string]::IsNullOrWhiteSpace($ext)) { return '.svg' }
    return $ext
}

function Test-IsRealAsset([string]$path, [string]$ext) {
    if (-not (Test-Path $path)) { return $false }
    $size = (Get-Item $path).Length
    if ($size -lt 100) { return $false }
    $bytes = [System.IO.File]::ReadAllBytes($path)
    if ($ext -eq '.svg') {
        $head = Get-Content $path -TotalCount 5 -ErrorAction SilentlyContinue
        return ($head -join '') -match '(?i)<svg|xml'
    }
    if ($ext -eq '.png') {
        return $bytes.Length -ge 8 -and $bytes[0] -eq 0x89 -and $bytes[1] -eq 0x50 -and $bytes[2] -eq 0x4E -and $bytes[3] -eq 0x47
    }
    if ($ext -eq '.gif') {
        return $bytes.Length -ge 6 -and [System.Text.Encoding]::ASCII.GetString($bytes, 0, 6) -match '^GIF8[79]a$'
    }
    if ($ext -eq '.ico') {
        return $bytes.Length -ge 4 -and $bytes[0] -eq 0x00 -and $bytes[1] -eq 0x00 -and $bytes[2] -eq 0x01 -and $bytes[3] -eq 0x00
    }
    return $true
}

$total = $catalog.Count
$hit = 0
$miss = 0
$skip = 0
$failures = @()

foreach ($entry in $catalog) {
    $slug = $entry.slug
    $existing = Get-ChildItem -Path $outDir -Filter "$slug.*" -ErrorAction SilentlyContinue
    if ($existing -and -not $Force) {
        Write-Verbose "[skip ] $slug (already exists: $($existing.Name))"
        $skip++
        continue
    }
    if ($Force -and $existing) {
        $existing | Remove-Item -Force
    }

    $downloaded = $false
    foreach ($url in $entry.urls) {
        $ext = Get-ExtensionFromUrl $url
        $outPath = Join-Path $outDir "$slug$ext"
        try {
            Invoke-WebRequest -Uri $url -OutFile $outPath -UseBasicParsing -TimeoutSec 15 -UserAgent 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36' -ErrorAction Stop
            if (Test-IsRealAsset $outPath $ext) {
                Write-Host "  + $slug" -ForegroundColor Green
                $hit++
                $downloaded = $true
                break
            } else {
                Remove-Item $outPath -Force -ErrorAction SilentlyContinue
            }
        } catch {
            # try next
        }
    }
    if (-not $downloaded) {
        Write-Host "  ! $slug - no source responded" -ForegroundColor Yellow
        $miss++
        $failures += $slug
    }
}

$parts = @("icons: $total total")
if ($skip -gt 0)  { $parts += "$skip cached" }
if ($hit -gt 0)   { $parts += "$hit fetched" }
if ($miss -gt 0)  { $parts += "$miss missing" }
$summary = $parts -join '  |  '

if ($miss -gt 0) {
    Write-Host $summary -ForegroundColor Yellow
} else {
    Write-Host $summary -ForegroundColor DarkGray
}
