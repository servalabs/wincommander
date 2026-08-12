// src/panels/apps/debloatLists.ts
// Curated lists grounded in Raphire/Win11Debloat Config/Apps.json (release 2026.06.11)
// Refresh each major Windows version cycle.

// ── Never show: would break Windows or are unreinstallable ──────────────────
const SYSTEM_CRITICAL: string[] = [
  "1527c705-839a-4832-9118-54d4Bd6a0c89",
  "c5e2524a-ea46-4f67-841f-6a9465d9d515",
  "E2A4F912-2574-4A75-9BB0-0D023378592B",
  "F46D4000-FD22-4DB4-AC8E-4E1DDDE828FE",
  "Microsoft.AAD.BrokerPlugin",
  "Microsoft.AccountsControl",
  "Microsoft.AsyncTextService",
  "Microsoft.BioEnrollment",
  "Microsoft.CredDialogHost",
  "Microsoft.ECApp",
  "Microsoft.LockApp",
  "Microsoft.MicrosoftEdgeDevToolsClient",
  "Microsoft.Win32WebViewHost",
  "Microsoft.Windows.Apprep.ChxApp",
  "Microsoft.Windows.CapturePicker",
  "Microsoft.Windows.CloudExperienceHost",
  "Microsoft.Windows.NarratorQuickStart",
  "Microsoft.Windows.OOBENetworkCaptivePortal",
  "Microsoft.Windows.OOBENetworkConnectionFlow",
  "Microsoft.Windows.PeopleExperienceHost",
  "Microsoft.Windows.PinningConfirmationDialog",
  "Microsoft.Windows.ShellExperienceHost",
  "Microsoft.Windows.StartMenuExperienceHost",
  "Microsoft.Windows.XGpuEjectDialog",
  "windows.immersivecontrolpanel",
  "Windows.PrintDialog",
  "Windows.CBSPreview",
  "MicrosoftWindows.Client.CBS",
  "MicrosoftWindows.Client.Core",
  "MicrosoftWindows.Client.FileExp",
  "MicrosoftWindows.Client.OOBE",
  "MicrosoftWindows.Client.Photon",
  "MicrosoftWindows.UndockedDevKit",
  // Package manager / store — unreinstallable; removal breaks app installs
  "Microsoft.DesktopAppInstaller",
  "Microsoft.WindowsStore",
  "Microsoft.StorePurchaseApp",
  "Microsoft.OfficePushNotificationUtility",
  "aimgr",
  // Essential tools & security
  "Microsoft.WindowsNotepad",
  "Microsoft.WindowsTerminal",
  "Microsoft.SecHealthUI",
  "Microsoft.CommandPalette",
  // Unreinstallable per Win11Debloat warnings
  "Microsoft.XboxSpeechToTextOverlay",
  // Codecs & media extensions
  "Microsoft.HEVCVideoExtensions",
  "Microsoft.WebMediaExtensions",
  "Microsoft.WebpImageExtension",
  "Microsoft.VP9VideoExtensions",
  "Microsoft.AV1VideoExtension",
  "Microsoft.RawImageExtension",
  // Hardware vendor drivers
  "NVIDIACorp.NVIDIAControlPanel",
  "NearbyShare",
];

const SYSTEM_PREFIXES: string[] = [
  "Microsoft.WinAppRuntime.",
  "MicrosoftCorporationII.WinAppRuntime.",
  "Microsoft.Winget.",
  "Microsoft.Services.Store.",
  "Microsoft.NET.",
  "Microsoft.VCLibs.",
  "Microsoft.UI.Xaml.",
  "Microsoft.DirectX",
  "Microsoft.WebView2",
  "MicrosoftWindows.57",
  "MicrosoftWindows.59",
  "Microsoft.VisualStudioCode",
  "Microsoft.PowerToys.",
  "5319275A.",
  "18496Starpine.",
  "Realtek.",
  "AMDInc.",
  "IntelCorporation.",
];

export function isSystemCritical(name: string): boolean {
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-/i.test(name)) return true;
  const lower = name.toLowerCase();
  if (SYSTEM_CRITICAL.some(c => c.toLowerCase() === lower)) return true;
  if (SYSTEM_PREFIXES.some(p => lower.startsWith(p.toLowerCase()))) return true;
  return false;
}

