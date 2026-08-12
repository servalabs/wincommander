import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { getProManifestPath } from "./pro-workspace";

const ROOT = resolve(import.meta.dir, "..");
const PRO_MANIFEST = getProManifestPath(ROOT);
const TARGET_DIR = resolve(ROOT, "src-tauri", "target");
const extraArgs = process.argv.slice(2);

const result = spawnSync(
  "cargo",
  ["build", "-p", "commander-pro", ...extraArgs, "--manifest-path", PRO_MANIFEST, "--target-dir", TARGET_DIR],
  {
    cwd: ROOT,
    env: process.env,
    stdio: "inherit",
    shell: false,
  },
);

if (result.error) throw result.error;
process.exit(result.status ?? 1);
