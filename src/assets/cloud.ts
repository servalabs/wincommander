import { CAN_LOAD_BROWSER_ASSET_MAPS, EMPTY_ASSET_MODS, mergeByBasename, type AssetMods } from "./runtime";

const cloudMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob([
      "../../assets/softwares/excel.svg",
      "../../assets/softwares/gdrive.svg",
      "../../assets/softwares/gphotos.svg",
      "../../assets/softwares/icloud.svg",
    ], { eager: true, query: "?url&wc-module", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;

export const saas = mergeByBasename(cloudMods);