// ── Friendly names ──────────────────────────────────────────────────────────
const FRIENDLY_NAMES: Record<string, string> = {
  "Microsoft.549981C3F5F10": "Cortana",
  "Microsoft.3DBuilder": "3D Builder",
  "Microsoft.Advertising.Xaml": "Advertising SDK",
  "Microsoft.BingFinance": "Bing Finance",
  "Microsoft.BingFoodAndDrink": "Bing Food & Drink",
  "Microsoft.BingHealthAndFitness": "Bing Health & Fitness",
  "Microsoft.BingMaps": "Bing Maps",
  "Microsoft.BingNews": "Bing News",
  "Microsoft.BingSearch": "Bing Search",
  "Microsoft.BingSports": "Bing Sports",
  "Microsoft.BingTranslator": "Bing Translator",
  "Microsoft.BingTravel": "Bing Travel",
  "Microsoft.BingWeather": "Bing Weather",
  "Microsoft.Clipchamp": "Clipchamp",
  "Microsoft.Copilot": "Copilot",
  "Microsoft.GamingApp": "Xbox Gaming App",
  "Microsoft.GetHelp": "Get Help",
  "Microsoft.Getstarted": "Get Started / Tips",
  "Microsoft.Microsoft3DViewer": "3D Viewer",
  "Microsoft.MicrosoftEdge.Stable": "Microsoft Edge",
  "Microsoft.MicrosoftJournal": "Journal",
  "Microsoft.MicrosoftOfficeHub": "Office Hub",
  "Microsoft.MicrosoftPowerBIForWindows": "Power BI",
  "Microsoft.MicrosoftSolitaireCollection": "Solitaire Collection",
  "Microsoft.MicrosoftStickyNotes": "Sticky Notes",
  "Microsoft.MicrosoftTeamsforSurfaceHub": "Teams for Surface Hub",
  "Microsoft.MixedReality.Portal": "Mixed Reality Portal",
  "Microsoft.MSPaint": "Paint 3D",
  "Microsoft.NetworkSpeedTest": "Network Speed Test",
  "Microsoft.News": "Microsoft News",
  "Microsoft.Office.Excel": "Excel",
  "Microsoft.Office.OneNote": "OneNote",
  "Microsoft.Office.PowerPoint": "PowerPoint",
  "Microsoft.Office.Sway": "Sway",
  "Microsoft.Office.Word": "Word",
  "Microsoft.OneConnect": "Paid Wi-Fi & Cellular",
  "Microsoft.OutlookForWindows": "Outlook for Windows",
  "Microsoft.Paint": "Paint",
  "Microsoft.People": "People",
  "Microsoft.PCManager": "PC Manager",
  "Microsoft.PowerAutomateDesktop": "Power Automate Desktop",
  "Microsoft.Print3D": "Print 3D",
  "Microsoft.RemoteDesktop": "Remote Desktop",
  "Microsoft.ScreenSketch": "Snip & Sketch",
  "Microsoft.SkypeApp": "Skype",
  "Microsoft.StartExperiencesApp": "Start Experiences",
  "Microsoft.Todos": "Microsoft To Do",
  "Microsoft.Wallet": "Wallet",
  "Microsoft.Whiteboard": "Whiteboard",
  "Microsoft.WidgetsPlatformRuntime": "Widgets Platform",
  "Microsoft.Windows.AIHub": "Windows AI Hub",
  "Microsoft.Windows.DevHome": "Dev Home",
  "Microsoft.Windows.Photos": "Photos",
  "Microsoft.Windows.SecureAssessmentBrowser": "Take a Test",
  "Microsoft.WindowsAlarms": "Alarms & Clock",
  "Microsoft.WindowsCalculator": "Calculator",
  "Microsoft.WindowsCamera": "Camera",
  "Microsoft.WindowsFeedbackHub": "Feedback Hub",
  "Microsoft.WindowsMaps": "Maps",
  "Microsoft.WindowsSoundRecorder": "Sound Recorder",
  "Microsoft.Xbox": "Xbox",
  "Microsoft.Xbox.TCUI": "Xbox TCUI",
  "Microsoft.XboxApp": "Xbox Console Companion",
  "Microsoft.XboxGameOverlay": "Xbox Game Overlay",
  "Microsoft.XboxGamingOverlay": "Xbox Game Bar",
  "Microsoft.XboxIdentityProvider": "Xbox Identity Provider",
  "Microsoft.YourPhone": "Phone Link",
  "Microsoft.ZuneMusic": "Groove Music",
  "Microsoft.ZuneVideo": "Movies & TV",
  "MicrosoftCorporationII.MicrosoftFamily": "Microsoft Family Safety",
  "MicrosoftCorporationII.QuickAssist": "Quick Assist",
  "MicrosoftWindows.Client.WebExperience": "Web Experience (Widgets)",
  "microsoft.windowscommunicationsapps": "Mail & Calendar",
  "Clipchamp.Clipchamp": "Clipchamp",
  "Disney.37853FC22B2CE": "Disney+",
  "SpotifyAB.SpotifyMusic": "Spotify",
  "BytedancePte.Ltd.TikTok": "TikTok",
  "king.com.CandyCrushSaga": "Candy Crush Saga",
  "king.com.CandyCrushSodaSaga": "Candy Crush Soda",
  "king.com.BubbleWitch3Saga": "Bubble Witch 3",
  "king.com.FarmHeroesSaga": "Farm Heroes Saga",
  "FACEBOOK.FACEBOOK": "Facebook",
  "FACEBOOK.InstagramBeta": "Instagram Beta",
  "9E2F88E3.Twitter": "Twitter / X",
  "PandoraMediaInc.Pandora": "Pandora",
  "Flipboard.Flipboard": "Flipboard",
  "ShazamEntertainmentLtd.Shazam": "Shazam",
  "DolbyLaboratories.DolbyAccess": "Dolby Access",
  "A278AB0D.MarchofEmpires": "March of Empires",
  "A278AB0D.DisneyMagicKingdoms": "Disney Magic Kingdoms",
  "828B5831.HiddenCityMysteryofShadows": "Hidden City",
  "WinZipComputing.WinZipUniversal": "WinZip",
  "CAF9E577.Plex": "Plex",
  "D5EA27B7.Duolingo": "Duolingo",
  "MSTeams": "Microsoft Teams",
  "MicrosoftTeams": "Microsoft Teams",
  "Microsoft.Edge.GameAssist": "Edge Game Assist",
};

