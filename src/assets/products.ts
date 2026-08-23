type AssetMods = Record<string, string>;
const EMPTY_ASSET_MODS: AssetMods = {};
const CAN_LOAD_BROWSER_ASSET_MAPS = typeof document !== "undefined";

function byBasename(mods: AssetMods): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [path, url] of Object.entries(mods)) out[path.slice(path.lastIndexOf("/") + 1)] = url;
  return out;
}

function byRelativePath(mods: AssetMods, marker: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [path, url] of Object.entries(mods)) {
    const index = path.indexOf(marker);
    if (index !== -1) out[path.slice(index + marker.length)] = url;
  }
  return out;
}

function prefer(map: Record<string, string>, ...keys: string[]): string | undefined {
  for (const key of keys) {
    if (map[key] !== undefined) return map[key];
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
  const aliases = Object.entries(PRODUCT_ALIAS_CANDIDATES).reduce<Record<string, string>>((out, [target, fallbacks]) => {
    const value = prefer(productMap, target, ...fallbacks);
    if (value !== undefined) out[target] = value;
    return out;
  }, {});
  return { ...productMap, ...aliases };
}

const contingencyMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../../assets/products/contingency/usb-decoy-hub.png", { eager: true, query: "?url&wc-module", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const privateServerMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob([
    "../../assets/products/private-server/pro-nobg.png",
    "../../assets/products/private-server/max-nobg.png",
    "../../assets/products/private-server/contingency/panic-button.webm",
    "../../assets/products/private-server/contingency/panic-button.mp4",
    "../../assets/products/private-server/contingency/watering-plant.webm",
    "../../assets/products/private-server/contingency/watering-plant.mp4",
    "../../assets/products/private-server/contingency/phone-click.webm",
    "../../assets/products/private-server/contingency/phone-click.mp4",
  ], { eager: true, query: "?url&wc-module", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const winCommanderMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob([
    "../../assets/products/wincommander/Scrub.gif",
    "../../assets/products/wincommander/Extention.png",
    "../../assets/products/wincommander/videos/wc-lockdown.mp4",
    "../../assets/products/wincommander/searching.gif",
  ], { eager: true, query: "?url&wc-module", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;

const productMap = [contingencyMods, privateServerMods, winCommanderMods].reduce<Record<string, string>>(
  (out, mods) => Object.assign(out, byRelativePath(mods, "/products/")),
  {},
);

export const products = applyProductAliases(productMap);
export const ui = {
  ...byBasename(contingencyMods),
  ...byBasename(privateServerMods),
  ...byBasename(winCommanderMods),
};
