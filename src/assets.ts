// Single source of truth for bundled asset URLs.
//
// WinCommander-owned runtime assets are vendored in `assets-shared`. Components
// reference them through the maps below — keyed by file basename — instead of
// hard-coded paths. Keeping the required files in this repository makes public
// clones and CI builds independent of ServaLabs' private asset repository.
//
// We use Vite `import.meta.glob` (not static `import` statements like the
// servalabs.com website) because WinCommander resolves some assets dynamically
// at runtime — app icons are looked up by winget slug, blocklist logos by
// category — so we need the whole folder, keyed by name, available as a map.
//
// IMPORTANT: `import.meta.glob` does NOT resolve path aliases (`@assets`).
// The glob patterns below MUST be RELATIVE to this file
// (`../assets-shared/<folder>/*`). Each match is eagerly resolved to its final
// fingerprinted URL via `{ eager: true, query: '?url', import: 'default' }`.
//
// KT: `import.meta.glob` is a Vite build-time transform with no Bun runtime
// implementation. `tools/check-tier-invariants.ts` (`bun run lint:tiers`)
// transitively imports this module (via src/registry/features.ts, for its
// tier/risk metadata only — it never reads asset URLs), so every call below
// is guarded behind a DOM check. That keeps Bun/Node imports on the empty-map
// path, while browser builds still take the transformed object-literal branch
// at runtime. Guarding on `typeof import.meta.glob` is NOT safe here because
// Vite leaves that runtime check in place after replacing the glob call, which
// would make the browser discard the bundled asset map.

type AssetMods = Record<string, string>;
const EMPTY_ASSET_MODS: AssetMods = {};
const CAN_LOAD_BROWSER_ASSET_MAPS = typeof document !== "undefined";

/** Reduce a glob result (keyed by full relative path) to a record keyed by
 *  the file's basename, e.g. "../assets-shared/logos/companies/x.svg" -> "x.svg". */
function byBasename(mods: AssetMods): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [path, url] of Object.entries(mods)) {
    const base = path.slice(path.lastIndexOf("/") + 1);
    out[base] = url;
  }
  return out;
}

function mergeByBasename(...groups: AssetMods[]): Record<string, string> {
  return groups.reduce((acc, group) => Object.assign(acc, byBasename(group)), {} as Record<string, string>);
}

function byRelativePath(mods: AssetMods, marker: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [path, url] of Object.entries(mods)) {
    const idx = path.indexOf(marker);
    if (idx === -1) continue;
    out[path.slice(idx + marker.length)] = url;
  }
  return out;
}

function mergeByRelativePath(marker: string, ...groups: AssetMods[]): Record<string, string> {
  return groups.reduce(
    (acc, group) => Object.assign(acc, byRelativePath(group, marker)),
    {} as Record<string, string>,
  );
}

function prefer(map: Record<string, string>, ...keys: string[]): string | undefined {
  for (const key of keys) {
    if (map[key]) return map[key];
  }
  return undefined;
}

