// src/registry/features.ts
//
// ═══════════════════════════════════════════════════════════════════════
// FEATURES & BLOCKLISTS — Non-toggle capability registry (SSOT)
// ═══════════════════════════════════════════════════════════════════════
//
// Companion to the toggle registries in privacy/tweaks/capabilities.toggles.ts.
// This file holds capabilities that aren't simple on/off toggles:
//
//   - **Blocklists** — 9 hostsfile blocklists (telemetry, AI sites, cloud
//     upload, Adobe, etc.). They have an active/inactive state but are
//     rendered specially in the Network panel and First Run Wizard.
//   - **Future:** vault operations, mesh actions, Privacy Shield orchestration,
//     self-destruct, panic, USB key, etc. These will be added as the tier-split
//     rollout progresses (Phases 2-10).
//
// Every feature carries the same tier + 4-risk-boolean classification as
// toggles. CI invariants (in ref/architecture.md — Open-Core Architecture) apply.

import type { IconName } from "@/components/ui/bp";
import type { Tier } from "../types/toggles";
import { companyLogos, software, saas } from "@/assets";

// ═══════════════════════════════════════════════════════════════════════
// FEATURE DEFINITION — Generic non-toggle capability
// ═══════════════════════════════════════════════════════════════════════

export interface FeatureDef {
  /** Unique key. Convention: "<panel>.<feature>" e.g. "vault.create-volume",
   *  "blocklist.ai-sites", "self-destruct". */
  id: string;

  /** Which panel surfaces this feature. */
  panel: string;

  /** Display label. */
  label: string;

  /** One-line description. */
  description: string;

  // Tier & risk axes — same five required fields as ToggleDef.
  tier: Tier;
  needsAdmin: boolean;
  irreversible: boolean;
  reducesSecurity: boolean;
  defenderFlagged: boolean;
}

// ═══════════════════════════════════════════════════════════════════════
// BLOCKLIST FEATURE — Hostsfile blocklist with UI metadata
// ═══════════════════════════════════════════════════════════════════════
//
// Hostsfile rewrite (`%SystemRoot%\System32\drivers\etc\hosts`) — single
// admin-required write per blocklist. Domain lists themselves are bundled
// in the encrypted backend module `network/blocklists-data.enc`.

export interface BlocklistFeature extends FeatureDef {
  /** Emoji shown in the Network panel grid. */
  emoji?: string;

  /** Path to a brand logo image (rendered in Network panel + First Run Wizard). */
  logo?: string;

  /** Blueprint icon — fallback when no logo is available. */
  icon?: IconName;
}

// ═══════════════════════════════════════════════════════════════════════
// THE 9 BLOCKLISTS — single source of truth
// ═══════════════════════════════════════════════════════════════════════
//
// All free per user comment ("hostsfile rewrite is trivial; competing with
// free privacy tools wins adoption"). All need admin (hosts file is in
// %SystemRoot%). None are irreversible (re-enable just removes the entries).

export const BLOCKLISTS: BlocklistFeature[] = [
  {
    id: "blocklist.telemetry-blocklist",
    panel: "network",
    label: "Telemetry",
    description: "Block telemetry from apps, Windows, NVIDIA, and web analytics",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "📡",
    logo: companyLogos["nvidia-logo.svg"],
  },
  {
    id: "blocklist.ai-sites",
    panel: "network",
    label: "AI & LLM Services",
    description: "Block AI services and Large Language Model domains",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "🤖",
    logo: companyLogos["openai-logo.svg"],
  },
  {
    id: "blocklist.piracy-torrent",
    panel: "network",
    label: "Piracy & Torrents",
    description: "Block piracy, scene and torrent related domains",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "🧲",
    icon: "ban-circle",
  },
  {
    id: "blocklist.adobe",
    panel: "network",
    label: "Adobe Apps",
    description: "Block Adobe network endpoints",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "🎨",
    logo: companyLogos["adobe-logo.png"],
  },
  {
    id: "blocklist.autodesk",
    panel: "network",
    label: "Autodesk Apps",
    description: "Block Autodesk network endpoints",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "📐",
    logo: software["autocad.png"],
  },
  {
    id: "blocklist.corel",
    panel: "network",
    label: "Corel",
    description: "Block Corel network endpoints",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "🖌️",
    logo: software["coreldraw.png"],
  },
  {
    id: "blocklist.glasswire",
    panel: "network",
    label: "GlassWire",
    description: "Block GlassWire network endpoints",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "🔥",
    logo: software["glasswire.png"],
  },
  {
    id: "blocklist.lightburn",
    panel: "network",
    label: "LightBurn",
    description: "Block LightBurn network endpoints",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "💡",
    logo: software["lightburn.png"],
  },
  {
    id: "blocklist.cloud-upload",
    panel: "network",
    label: "Cloud Upload",
    description: "Block Cloud Upload (Dropbox, GDrive, OneDrive Personal, etc.)",
    tier: "free",
    needsAdmin: true,
    irreversible: false,
    reducesSecurity: false,
    defenderFlagged: false,
    emoji: "☁️",
    logo: saas["gdrive.svg"],
  },
];

export const FEATURES: FeatureDef[] = [];

/** Backend ID for a blocklist feature (the part the PowerShell hosts module
 *  expects — strips the "blocklist." prefix from the FeatureDef id).
 *
 *  Example: blocklistBackendId(BLOCKLISTS[1]) === "ai-sites" */
export function blocklistBackendId(b: BlocklistFeature): string {
  return b.id.replace(/^blocklist\./, "");
}