export function getFriendlyName(name: string): string {
  if (FRIENDLY_NAMES[name]) return FRIENDLY_NAMES[name];
  for (const [key, label] of Object.entries(FRIENDLY_NAMES)) {
    if (key.toLowerCase() === name.toLowerCase()) return label;
  }
  let friendly = name;
  const prefixes = ["Microsoft.", "MicrosoftCorporationII.", "MicrosoftWindows.", "Windows.", "Microsoft.Windows."];
  for (const p of prefixes) {
    if (friendly.startsWith(p)) { friendly = friendly.substring(p.length); break; }
  }
  return friendly
    .replace(/\./g, " ")
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2");
}

// ── Category assignment for Store items ────────────────────────────────────
const COMM_IDS = new Set([
  "MicrosoftTeams", "MSTeams", "Microsoft.SkypeApp", "Microsoft.Messaging",
  "Microsoft.YourPhone", "MicrosoftCorporationII.MicrosoftFamily",
]);
const MEDIA_IDS = new Set(["Microsoft.ZuneMusic", "Microsoft.ZuneVideo"]);
const OFFICE_IDS = new Set([
  "Microsoft.Office.OneNote", "Microsoft.Office.Sway", "Microsoft.Office.Excel",
  "Microsoft.Office.Word", "Microsoft.Office.PowerPoint", "Microsoft.MicrosoftOfficeHub",
  "Microsoft.MicrosoftPowerBIForWindows", "Microsoft.PowerAutomateDesktop", "Microsoft.Todos",
]);
const GAME_PREFIXES = [
  "king.com.", "A278AB0D.", "828B5831.", "BytedancePte.Ltd.", "SpotifyAB.", "Disney.",
  "HULULLC.", "PandoraMediaInc.", "CAF9E577.", "ShazamEntertainmentLtd.", "D5EA27B7.",
  "FACEBOOK.", "Amazon.", "AmazonVideo.", "Flipboard.", "WinZipComputing.", "9E2F88E3.",
];

