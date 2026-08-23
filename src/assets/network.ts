import {
  CAN_LOAD_BROWSER_ASSET_MAPS,
  EMPTY_ASSET_MODS,
  mergeByBasename,
  type AssetMods,
} from "./runtime";

const softwareMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../../assets/softwares/**/*.{svg,png,webp,ico,gif,jpg,jpeg,avif}", {
      eager: true,
      query: "?url&wc-module",
      import: "default",
    })
  : EMPTY_ASSET_MODS) as AssetMods;
const blocklistSoftwareMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../../assets/softwares/blocklist/**/*", {
      eager: true,
      query: "?url&wc-module",
      import: "default",
    })
  : EMPTY_ASSET_MODS) as AssetMods;
const entityMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob("../../assets/entities/**/*", {
      eager: true,
      query: "?url&wc-module",
      import: "default",
    })
  : EMPTY_ASSET_MODS) as AssetMods;

export const companyLogos = mergeByBasename(entityMods);
export const agencyLogos = mergeByBasename(entityMods);
export const blocklistLogos = mergeByBasename(entityMods, softwareMods, blocklistSoftwareMods);
export const flags = mergeByBasename(entityMods);
export const software = mergeByBasename(softwareMods);
export const saas = mergeByBasename(softwareMods);
