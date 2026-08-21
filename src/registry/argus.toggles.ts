// src/registry/argus.toggles.ts
//
// ═══════════════════════════════════════════════════════════════════════
// ARGUS DOMAIN — Toggle Registry
// ═══════════════════════════════════════════════════════════════════════
//
// Argus is the disclosed-consent app-usage / productivity monitoring
// subsystem. It lives entirely in the Pro sidecar (commander-pro/src/
// session_monitor.rs). All toggles are tier:"paid" + defenderFlagged:true
// because the Win32 foreground-window polling code will be flagged by AV.
//
// The panel (ArgusAppUsageSection.tsx) calls the Tauri invoke wrappers
// directly — it does NOT use enableCmd/disableCmd. The toggles here exist
// for the tier+risk registry so the invariant linter and the fleet
// compliance engine can see them.

import type { ToggleDef } from "../types/toggles";

export const ARGUS_TOGGLES: ToggleDef[] = [
  {
    // ── Tier & Risk ───────────────────────────────────────────────────
    tier: "paid",
    needsAdmin: false,
    irreversible: false,
    reducesSecurity: false,
    // Win32 foreground-window enumeration is flagged by behavioural AV rules.
    // Must live in the Pro sidecar — never the Free binary.
    defenderFlagged: true,

    // ── Identity ──────────────────────────────────────────────────────
    id: "argus-app-usage",
    label: "App-Usage Monitor",
    description: "Track active/idle time per app category for fleet productivity reporting",

    // ── Categorization ────────────────────────────────────────────────
    section: "argus",
    domain: "privacy",

    // ── Settings paths (required by ToggleDef, used for SSOT) ────────
    // Argus running-state is runtime-only (AtomicBool in Pro sidecar);
    // these paths represent the user's desired monitoring preference.
    settingsPath: "ideal.argus.appUsage.enabled",
    currentPath: "current.argus.appUsage.running",

    // ── Backend commands (thin Free invoke wrappers) ──────────────────
    enableCmd: "argus_app_usage_start",
    disableCmd: "argus_app_usage_stop",
    statusCmd: "argus_app_usage_status",

    // ── Cleanup Score — surveillance category ─────────────────────────
    cleanupScore: 8,
    cleanupScoreCategory: "surveillance",

    // ── Fleet ─────────────────────────────────────────────────────────
    complianceWeight: "optional",
  },

  // ── Argus DLP Monitor ───────────────────────────────────────────────────
  // Watches USB large-transfer events + clipboard sensitive-pattern hits +
  // coarse cloud-upload TCP signal. Requires admin for USB byte accounting.
  // Collector lives in commander-pro/src/dlp_monitor.rs (AV domain).
  {
    tier: "paid",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    // USB metering, ETW-backed clipboard sniff, and raw TCP connection
    // enumeration are all flagged by behavioural AV heuristics.
    defenderFlagged: true,

    id: "argus-dlp",
    label: "DLP Signal Monitor",
    description:
      "Detect large USB transfers, sensitive clipboard patterns, and coarse cloud-upload activity — aggregate counts only, no content",

    section: "argus",
    domain: "privacy",

    settingsPath: "ideal.argus.dlp.enabled",
    currentPath: "current.argus.dlp.running",

    enableCmd: "start_argus_dlp",
    disableCmd: "stop_argus_dlp",
    statusCmd: "argus_dlp_status",

    cleanupScore: 8,
    cleanupScoreCategory: "surveillance",

    complianceWeight: "optional",
  },

  // ── Argus Tamper Monitor ────────────────────────────────────────────────
  // Watches the evidence log-clear hook, consent revocations, Pro-binary
  // hash mismatches, and unexpected sidecar exits. Does NOT require admin
  // because it only observes signals already surfaced inside the Pro process.
  // Collector lives in commander-pro/src/tamper_monitor.rs.
  {
    tier: "paid",
    needsAdmin: false,
    irreversible: false,
    reducesSecurity: false,
    // Integrity-check code is flagged by some AV engines as anti-analysis.
    defenderFlagged: true,

    id: "argus-tamper",
    label: "Tamper Detection",
    description:
      "Detect evidence log clearance, consent revocations, Pro-binary integrity failures, and unexpected sidecar exits",

    section: "argus",
    domain: "privacy",

    settingsPath: "ideal.argus.tamper.enabled",
    currentPath: "current.argus.tamper.running",

    enableCmd: "start_argus_tamper",
    disableCmd: "stop_argus_tamper",
    statusCmd: "argus_tamper_status",

    cleanupScore: 8,
    cleanupScoreCategory: "surveillance",

    complianceWeight: "optional",
  },

  // ── Argus Print + USB Monitor ───────────────────────────────────────────
  // Watches Windows PrintService EventID 307 (print jobs) + USB mass-storage
  // attach/detach events. Both require admin-level event-channel access.
  // Collector lives in commander-pro/src/print_usb_monitor.rs.
  {
    tier: "paid",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    // ETW/WMI-backed USB enumeration and print-service event reading are
    // flagged by behavioural AV rules.
    defenderFlagged: true,

    id: "argus-print-usb",
    label: "Print & Removable Media Monitor",
    description:
      "Detect print jobs and USB mass-storage attach events — page counts and device classes only, no document names or paths",

    section: "argus",
    domain: "privacy",

    settingsPath: "ideal.argus.printUsb.enabled",
    currentPath: "current.argus.printUsb.running",

    enableCmd: "start_argus_print_usb",
    disableCmd: "stop_argus_print_usb",
    statusCmd: "argus_print_usb_status",

    cleanupScore: 8,
    cleanupScoreCategory: "surveillance",

    complianceWeight: "optional",
  },
];
