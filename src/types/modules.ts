// src/types/modules.ts
//
// ═══════════════════════════════════════════════════════════════════════
// MODULE DEFINITIONS — Controls which features are loaded & polled
// ═══════════════════════════════════════════════════════════════════════
//
// Modules are the coarse-grained on/off switches for entire feature areas.
// When a module is OFF:
//   - Its sidebar panel is dimmed + sorted to the bottom of its group and shows
//     an inline power-dot to re-enable in one click (NOT hidden — see Sidebar.tsx
//     `sidebar-item--module-off`; full hiding is a separate Secret-Settings feature)
//   - Clicking the row is a no-op (routing into a disabled panel bounces to dashboard)
//   - No backend polling runs for it
//   - No background processes spawn (e.g. Privacy Shield AI, productivity tracker)
//   - Dashboard radar skips its scan checks
//
// By default, only basic modules are enabled (dashboard, network, tweaks, apps).
// Help & Setup sets defaults based on the selected experience level.
// Users can manually toggle any module regardless of experience level.
//
// STORED AT: settings.json → app.modules  (flat Record<ModuleId, boolean>)

import type { PanelId } from './panels';
import type { ExperienceLevel, ThreatPersona } from './settings';

// ── Module IDs ───────────────────────────────────────────────────────

export type ModuleId =
  | 'privacy'        // Privacy telemetry & capability toggles
  | 'privacyShield'  // AI camera-based privacy shield (heavy — Python + YOLO)
  | 'cleanup'      // Privacy Clean: shred, erase, trace cleanup
  | 'network'        // DNS & domain blocklists
  | 'tweaks'         // System tweaks & OS hardening
  | 'apps'           // Winget package manager
  | 'vault'          // Encryption Engine encrypted volumes
  | 'mesh'           // MeshVPN private mesh network
  | 'productivity'   // Productivity Engine activity tracker
  | 'serverApps'     // Self-hosted server apps
  | 'searchFiles'    // Instant Search Engine file search
  | 'flows';         // Automation engine (triggers → actions)

// ── Module Config (persisted in settings.json) ───────────────────────

/** Flat map of module ID → enabled. Missing keys default to false. */
export type ModuleConfig = Partial<Record<ModuleId, boolean>>;

// ── Module Definition ────────────────────────────────────────────────

export interface ModuleDef {
  id: ModuleId;
  label: string;
  description: string;
  /** Which panel(s) this module controls. Empty = no panel (sub-module). */
  panels: PanelId[];
  /** Icon name for the module toggle UI */
  icon: string;
  /** Category for grouping in the UI */
  category: 'core' | 'security' | 'privacy' | 'tools' | 'advanced';
  /** If true, module requires external software */
  requiresDependency?: boolean;
}

// ── Module Manifest ──────────────────────────────────────────────────

export const MODULE_DEFS: ModuleDef[] = [
  {
    id: 'network',
    label: 'Network & DNS',
    description: 'DNS provider, domain blocklists, firewall rules',
    panels: ['network'],
    icon: 'globe-network',
    category: 'core',
  },
  {
    id: 'tweaks',
    label: 'System Tweaks',
    description: 'OS hardening, power plans, explorer tweaks, security settings',
    panels: ['tweaks'],
    icon: 'console',
    category: 'core',
  },
  {
    id: 'apps',
    label: 'Package Manager',
    description: 'Install, update, and remove software',
    panels: ['apps'],
    icon: 'applications',
    category: 'core',
    requiresDependency: true,
  },
  {
    id: 'privacy',
    label: 'Privacy & Telemetry',
    description: 'Block telemetry, manage app capabilities, clipboard & tracking controls',
    panels: ['privacy'],
    icon: 'shield',
    category: 'privacy',
  },
  {
    id: 'privacyShield',
    label: 'AI Privacy Shield',
    description: 'Camera-based gaze detection, anti-shoulder-surfing, screen blur',
    panels: [],  // sub-module within privacy panel
    icon: 'eye-open',
    category: 'privacy',
  },
  {
    id: 'cleanup',
    label: 'Privacy Clean',
    description: 'Trace cleanup, secure file deletion, USB/Bluetooth history cleanup',
    panels: ['cleanup'],
    icon: 'flame',
    category: 'privacy',
  },
  {
    id: 'vault',
    label: 'Encrypted Volumes',
    description: 'Create and mount encrypted containers via the Encryption Engine',
    panels: ['vault'],
    icon: 'lock',
    category: 'security',
    requiresDependency: true,
  },
  {
    id: 'mesh',
    label: 'Private Mesh VPN',
    description: 'Private mesh networking between devices via MeshVPN',
    panels: ['private-mesh'],
    icon: 'ip-address',
    category: 'security',
    requiresDependency: true,
  },
  {
    id: 'productivity',
    label: 'Productivity Tracker',
    description: 'Local activity monitoring and time analytics via Productivity Engine',
    panels: ['productivity'],
    icon: 'timeline-events',
    category: 'tools',
    requiresDependency: true,
  },
  {
    id: 'serverApps',
    label: 'Server Apps',
    description: 'Self-hosted apps in embedded browser',
    panels: ['server-apps'],
    icon: 'cloud',
    category: 'tools',
  },
  {
    id: 'searchFiles',
    label: 'File Search',
    description: 'Instant file search powered by the Instant Search Engine',
    panels: [],
    icon: 'search',
    category: 'tools',
    requiresDependency: true,
  },
  {
    id: 'flows',
    label: 'Automation Flows',
    description: 'Trigger → condition → action automation chains',
    panels: ['flows'],
    icon: 'data-lineage',
    category: 'advanced',
  },
];

