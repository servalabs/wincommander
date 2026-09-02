import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, renameSync } from "node:fs";
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
if (result.status !== 0) process.exit(result.status ?? 1);

// The development Free app starts Pro from target/debug, while the debug
// SYSTEM service starts the dedicated ProgramData `.dev` sidecar. Keep those
// endpoints on the same build without touching the production sidecar or its
// signed metadata, which an installed update is allowed to replace.
if (!extraArgs.includes("--release")) {
  const proBinary = resolve(TARGET_DIR, "debug", "wincommander-pro.exe");
  const managedDir = "C:\\ProgramData\\WinCommander\\bin";
  const managedBinary = resolve(managedDir, "wincommander-pro.dev.exe");
  const temporaryBinary = `${managedBinary}.dev-sync`;

  try {
    mkdirSync(managedDir, { recursive: true });
    copyFileSync(proBinary, temporaryBinary);
    renameSync(temporaryBinary, managedBinary);
    console.log("[build:pro] synced the isolated dev Pro sidecar for the SYSTEM service.");
  } catch (error) {
    console.warn(
      "[build:pro] could not sync the dev Pro sidecar for the SYSTEM service; vault broker tests require an elevated development terminal:",
      error,
    );
  }
}
