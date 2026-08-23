import { byBasename, CAN_LOAD_BROWSER_ASSET_MAPS, EMPTY_ASSET_MODS, type AssetMods } from "./runtime";

const browserLogoMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../../assets/softwares/**/*.{svg,png,webp,ico,gif,jpg,jpeg,avif}", {
      eager: true,
      query: "?url&wc-module",
      import: "default",
    })
  : EMPTY_ASSET_MODS) as AssetMods;

export const browserLogos = byBasename(browserLogoMods);
