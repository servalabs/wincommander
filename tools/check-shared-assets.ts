import { statSync } from "node:fs";
import { resolve } from "node:path";

export const REQUIRED_SHARED_ASSETS = [
  "components/risk-matrix/index.ts",
  "components/risk-matrix/RiskMatrix.tsx",
  "entities/rsa-logo.svg",
  "editorial/nsa-prism-slide.png",
  "softwares/calc.png",
  "products/wincommander/logo.png",
] as const;

export function assertSharedAssets(root: string): void {
  const missing = REQUIRED_SHARED_ASSETS.filter((relative) => {
    try {
      return !statSync(resolve(root, "assets", relative)).isFile();
    } catch {
      return true;
    }
  });
  if (missing.length) {
    throw new Error(
      `Shared build assets are missing or incomplete: ${missing.join(", ")}. ` +
      "From the WinCommander repository, run git submodule update --init --recursive -- assets. " +
      "A source archive must include the pinned assets submodule, including components/risk-matrix.",
    );
  }
}

if (import.meta.main) {
  assertSharedAssets(resolve(import.meta.dir, ".."));
  console.log("Shared build assets are available.");
}