const PRODUCT_ALIAS_CANDIDATES: Record<string, string[]> = {
  "contingency/decoy-hub.png": ["contingency/usb-decoy-hub.png"],
  "private-phone/hero.png": ["private-phone/phone-top-view.png"],
  "private-server/base.png": ["private-server/pro-nobg.png"],
  "private-server/base-model.png": ["private-server/pro-nobg.png"],
  "private-server/pro.png": ["private-server/pro-nobg.png"],
  "private-server/pro-model.png": ["private-server/pro-nobg.png"],
  "private-server/hero.png": ["private-server/max-nobg.png", "private-server/max-with-bg.jpg"],
  "private-server/servaultmax.jpg": ["private-server/max-with-bg.jpg"],
  "theron/hero.png": ["theron/end-to-end-monitoring-system-ai-cam-ai-server.png"],
  "theron/ai-chat.png": ["theron/theron-chat.png"],
  "theron/map-graph.png": ["theron/theron-entity-graph.png"],
  "theron/army-pilot.png": ["theron/openwebui-rag-chat-army-data-darkmode.png"],
  "theron/Chat-docs-lightmode.png": ["theron/openwebui-rag-chat-army-data-lightmode.png"],
  "theron/tank-detection.jpeg": ["theron/ai-vision-enemy-tank-humans-detection.png"],
  "theron/new_enemy_detection.png": ["theron/ai-vision-enemy-tank-humans-detection.png"],
  "theron/military-fusion-board.png": ["theron/end-to-end-monitoring-system-ai-cam-ai-server.png"],
  "wincommander/hero.png": ["wincommander/wc-dashboard-lightmode.png", "wincommander/wc-dashboard.png"],
  "wincommander/dashboard.png": ["wincommander/wc-dashboard.png"],
  "wincommander/forensic-trace-removal.png": ["wincommander/wc-forensic-cleanup.png"],
  "wincommander/privacy-settings.png": ["wincommander/wc-privacy-settings.png"],
  "wincommander/wincmd-dashboard.png": ["wincommander/wc-dashboard-with-callouts.png"],
  "wincommander/wincmd-network-control.png": ["wincommander/wc-network-control-honeypot-callout.png"],
  "wincommander/WinCommander_Honeypot-section.png": ["wincommander/wc-honeypot-lightmode.png"],
  "wincommander/Wincommander_dashboard.png": ["wincommander/wc-dashboard-lightmode.png"],
};

export function applyProductAliases(productMap: Record<string, string>): Record<string, string> {
  const aliases = Object.entries(PRODUCT_ALIAS_CANDIDATES).reduce<Record<string, string>>(
    (acc, [target, fallbacks]) => {
      const value = prefer(productMap, target, ...fallbacks);
      if (value !== undefined) {
        acc[target] = value;
      }
      return acc;
    },
    {},
  );

  return {
    ...productMap,
    ...aliases,
  };
}

const softwaresMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/softwares/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const entitiesMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/entities/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const appsMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/apps/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const defameMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/defame/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const contingencyProductMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/products/contingency/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const privateServerProductMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/products/private-server/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const winCommanderProductMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../assets-shared/products/wincommander/*", { eager: true, query: "?url", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;

// — app icons (svg/png/ico/gif), keyed by full filename incl. extension —
export const appIcons: Record<string, string> = mergeByBasename(
  softwaresMods,
);

// — browser brand logos, keyed by "<slug>.svg" —
export const browserLogos: Record<string, string> = byBasename(softwaresMods);

// — company / agency / blocklist logos —
export const companyLogos: Record<string, string> = mergeByBasename(
  entitiesMods,
);
export const agencyLogos: Record<string, string> = mergeByBasename(
  entitiesMods,
);
export const blocklistLogos: Record<string, string> = mergeByBasename(
  entitiesMods,
  softwaresMods,
);

// — flags (india/usa, emblems) —
export const flags: Record<string, string> = mergeByBasename(
  entitiesMods,
);

// — pirated/blocked software logos —
export const software: Record<string, string> = mergeByBasename(
  softwaresMods,
);

// — SaaS product logos (excel/gdrive/gphotos/icloud) —
export const saas: Record<string, string> = mergeByBasename(
  softwaresMods,
);

// — self-hosted service icons (the icons/ folder) —
export const serviceIcons: Record<string, string> = mergeByBasename(
  appsMods,
);

// — product shots (private-server, contingency, …) keyed by "<dir>/<file>" —
const productMap = mergeByRelativePath(
  "/products/",
  contingencyProductMods,
  privateServerProductMods,
  winCommanderProductMods,
);
export const products: Record<string, string> = applyProductAliases(productMap);

// — editorial imagery (PRISM slide, Gemini review, vintage Google ad, …) —
export const editorial: Record<string, string> = mergeByBasename(
  defameMods,
);

// — UI media (searching.gif + contingency demo videos) —
export const ui: Record<string, string> = mergeByBasename(
  contingencyProductMods,
  winCommanderProductMods,
);

// — brand (WinCommander logo, ServaLabs wallpaper) keyed by "<dir>/<file>" —
export const brand: Record<string, string> = {
  "wincommander/logo.png": products["wincommander/logo.png"],
};

/** WinCommander brand logo (was public/Logo.png). */
export const logo: string = brand["wincommander/logo.png"];
