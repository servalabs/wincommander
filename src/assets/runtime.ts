export type AssetMods = Record<string, string>;

export const EMPTY_ASSET_MODS: AssetMods = {};
export const CAN_LOAD_BROWSER_ASSET_MAPS = typeof document !== "undefined";

export function byBasename(mods: AssetMods): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [path, url] of Object.entries(mods)) {
    out[path.slice(path.lastIndexOf("/") + 1)] = url;
  }
  return out;
}

export function mergeByBasename(...groups: AssetMods[]): Record<string, string> {
  return groups.reduce(
    (out, group) => Object.assign(out, byBasename(group)),
    {} as Record<string, string>,
  );
}
