import { CAN_LOAD_BROWSER_ASSET_MAPS, EMPTY_ASSET_MODS, mergeByBasename, type AssetMods } from "./runtime";

const entityMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob([
      "../../assets/entities/nvidia-logo.svg",
      "../../assets/entities/openai-logo.svg",
      "../../assets/entities/adobe-logo.png",
      "../../assets/entities/microsoft-logo.svg",
      "../../assets/entities/google-logo.svg",
    ], { eager: true, query: "?url&wc-module", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;
const softwareMods = (CAN_LOAD_BROWSER_ASSET_MAPS
  ? import.meta.glob([
      "../../assets/softwares/autocad.png",
      "../../assets/softwares/coreldraw.png",
      "../../assets/softwares/glasswire.png",
      "../../assets/softwares/lightburn.png",
      "../../assets/softwares/piratebay.png",
      "../../assets/softwares/gdrive.svg",
    ], { eager: true, query: "?url&wc-module", import: "default" })
  : EMPTY_ASSET_MODS) as AssetMods;

export const companyLogos = mergeByBasename(entityMods);
export const software = mergeByBasename(softwareMods);
export const saas = mergeByBasename(softwareMods);