export function getCategoryForStoreId(id: string): string {
  if (id.startsWith("Microsoft.Bing")) return "Bing";
  if (
    id.startsWith("Microsoft.Xbox") ||
    id === "Microsoft.GamingApp" ||
    id === "Microsoft.XboxApp"
  ) return "Xbox";
  if (COMM_IDS.has(id)) return "Communication";
  if (MEDIA_IDS.has(id)) return "Media";
  if (OFFICE_IDS.has(id)) return "Office & productivity";
  if (GAME_PREFIXES.some(p => id.startsWith(p))) return "Games & promos";
  return "Microsoft apps";
}

// ── Category display order ──────────────────────────────────────────────────
export const CATEGORY_ORDER: string[] = [
  "Games & promos",
  "Bing",
  "Xbox",
  "Communication",
  "Office & productivity",
  "Media",
  "Microsoft apps",
  "Windows extras",
  "Programs",
];

// ── Recommended (aggressive / Revi-AME profile) ────────────────────────────
// Win11Debloat SelectedByDefault=true (2026.06.11) + aggressive additions
export const RECOMMENDED_IDS = new Set<string>([
  // Win11Debloat defaults
  "Clipchamp.Clipchamp",
  "Microsoft.3DBuilder",
  "Microsoft.549981C3F5F10",
  "Microsoft.BingFinance",
  "Microsoft.BingFoodAndDrink",
  "Microsoft.BingHealthAndFitness",
  "Microsoft.BingNews",
  "Microsoft.BingSports",
  "Microsoft.BingTranslator",
  "Microsoft.BingTravel",
  "Microsoft.BingWeather",
  "Microsoft.Copilot",
  "Microsoft.Windows.AIHub",
  "Microsoft.PCManager",
  "Microsoft.Getstarted",
  "Microsoft.Messaging",
  "Microsoft.Microsoft3DViewer",
  "Microsoft.MicrosoftJournal",
  "Microsoft.MicrosoftOfficeHub",
  "Microsoft.MicrosoftPowerBIForWindows",
  "Microsoft.MicrosoftSolitaireCollection",
  "Microsoft.MicrosoftStickyNotes",
  "Microsoft.MixedReality.Portal",
  "Microsoft.NetworkSpeedTest",
  "Microsoft.News",
  "Microsoft.Office.OneNote",
  "Microsoft.Office.Sway",
  "Microsoft.OneConnect",
  "Microsoft.Print3D",
  "Microsoft.PowerAutomateDesktop",
  "Microsoft.SkypeApp",
  "Microsoft.Todos",
  "Microsoft.Windows.DevHome",
  "Microsoft.WindowsAlarms",
  "Microsoft.WindowsFeedbackHub",
  "Microsoft.WindowsMaps",
  "Microsoft.WindowsSoundRecorder",
  "Microsoft.XboxApp",
  "Microsoft.ZuneVideo",
  "MicrosoftCorporationII.MicrosoftFamily",
  "MicrosoftCorporationII.QuickAssist",
  "MicrosoftTeams",
  "MSTeams",
  // Third-party promos
  "Amazon.com.Amazon",
  "AmazonVideo.PrimeVideo",
  "BytedancePte.Ltd.TikTok",
  "Disney.37853FC22B2CE",
  "D5EA27B7.Duolingo",
  "FACEBOOK.FACEBOOK",
  "FACEBOOK.InstagramBeta",
  "Flipboard.Flipboard",
  "HULULLC.HULUPLUS",
  "king.com.BubbleWitch3Saga",
  "king.com.CandyCrushSaga",
  "king.com.CandyCrushSodaSaga",
  "king.com.FarmHeroesSaga",
  "A278AB0D.MarchofEmpires",
  "A278AB0D.DisneyMagicKingdoms",
  "828B5831.HiddenCityMysteryofShadows",
  "PandoraMediaInc.Pandora",
  "CAF9E577.Plex",
  "ShazamEntertainmentLtd.Shazam",
  "SpotifyAB.SpotifyMusic",
  "9E2F88E3.Twitter",
  "WinZipComputing.WinZipUniversal",
  // Aggressive additions
  "Microsoft.GamingApp",
  "Microsoft.XboxGameOverlay",
  "Microsoft.XboxGamingOverlay",
  "Microsoft.XboxIdentityProvider",
  "Microsoft.Xbox.TCUI",
  "MicrosoftWindows.Client.WebExperience",
  "Microsoft.OutlookForWindows",
  "Microsoft.YourPhone",
  // Windows extras IDs (matched by id field in useDebloatInventory)
  "edge",
  "onedrive",
  "teams",
  "copilot-ai",
]);
