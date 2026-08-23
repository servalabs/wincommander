// src/content/guide/topics.ts
//
// The guide content SSOT. The spotlight tour is the only surface that reads
// this list now (the help center article system was removed) — a topic only
// does anything if it carries a `tour` block; every entry below has one.
//
// AV-CLEAN: this content is embedded in the Free binary. Never include the
// PowerShell command-name tokens in tools/strings-grep-forbidden.txt.

import type { GuideTopic } from "./types";
import { products } from "../../assets/products";
import PrivacyShieldAnimation from "../../panels/privacy/PrivacyShieldAnimation";

// Guide media belongs to the same pinned asset map as the rest of the app.
// Keeping URLs in that map lets Vite bundle them while Bun unit tests remain
// independent of binary-file module loading.
const scrubGif = products["wincommander/Scrub.gif"] ?? "";
const browserExtensionImage = products["wincommander/Extention.png"] ?? "";
const lockdownVideo = products["wincommander/videos/wc-lockdown.mp4"] ?? "";

export const GUIDE_TOPICS: GuideTopic[] = [
  // ── Concept / navigation topics ──────────────────────────────────────
  {
    id: "the-command-rail",
    title: "The command rail",
    summary: "The left sidebar groups every tool by what it does.",
    keywords: ["sidebar", "navigation", "menu", "rail", "panels"],
    related: ["command-palette"],
    tour: {
      anchor: '[data-tour="sidebar"]',
      placement: "right",
      tours: [{ id: "welcome", order: 10 }],
    },
    body: [
      "Everything lives in the rail on the left, grouped into **Monitor**, **Protect**, **Secure**, and **System**.",
      "",
      "- Click a group entry to open that screen.",
      "- The power dot next to some entries turns a whole module on or off.",
      "- Hover an entry to pre-load it so it opens instantly.",
    ].join("\n"),
  },
  {
    id: "health-score",
    title: "System health",
    summary: "The health score shows how well-protected this PC is right now.",
    keywords: ["score", "health", "posture", "status", "percent"],
    related: ["dashboard"],
    tour: {
      anchor: '[data-tour="health"]',
      placement: "bottom",
      tours: [{ id: "welcome", order: 20, densities: ["guided"] }],
    },
    body: [
      "The health pill in the title bar is a live read on your privacy and performance posture.",
      "",
      "- **Higher is better.** It rises as you apply recommended settings.",
      "- Open the **Dashboard** to see exactly what's pulling the number down and fix it in one click.",
    ].join("\n"),
  },
  {
    id: "command-palette",
    title: "Quick search",
    summary: "Press Ctrl+K to jump to any setting, file, or action.",
    keywords: ["search", "palette", "ctrl k", "cmd k", "jump", "find"],
    tour: {
      anchor: '[data-tour="search"]',
      placement: "bottom",
      tours: [{ id: "welcome", order: 40 }],
    },
    body: [
      "Press `Ctrl+K` (or click the search bar) to open the command palette.",
      "",
      "- Type a setting name to jump straight to it.",
      "- Search your files by name, or their contents.",
      "- Run common actions without hunting through screens.",
    ].join("\n"),
  },
  {
    id: "find-help",
    title: "Getting help later",
    summary: "The ? button reopens this guide and replays the tour any time.",
    keywords: ["help", "guide", "manual", "tour", "support"],
    related: ["welcome"],
    tour: {
      anchor: '[data-tour="help"]',
      placement: "bottom",
      tours: [{ id: "welcome", order: 50 }],
    },
    body: [
      "This **?** in the title bar opens the help center whenever you need it.",
      "",
      "- Search and read about any feature.",
      "- Click **Replay tour** to run this walkthrough again.",
      "- Every screen also has its own **?** for help in context.",
    ].join("\n"),
  },

  {
    id: "privacy",
    panelId: "privacy",
    title: "Privacy Settings",
    summary: "Turn off telemetry, tracking, and data collection.",
    keywords: ["telemetry", "tracking", "camera", "microphone", "permissions", "shield"],
    related: ["cleanup", "network"],
    tour: {
      anchor: '[data-tour="nav-privacy"]',
      placement: "right",
      navigateTo: "privacy",
      tours: [{ id: "welcome", order: 30, densities: ["guided"] }],
    },
    body: [
      "Privacy Settings shut down the ways Windows watches you — telemetry, activity history, ad tracking, and app permissions like camera and microphone.",
      "",
      "- Each toggle says plainly what it changes and what happens if it's off.",
      "- Safe, reversible defaults — flip a switch and WinCommander applies it for you.",
    ].join("\n"),
  },
  // ── Dashboard tour (tour-dashboard) — the full flow. Opens on the sidebar
  // itself (persona, order 5), then Dashboard's own hero moments (10-40),
  // then — via each step's own navigateTo — continues straight through
  // Privacy Settings (50-60), Network Control (65), Secure Storage (70), and
  // Packages & Apps (80, plus a conditional "install Brave" stop at 85 shown
  // only when useBraveInstalled() is false — see the `showWhen` predicate on
  // GuideTopic["tour"]) as one unbroken walkthrough (System Cleanup is
  // skipped in this flow — reach it via its own standalone tour). Those
  // panels' topics below carry a second tour-dashboard membership to
  // continue the sequence. Each panel also keeps its own standalone tour
  // (tour-privacy, tour-network, tour-cleanup, tour-vault, tour-apps) for
  // someone who lands there directly. ──
  {
    id: "dashboard-tour-persona",
    title: "Density & persona — set your defaults",
    summary: "Interface density (Guided/Expert) sets how hand-held the UI is; Persona (Casual/Secure) decides whether System Cleanup, Flows, and Encrypted Volumes start on. Switch either anytime from the sidebar footer.",
    keywords: ["persona", "casual", "secure", "density", "guided", "expert", "interface", "who is this for", "sidebar", "dashboard tour"],
    body: [
      "Two independent switches live at the bottom of the sidebar.",
      "",
      "**Interface density:**",
      "- **Guided** — a simpler, more hand-held UI.",
      "- **Expert** — full density, fewer training wheels.",
      "",
      "**Persona:**",
      "- **Casual** — leaves System Cleanup, Flows, and Encrypted Volumes off by default.",
      "- **Secure** — turns those three on by default.",
      "",
      "Either way, every module can still be toggled individually anytime.",
    ].join("\n"),
    tour: {
      anchor: '[data-tour="persona-density-switches"]',
      navigateTo: "dashboard",
      placement: "right",
      tours: [{ id: "tour-dashboard", order: 5 }],
    },
  },
  {
    id: "dashboard-tour-fix-all",
    title: "Fix everything, one click",
    summary: "Click Fix all — it resolves every open issue at once.",
    keywords: ["fix all", "fix everything", "dashboard tour"],
    body: "Fix all runs every recommended protection in one operation.",
    tour: {
      // Two spotlight cutouts, not just the button: the Needs-Attention card
      // (list + Fix All button — primary) and the radar (secondary). With
      // keepDim below, the tour scrim blurs everything OUTSIDE those two
      // regions (left/right cards + sidebar) and keeps them crisp until the
      // user clicks Fix All (2026-07-20). Anchoring the card (not the whole
      // centre column) keeps the callout placing cleanly to its right.
      anchor: '[data-tour="dashboard-fix-region"], [data-tour="dashboard-radar"]',
      secondaryAnchor: '[data-tour="dashboard-radar"]',
      navigateTo: "dashboard",
      placement: "right",
      requiresAction: {
        eventName: "tour-fix-all-done",
        warning: "This turns on the recommended protections in one go — including things like Clipboard History, which clears what's currently there. Review items individually below first if you'd rather be selective.",
        keepDim: true,
        alreadyStartedTitle: "Everything is already clear",
        alreadyStartedSummary: "There is nothing left to fix. The radar is clear because every finding has been resolved or intentionally ignored, so you can continue the tour.",
        hideWarningWhenPreStarted: true,
      },
      tours: [{ id: "tour-dashboard", order: 10 }],
    },
  },
  {
    id: "dashboard-tour-scrub",
    title: "Scrub Meta",
    summary: "Strips EXIF, PDF, and Office metadata from a file before you share it — GPS location, author name, edit history, all gone.",
    keywords: ["scrub", "metadata", "exif", "share safely", "dashboard tour"],
    body: "Scrub Meta lives in the right-hand action rail, available from any panel.",
    tour: {
      anchor: '[data-tour="right-sidebar-scrub"]',
      navigateTo: "dashboard",
      variant: "hero",
      media: { type: "image", src: scrubGif, alt: "Scrub Meta stripping EXIF, PDF, and Office metadata from a file" },
      showWhen: (ctx) => ctx.scrubMetadataVisible !== false,
      tours: [{ id: "tour-dashboard", order: 20 }],
    },
  },
  {
    id: "dashboard-tour-lockdown",
    title: "Lockdown",
    summary: "One button runs your configured emergency sequence — cleanup, uninstall, shutdown, whatever you've set up in Secret Settings. We won't trigger it for you — this is just so you know it's there.",
    keywords: ["lockdown", "self destruct", "panic", "emergency", "dashboard tour"],
    body: "Lockdown lives in the right-hand action rail. Configure the steps it runs in Secret Settings.",
    tour: {
      anchor: '[data-tour="right-sidebar-lockdown"]',
      navigateTo: "dashboard",
      variant: "hero",
      media: { type: "video", src: lockdownVideo },
      showWhen: (ctx) => ctx.lockdownVisible !== false,
      tours: [{ id: "tour-dashboard", order: 30 }],
    },
  },
  {
    id: "dashboard-tour-quick-toggles",
    title: "Camera, mic, internet — one click each",
    summary: "Quick kill switches for camera, microphone, and internet, right here. Flip them anytime you don't trust an app — no need to dig through Windows settings.",
    keywords: ["camera", "microphone", "internet", "kill switch", "dashboard tour"],
    body: "Quick privacy toggles for camera, microphone, and the internet kill switch.",
    tour: {
      anchor: '[data-tour="dashboard-privacy-toggles"]',
      navigateTo: "dashboard",
      placement: "right",
      tours: [{ id: "tour-dashboard", order: 40 }],
    },
  },
  // ── Privacy Settings tour (tour-privacy — standalone; also continues the
  // full flow started on Dashboard as tour-dashboard, orders 50-60) ──
  {
    id: "privacy-tour-browser-hardening",
    title: "Browser Hardening",
    summary: "Installs a privacy extension in your detected browsers — telemetry off, tracking blocked, sync/ads disabled.",
    keywords: ["browser", "hardening", "extension", "privacy tour"],
    body: "Browser Hardening installs a privacy extension and applies hardening policy to detected browsers.",
    tour: {
      anchor: '[data-tour="privacy-browser-hardening"]',
      navigateTo: "privacy",
      // The image stays large enough to show the extension's controls. The
      // tour placement logic measures the callout before final placement so
      // it remains above the highlighted card instead of covering its list.
      placement: "top",
      media: { type: "image", src: browserExtensionImage, alt: "Browser hardening extension controls" },
      tours: [
        { id: "tour-privacy", order: 10 },
        { id: "tour-dashboard", order: 50 },
      ],
    },
  },
  {
    id: "privacy-tour-shield",
    title: "Privacy Shield",
    summary: "Instantly blurs when threat detected",
    keywords: ["privacy shield", "gaze", "camera", "shoulder surfing", "privacy tour"],
    // Kept identical to the "How it works" copy in PrivacyShieldIntro.tsx so
    // the feature reads the same in the tour and the in-app dialog.
    body: "Privacy Gaze Shield uses local AI to detect faces and camera lenses without ever sending data to the cloud. When a threat is detected, it instantly blurs your screen to protect your sensitive information.",
    tour: {
      anchor: '[data-tour="privacy-shield-card"]',
      navigateTo: "privacy",
      // Rectangle callout (like RDP Idle, its neighbor in the same row),
      // not the hero pill+dead-center treatment — see the note on
      // Browser Hardening above for why.
      placement: "bottom",
      component: PrivacyShieldAnimation,
      tours: [
        { id: "tour-privacy", order: 20 },
        { id: "tour-dashboard", order: 60 },
      ],
    },
  },
  {
    id: "privacy-tour-rdp-idle",
    title: "RDP Idle",
    summary: "If you use Remote Desktop: auto-disconnects idle sessions, and can wipe RDP history and cached credentials the moment a session ends.",
    keywords: ["rdp", "remote desktop", "idle", "privacy tour"],
    body: "RDP Idle auto-disconnects idle Remote Desktop sessions and can clean up on disconnect.",
    tour: {
      anchor: '[data-tour="privacy-rdp-idle"]',
      navigateTo: "privacy",
      placement: "left",
      tours: [{ id: "tour-privacy", order: 30 }],
    },
  },

  // ── Network Control tour (tour-network — standalone; also continues the
  // full flow as tour-dashboard, order 65) ──
  {
    id: "network-tour-dns-firewall",
    title: "Start with DNS Firewall",
    summary: "DNS-category filtering blocks whole categories of sites at the lookup level before a connection is made.",
    keywords: ["dns", "firewall", "simple firewall", "network tour"],
    body: "DNS Firewall categories filter DNS lookups by category, such as ads, malware, and adult content.",
    tour: {
      anchor: '[data-tour="network-dns-firewall"]',
      navigateTo: "network",
      openEvent: "open-network-dns-blocklists",
      placement: "bottom",
      tours: [
        { id: "tour-network", order: 10 },
        { id: "tour-dashboard", order: 65 },
      ],
    },
  },
  {
    id: "network-tour-hosts-blocklists",
    title: "Then add Hosts Protection",
    summary: "Hosts-file blocklists add domain-based protection for telemetry, AI services, piracy, and more.",
    keywords: ["hosts file", "blocklist", "network tour"],
    body: "Hosts Protection blocks selected domains directly in the Windows hosts file.",
    tour: {
      anchor: '[data-tour="network-hosts-protection"]',
      navigateTo: "network",
      openEvent: "open-network-dns-blocklists",
      placement: "bottom",
      tours: [
        { id: "tour-network", order: 20 },
        { id: "tour-dashboard", order: 65.5 },
      ],
    },
  },

  // ── System Cleanup tour (tour-cleanup — standalone). Only Scan All also
  // continues the full flow as tour-dashboard order 66 (between Network
  // Control at 65 and Secure Storage at 70) — Process Review and One-Time
  // Actions stay tour-cleanup-only. ──
  {
    id: "cleanup-tour-scan-all",
    title: "Scan All",
    summary: "Click Scan All — it checks every cleanup item in the tab you are viewing right now.",
    keywords: ["scan all", "cleanup", "traces", "cleanup tour"],
    body: "Scan All checks every cleanup category in the selected group in one pass.",
    tour: {
      anchor: '.cleanup-scan-all-btn',
      navigateTo: "cleanup",
      // "right" preferred, but Scan All sits at the panel's top-right
      // corner — the flip logic in calloutStyle() will pick "left" (or
      // "bottom") automatically once it detects there's no room on the
      // right, rather than clamping the callout back onto the button.
      placement: "right",
      requiresAction: {
        eventName: "tour-cleanup-scan-done",
        // Shown instead of the normal copy when Scan All was already
        // clicked (or already finished) before this step displayed — the
        // button itself reports which via data-tour-state (see
        // CleanupCategoryGrid.tsx) (2026-07-10 fix: "if it is already
        // scanning and 'Take the tour' is clicked, it should check").
        alreadyStartedTitle: "Already checking your trace data",
        alreadyStartedSummary: "Scan All is already running — every card in this cleanup group will update with the trace data it finds, ready the moment it lands.",
      },
      tours: [
        { id: "tour-cleanup", order: 10 },
        { id: "tour-dashboard", order: 66 },
      ],
    },
  },
  {
    id: "cleanup-tour-process-review",
    title: "Process Review",
    summary: "Flags unsigned or unusually elevated running processes — a quick look at what's actually executing on this PC right now.",
    keywords: ["process review", "processes", "cleanup tour"],
    body: "Process Review lists running processes worth a second look — unsigned binaries and unusual elevation.",
    tour: {
      anchor: '[data-tour="cleanup-process-review"]',
      navigateTo: "cleanup",
      openEvent: "open-cleanup-actions-monitoring",
      placement: "left",
      tours: [{ id: "tour-cleanup", order: 20 }],
    },
  },
  {
    id: "cleanup-tour-one-time-actions",
    title: "One-Time Actions",
    summary: "Two background jobs you kick off once: Free Space Cleanup overwrites unallocated space per drive (SSD gets cipher + TRIM, HDD gets a DoD 3-pass overwrite) so deleted files can't be recovered, and Virtual Memory Purge disables hibernation and forces the pagefile to clear on shutdown. Both run in the background and can take 30+ minutes. (Force SSD TRIM runs from the Windows repair section further down this tab.)",
    keywords: ["one-time actions", "free space cleanup", "wipe free space", "virtual memory", "pagefile", "hibernation", "cleanup tour"],
    body: "One-Time Actions runs background maintenance you don't need to repeat: overwrite free space or purge virtual memory.",
    tour: {
      anchor: '[data-tour="cleanup-one-time-actions"]',
      navigateTo: "cleanup",
      openEvent: "open-cleanup-actions-monitoring",
      placement: "right",
      tours: [{ id: "tour-cleanup", order: 30 }],
    },
  },

  // ── System Maintenance tour (also continues the full dashboard flow) ──
  {
    id: "maintenance-tour-disk-cleanup",
    title: "Reclaim space instantly",
    summary: "That MB counter is live — it tallies exactly how much junk these categories will free, updated as you check them, before you ever click Clean.",
    keywords: ["disk cleanup", "reclaim space", "system maintenance"],
    body: "Disk Clean-Up shows a live running total of reclaimable space as you select categories, before you clear anything.",
    tour: {
      // The primary ring deliberately covers Windows Storage only. File
      // Hygiene shares the outer card but is a separate workflow. The
      // secondary ring narrows attention to the live selected-space value.
      anchor: '[data-tour="maintenance-disk-cleanup"]',
      secondaryAnchor: '[data-tour="maintenance-disk-cleanup-actions"]',
      navigateTo: "maintenance",
      openEvent: "open-maintenance-storage",
      placement: "right",
      tours: [
        { id: "tour-dashboard", order: 70 },
      ],
    },
  },

  // ── Packages & Apps tour (tour-apps — standalone; also continues the
  // full flow as tour-dashboard, order 80) — plus a conditional "install
  // Brave" stop (order 30 / 85) shown only when useBraveInstalled() reads
  // false, via the `showWhen` predicate resolveTourSteps checks against the
  // TourContext GuideHost passes in. ──
  {
    id: "apps-tour-updates",
    title: "Update every app, one click",
    summary: "See how many updates are waiting in the badge up top, then hit Update All to grab every one of them at once.",
    keywords: ["update all", "app updates", "apps tour"],
    body: "Update All installs every pending app update in one operation.",
    tour: {
      // Prefer highlighting the actual per-app update cards; fall back to
      // the Update All / Update Selected button group when there are no
      // pending updates to show cards for. apps-updates-grid is always in
      // the DOM (see AppInstallerPanel) but only measures to something real
      // once there's content inside it — useTour's anchor resolver treats
      // "matched but empty" the same as "not found" and falls through to
      // apps-update-section, so this stays correct regardless of async
      // inventory-load timing.
      anchor: '[data-tour="apps-updates-grid"], [data-tour="apps-update-section"]',
      navigateTo: "apps",
      openEvent: "apps-open-install-tab",
      placement: "bottom",
      tours: [
        { id: "tour-apps", order: 10 },
        { id: "tour-dashboard", order: 80 },
      ],
    },
  },
  {
    id: "apps-tour-utilities",
    title: "Curated apps, by category",
    summary: "Tap Privacy, Developer, System, or another tab to surface trusted, productivity-boosting and security-hardening apps worth installing — no generic app-store search needed.",
    keywords: ["utility apps", "catalog", "productivity", "apps tour"],
    body: "Browse the curated app catalog by category to quickly install trusted productivity and security tools.",
    tour: {
      anchor: '[data-tour="apps-utility-section"]',
      navigateTo: "apps",
      openEvent: "apps-open-install-tab",
      placement: "bottom",
      tours: [{ id: "tour-apps", order: 20 }],
    },
  },
  {
    id: "apps-tour-install-brave",
    title: "Add Brave — blocks trackers by default",
    summary: "Brave isn't installed yet. It blocks ads and trackers out of the box — a stronger starting point than Chrome or Edge before WinCommander's own hardening even runs.",
    keywords: ["brave", "browser", "install", "apps tour"],
    body: "Brave ships with tracker and ad blocking on by default — a stronger base than Chrome or Edge for the hardening WinCommander applies on top.",
    tour: {
      anchor: '[data-tour="apps-utility-section"]',
      navigateTo: "apps",
      openEvent: "apps-open-install-tab",
      placement: "bottom",
      // Deep-links into Packages & Apps' existing install flow rather than
      // requiring a real "Install Brave" element to exist in the DOM yet —
      // both listeners (AppInstallerPanel, BackgroundPollers) already handle
      // this event outside of the tour.
      action: { label: "Install Brave", eventName: "apps-install-missing", eventDetail: { appIds: ["Brave.Brave"] } },
      showWhen: (ctx) => ctx.braveInstalled !== true,
      tours: [
        { id: "tour-apps", order: 30 },
        { id: "tour-dashboard", order: 85 },
      ],
    },
  },
];
