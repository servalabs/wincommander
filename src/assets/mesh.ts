import { CAN_LOAD_BROWSER_ASSET_MAPS, EMPTY_ASSET_MODS, mergeByBasename, type AssetMods } from "./runtime";

const serviceMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../../assets/apps/**/*", {
      eager: true,
      query: "?url&wc-module",
      import: "default",
    })
  : EMPTY_ASSET_MODS) as AssetMods;

export const serviceIcons = mergeByBasename(serviceMods);
