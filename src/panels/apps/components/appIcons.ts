// Resolves a brand icon for each app in the winget manifest.
//
// Strategy:
//   1. Map the winget id to a slug (e.g. "Giorgiotani.Peazip" -> "peazip").
//   2. Resolve <slug>.svg, .png, .ico, then .gif against the bundled appIcons
//      manifest (now sourced mainly from assets/softwares/*, exposed via
//      src/assets.ts). If the asset
//      ships, the <img> renders. If not, onError swaps in a Blueprint icon
//      picked by category.
//   3. Drop a new SVG into assets/softwares/<slug>.svg to "light up"
//      any app without touching code — the slug is derived from the id below.

import type { IconName } from "@/components/ui/bp";
import { appIcons } from "@/assets";

const DISABLED_BUNDLED_BRAND_SLUGS = new Set([
  "audacity",
  "chrome",
  "gog-galaxy",
  "opera",
  "opera-gx",
  "pycharm",
  "skype",
]);

// Hand-mapped slugs for cases where the auto-derived slug isn't the obvious
// brand filename. Anything not listed here falls back to the auto-derived
// slug (lowercased part after the last dot, with dashes stripped).
const ID_TO_SLUG: Record<string, string> = {
  "Giorgiotani.Peazip": "peazip",
  "Voidtools.Everything.Cli": "everything",
  "DuongDieuPhap.ImageGlass": "imageglass",
  "Starpine.Screenbox": "screenbox",
  "IObit.DriverBooster": "driver-booster",
  "Nilesoft.Shell": "nilesoft-shell",
  "GitHub.GitHubDesktop": "github-desktop",
  "PostgreSQL.pgAdmin": "pgadmin",
  "Anysphere.Cursor": "cursor",
  "Google.Antigravity": "antigravity",
  "Google.AntigravityIDE": "antigravity",
  "Microsoft.WindowsTerminal": "windows-terminal",
  "Microsoft.VisualStudio.2026.Community": "visual-studio",
  "Microsoft.VisualStudio.2026.BuildTools": "visual-studio",
  "Microsoft.VisualStudio.2022.Community": "visual-studio",
  "Microsoft.VisualStudio.2022.BuildTools": "visual-studio",
  "OpenJS.NodeJS": "nodejs",
  "Oven-sh.Bun": "bun",
  "Cloudflare.cloudflared": "cloudflare",
  "Microsoft.PowerShell": "powershell",
  "Git.Git": "git",
  "Oracle.JDK.25": "java",
  "Python.Python.3.12": "python",
  "Python.Launcher": "python",
  "Mobatek.MobaXterm": "mobaxterm",
  "Gyan.FFmpeg": "ffmpeg",
  "Bopsoft.Listary": "listary",
  "PDFgear.PDFgear": "pdfgear",
  "AntibodySoftware.WizTree": "wiztree",
  "CodeSector.TeraCopy": "teracopy",
  "Vivaldi.Vivaldi": "vivaldi",
  "SoftDeluxe.FreeDownloadManager": "fdm",
  "Klocman.BulkCrapUninstaller": "bcu",
  "flux.flux": "flux",
  "REALiX.HWiNFO": "hwinfo",
  "CrystalDewWorld.CrystalDiskInfo": "crystaldiskinfo",
  "CrystalDewWorld.CrystalDiskMark": "crystaldiskmark",
  "smartmontools.smartmontools": "smartmontools",
  "WinsiderSS.SystemInformer": "system-informer",
  "Resplendence.WhoCrashed": "whocrashed",
  "Famatech.AdvancedIPScanner": "advanced-ip-scanner",
  "Microsoft.VCRedist.2015+.x64": "vcredist",
  "Microsoft.DotNet.DesktopRuntime.6": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.6.arm64": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.6.x64": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.6.x86": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.8": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.8.arm64": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.8.x64": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.8.x86": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.10": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.10.arm64": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.10.x64": "dotnet",
  "Microsoft.DotNet.DesktopRuntime.10.x86": "dotnet",
  "Microsoft.DotNet.Runtime.7": "dotnet",
  "Microsoft.DotNet.Runtime.7.arm64": "dotnet",
  "Microsoft.DotNet.Runtime.7.x64": "dotnet",
  "Microsoft.DotNet.Runtime.7.x86": "dotnet",
  "Microsoft.DotNet.Runtime.8": "dotnet",
  "Microsoft.DotNet.Runtime.8.arm64": "dotnet",
  "Microsoft.DotNet.Runtime.8.x64": "dotnet",
  "Microsoft.DotNet.Runtime.8.x86": "dotnet",
  "Microsoft.DotNet.Runtime.9": "dotnet",
  "Microsoft.DotNet.Runtime.9.arm64": "dotnet",
  "Microsoft.DotNet.Runtime.9.x64": "dotnet",
  "Microsoft.DotNet.Runtime.9.x86": "dotnet",
  "Microsoft.DotNet.Runtime.10": "dotnet",
  "Microsoft.DotNet.Runtime.10.arm64": "dotnet",
  "Microsoft.DotNet.Runtime.10.x64": "dotnet",
  "Microsoft.DotNet.Runtime.10.x86": "dotnet",
  "Google.QuickShare": "quick-share",
  "LocalSend.LocalSend": "localsend",
  "Canva.Affinity": "affinity",
  "Obsidian.Obsidian": "obsidian",
  "Nlitesoft.NTLite": "ntlite",
  "BillStewart.SyncthingWindowsSetup": "syncthing",
  "Balena.Etcher": "balena-etcher",
  "BlastApps.FluentSearch": "fluent-search",
  "Ablaze.Floorp": "floorp",
  "Microsoft.PowerToys": "powertoys",
  "ActivityWatch.ActivityWatch": "activitywatch",
  "ShareX.ShareX": "sharex",
  "Espanso.Espanso": "espanso",
  "AutoHotkey.AutoHotkey": "autohotkey",
  "QL-Win.QuickLook": "quicklook",
  "hluk.CopyQ": "copyq",
  "Zaarrg.StremioCommunity": "stremio",
  "Winaero.Tweaker": "winaero-tweaker",
  "Anthropic.Claude": "claude",
  "Proton.ProtonDrive": "proton-drive",
  "Proton.ProtonPass": "proton-pass",
  "Proton.ProtonMail": "proton-mail",
  "Proton.ProtonVPN": "proton-vpn",
  "Tailscale.Tailscale": "tailscale",
  "OpenWhisperSystems.Signal": "signal",
  "Brave.Brave": "brave",
  "LibreWolf.LibreWolf": "librewolf",
  "Cryptomator.Cryptomator": "cryptomator",
  "Ferdium.Ferdium": "ferdium",
  "BleachBit.BleachBit": "bleachbit",
  // Rclone — cloud sync CLI, sometimes pulled in as a winget search hit.
  "Rclone.Rclone": "rclone",
  // Apple support bundles — iTunes drags these in; both share the Apple
  // logo so they collapse onto a single asset.
  "Apple.iTunes": "itunes",
  "Apple.AppleApplicationSupport": "apple",
  "Apple.AppleApplicationSupport.64": "apple",
  "Apple.AppleApplicationSupport.32": "apple",
  "Apple.AppleMobileDeviceSupport": "apple",
  "Apple.AppleMobileDeviceSupport.64": "apple",
  "Apple.AppleMobileDeviceSupport.32": "apple",
  // Plain GitHub IDs that aren't GitHubDesktop (CLI, the .com helper, etc.)
  "GitHub.cli": "github",
  "GitHub.GitHub": "github",
  "GitHub.GitLFS": "github",

  // ── Popular installed apps NOT in the curated catalog ──
  // These render via the user's `winget list` / pending-update path. Without
  // an explicit slug they auto-derive to "chrome", "firefox", etc. — which
  // also need files in the shared appIcons manifest. Channel variants (Beta/Dev/Canary)
  // collapse onto the same brand icon.

  // Browsers
  "Google.Chrome": "chrome",
  "Google.Chrome.Beta": "chrome",
  "Google.Chrome.Dev": "chrome",
  "Google.Chrome.Canary": "chrome",
  "Google.ChromeEnterprise": "chrome",
  "Mozilla.Firefox": "firefox",
  "Mozilla.Firefox.ESR": "firefox",
  "Mozilla.Firefox.DeveloperEdition": "firefox",
  "Mozilla.Firefox.Beta": "firefox",
  "Mozilla.Firefox.Nightly": "firefox",
  "Microsoft.Edge": "edge",
  "Microsoft.EdgeBeta": "edge",
  "Microsoft.EdgeDev": "edge",
  "Microsoft.EdgeCanary": "edge",
  "Opera.Opera": "opera",
  "Opera.OperaGX": "opera-gx",

  // Communication
  "Discord.Discord": "discord",
  "Discord.Discord.PTB": "discord",
  "Discord.Discord.Canary": "discord",
  "Telegram.TelegramDesktop": "telegram",
  "WhatsApp.WhatsApp": "whatsapp",
  "SlackTechnologies.Slack": "slack",
  "Microsoft.Teams": "teams",
  "Microsoft.Teams.Classic": "teams",
  "Skype.Skype": "skype",
  "Zoom.Zoom": "zoom",
  "Mozilla.Thunderbird": "thunderbird",
  "Mozilla.Thunderbird.Beta": "thunderbird",

  // Media
  "VideoLAN.VLC": "vlc",
  "Spotify.Spotify": "spotify",
  "OBSProject.OBSStudio": "obs",
  "Audacity.Audacity": "audacity",
  "Plex.Plex": "plex",
  "Plex.Plexamp": "plex",

  // Editors / dev
  "Microsoft.VisualStudioCode": "vscode",
  "Microsoft.VisualStudioCode.Insiders": "vscode",
  "JetBrains.IntelliJIDEA.Community": "intellij",
  "JetBrains.IntelliJIDEA.Ultimate": "intellij",
  "JetBrains.PyCharm.Community": "pycharm",
  "JetBrains.PyCharm.Professional": "pycharm",
  "Notepad++.Notepad++": "notepad-plus-plus",
  "Docker.DockerDesktop": "docker",

  // Productivity / cloud
  "Microsoft.OneDrive": "onedrive",
  "Notion.Notion": "notion",
  "Notion.NotionCalendar": "notion",
  "Adobe.Acrobat.Reader.64-bit": "acrobat-reader",
  "Adobe.Acrobat.Reader.32-bit": "acrobat-reader",
  "Adobe.AdobeReader": "acrobat-reader",
  "Apple.iCloud": "icloud",
  "Dropbox.Dropbox": "dropbox",
  "Google.GoogleDrive": "google-drive",

  // File tools
  "7zip.7zip": "7zip",
  "WinRAR.WinRAR": "winrar",
  "Bandisoft.Bandizip": "bandizip",

  // Gaming
  "Valve.Steam": "steam",
  "EpicGames.EpicGamesLauncher": "epic-games",
  "GOG.Galaxy": "gog-galaxy",

  // Catalog gap
  "ImDisk.Toolkit": "imdisk-toolkit",
};

