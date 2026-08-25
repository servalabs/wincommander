import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { getProManifestPath } from "./pro-workspace";

const ROOT = resolve(import.meta.dir, "..");
const PRO_MANIFEST = getProManifestPath(ROOT);
const TARGET_DIR = resolve(ROOT, "src-tauri", "target");
const extraArgs = process.argv.slice(2);
const staticCrtFlags = "-C target-feature=+crt-static";
const rustflags = [process.env.RUSTFLAGS, staticCrtFlags].filter(Boolean).join(" ");

const result = spawnSync(
  "cargo",
  ["build", "-p", "commander-pro", ...extraArgs, "--manifest-path", PRO_MANIFEST, "--target-dir", TARGET_DIR],
  {
    cwd: ROOT,
    // Cargo reads config from its working directory, which is this public
    // checkout rather than the private manifest's directory. Keep local Pro
    // builds self-contained just like the private release path.
    env: { ...process.env, RUSTFLAGS: rustflags },
    stdio: "inherit",
    shell: false,
  },
);

if (result.error) throw result.error;
process.exit(result.status ?? 1);
