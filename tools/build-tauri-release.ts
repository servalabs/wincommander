import { copyFileSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const configPath = resolve(root, "src-tauri", "commander-free", "tauri.conf.json");
const generatedConfigPath = resolve(root, "src-tauri", "commander-free", "tauri.release.generated.json");
const contextShredBuildPath = resolve(root, "src-tauri", "target", "release", "wincommander-context-shred.exe");
const serviceBuildPath = resolve(root, "src-tauri", "target", "release", "wincommander-svc.exe");
const contextDeleteIconPath = resolve(
  root,
  "src-tauri",
  "commander-free",
  "icons",
  "context-delete.ico",
);
// Explorer invokes a separate asInvoker binary for secure erase so selected
// files are handled by a narrow helper rather than the long-lived desktop app.
const stagedContextShredPath = resolve(root, "src-tauri", "commander-free", "resources", "wincommander-context-shred.exe");
const stagedServicePath = resolve(root, "src-tauri", "commander-free", "resources", "wincommander-svc.exe");
const stagedContextDeleteIconPath = resolve(
  root,
  "src-tauri",
  "commander-free",
  "resources",
  "context-delete.ico",
);
// Release installers must run on a clean Windows machine. Link the MSVC C
// runtime into both the service and the Tauri application so they do not
// require a separately installed VCRUNTIME140.dll.
const staticCrtFlags = "-C target-feature=+crt-static";
const rustflags = [process.env.RUSTFLAGS, staticCrtFlags].filter(Boolean).join(" ");

function run(command: string[], label: string) {
  const result = Bun.spawnSync(command, {
    cwd: root,
    env: { ...process.env, RUSTFLAGS: rustflags },
    stdout: "inherit",
    stderr: "inherit",
  });
  if (result.exitCode !== 0) throw new Error(`${label} failed with exit code ${result.exitCode}`);
}

run(
  ["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-context-shred", "--release"],
  "WinCommander context-delete helper release build",
);
run(
  ["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-svc", "--release"],
  "WinCommander machine-service release build",
);
const config = JSON.parse(readFileSync(configPath, "utf8")) as {
  bundle: { resources: string[]; targets: string | string[] };
};
const contextShredResource = "resources/wincommander-context-shred.exe";
const contextDeleteIconResource = "resources/context-delete.ico";
const serviceResource = "resources/wincommander-svc.exe";
config.bundle.resources = [
  ...config.bundle.resources.filter(
    resource => resource !== contextShredResource
      && resource !== contextDeleteIconResource
      && resource !== serviceResource,
  ),
  contextShredResource,
  contextDeleteIconResource,
  serviceResource,
];
// Keep one signed NSIS artifact for the updater. The machine-wide installer
// places the immutable application binary in Program Files, while each person
// still gets their own settings when they run it without elevation.
config.bundle.targets = ["nsis"];

copyFileSync(contextShredBuildPath, stagedContextShredPath);
copyFileSync(contextDeleteIconPath, stagedContextDeleteIconPath);
copyFileSync(serviceBuildPath, stagedServicePath);
writeFileSync(generatedConfigPath, `${JSON.stringify(config, null, 2)}\n`);
try {
  run(["bun", "x", "tauri", "build", "--config", generatedConfigPath], "Tauri release bundle");
} finally {
  rmSync(generatedConfigPath, { force: true });
  rmSync(stagedContextShredPath, { force: true });
  rmSync(stagedContextDeleteIconPath, { force: true });
  rmSync(stagedServicePath, { force: true });
}