// Category → Blueprint icon used when no brand asset is present. Keeps cards
// from showing a blank box for apps we haven't shipped an SVG for yet.
const CATEGORY_FALLBACK: Record<string, IconName> = {
  base: "application",
  dev: "code",
  mid: "wrench",
  "bench-mon": "pulse",
  depend: "cube",
  misc: "applications",
  power: "flash",
  privacy: "shield",
};

function slugFromId(id: string): string {
  if (ID_TO_SLUG[id]) return ID_TO_SLUG[id];
  const normalized = id.trim().toLowerCase();
  if (normalized.includes("affinity")) return "affinity";
  if (normalized.includes("antigravity")) return "antigravity";
  // Apple bundles (iTunes drags Apple.AppleMobileDeviceSupport.* in) —
  // every variant should pick up the apple icon, not fall through to
  // the slugified Win32 component name.
  if (normalized.startsWith("apple.")) return "apple";
  // Any GitHub.* that we haven't mapped explicitly still shows the
  // octocat instead of the slugified tail (e.g. "GitHub.cli" → "cli"
  // which would never find an icon).
  if (normalized.startsWith("github.")) return "github";
  // Rclone has only one winget id pattern but match defensively.
  if (normalized.startsWith("rclone.")) return "rclone";
  if (normalized.includes("cloudflared") || normalized.includes("cloudflare")) return "cloudflare";
  if (normalized.includes("visualstudio") || normalized.includes("visual-studio")) return "visual-studio";
  // Brand-channel collapses — Beta/Dev/Canary/Nightly/ESR variants all share
  // the parent brand icon. Catch future channel IDs we haven't enumerated.
  if (normalized.startsWith("google.chrome")) return "chrome";
  if (normalized.startsWith("mozilla.firefox")) return "firefox";
  if (normalized.startsWith("mozilla.thunderbird")) return "thunderbird";
  if (normalized.startsWith("microsoft.edge")) return "edge";
  if (normalized.startsWith("discord.")) return "discord";
  if (normalized.startsWith("microsoft.teams")) return "teams";
  if (normalized.startsWith("adobe.acrobat") || normalized.startsWith("adobe.adobereader")) return "acrobat-reader";
  if (normalized.startsWith("jetbrains.intellijidea")) return "intellij";
  if (normalized.startsWith("jetbrains.pycharm")) return "pycharm";
  if (normalized.startsWith("microsoft.visualstudiocode")) return "vscode";
  if (normalized.startsWith("plex.")) return "plex";
  if (
    normalized.includes("dotnet") ||
    normalized.includes(".net") ||
    normalized.includes("microsoft windows desktop runtime") ||
    normalized.includes("microsoft.net runtime") ||
    normalized.includes("microsoft .net runtime")
  ) {
    return "dotnet";
  }
  const tail = id.includes(".") ? id.split(".").pop()! : id;
  return tail.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
}