// ── Default modules per experience level ─────────────────────────────
// These are applied during setup based on the profile the user picks.
// Users can always override individual modules afterwards.

const SIMPLE_DEFAULTS: ModuleId[] = ['network', 'tweaks', 'apps', 'searchFiles'];

const STANDARD_DEFAULTS: ModuleId[] = [
  ...SIMPLE_DEFAULTS,
  'privacy',
  'mesh',
];

const ADVANCED_DEFAULTS: ModuleId[] = [
  ...STANDARD_DEFAULTS,
  'privacyShield',
  'cleanup',
  'vault',
  'mesh',
  'productivity',
  'serverApps',
];

/** Build the default ModuleConfig for a given experience level. */
export function getDefaultModules(level: ExperienceLevel): ModuleConfig {
  const ids =
    level === 'simple' ? SIMPLE_DEFAULTS :
    level === 'standard' ? STANDARD_DEFAULTS :
    ADVANCED_DEFAULTS;

  const config: ModuleConfig = {};
  for (const def of MODULE_DEFS) {
    config[def.id] = ids.includes(def.id);
  }
  return config;
}

// ── Default modules per persona (threat-model axis) ──────────────────
// Orthogonal to experience level: "secure" is today's all-on advanced default;
// "casual" is the same set with cleanup/flows/vault forced off (still
// discoverable, not hidden — see Sidebar/panel power-dot handling). Active
// behavioral monitors stay on for casual; they protect everyday users too.

// The three coarse feature areas the persona axis governs. Secure turns them
// ON, Casual turns them OFF — deterministically, regardless of what the base
// advanced defaults happen to include (ADVANCED_DEFAULTS omits 'flows', so we
// must set these explicitly or Secure would silently leave Flows off and the
// "Casual turns off Flows" differentiation would be meaningless).
export const PERSONA_CONTROLLED_MODULES: ModuleId[] = ['cleanup', 'flows', 'vault'];

/** Build the default ModuleConfig for a given threat persona. */
export function modulesForPersona(persona: ThreatPersona): ModuleConfig {
  const config = getDefaultModules('advanced');
  const enabled = persona === 'secure';
  for (const id of PERSONA_CONTROLLED_MODULES) {
    config[id] = enabled;
  }
  return config;
}

/** Check if a module is enabled in the given config. Missing = false. */
export function isModuleEnabled(config: ModuleConfig | undefined, id: ModuleId): boolean {
  return config?.[id] === true;
}

/** Get the ModuleDef for a given ID. */
export function getModuleDef(id: ModuleId): ModuleDef | undefined {
  return MODULE_DEFS.find(m => m.id === id);
}

// ── Panel → Module mapping ───────────────────────────────────────────
// Reverse lookup: given a panel ID, which module controls it?

const PANEL_TO_MODULE = new Map<PanelId, ModuleId>();
for (const mod of MODULE_DEFS) {
  for (const panelId of mod.panels) {
    PANEL_TO_MODULE.set(panelId, mod.id);
  }
}

/** Get the module that controls a given panel. Returns undefined for dashboard (always on). */
export function getModuleForPanel(panelId: PanelId): ModuleId | undefined {
  return PANEL_TO_MODULE.get(panelId);
}