// Extension preference order — same fallback chain as before, now resolved
// against the bundled appIcons set in the shared-asset manifest.
const ICON_EXTS = ["svg", "png", "ico", "gif"] as const;

export function isBundledBrandSlugEnabled(slug: string): boolean {
  return !DISABLED_BUNDLED_BRAND_SLUGS.has(slug);
}

export function resolveBundledBrandSlug(id: string): string | undefined {
  const slug = slugFromId(id);
  return isBundledBrandSlugEnabled(slug) ? slug : undefined;
}

export function brandIconCandidates(id: string): string[] {
  const slug = resolveBundledBrandSlug(id);
  if (!slug) return [];
  // Resolve the slug against the bundled appIcons manifest, exposed by
  // src/assets.ts keyed by full filename. We keep the .svg→.png→
  // .ico→.gif preference, but only emit candidates that actually ship, so the
  // returned URLs are real fingerprinted asset URLs. The onError chain in
  // AppIcon.tsx still falls through to iconData / a Blueprint icon when the
  // slug has no bundled file at all. No CDN URLs (zero-telemetry).
  // Run `bun run fetch-icons` to add brand icons into assets/softwares/.
  const out: string[] = [];
  for (const ext of ICON_EXTS) {
    const url = appIcons[`${slug}.${ext}`];
    if (url) out.push(url);
  }
  return out;
}

export function fallbackIcon(category: string): IconName {
  const normalized = (category || "").trim().toLowerCase();
  return CATEGORY_FALLBACK[normalized] || "application";
}
