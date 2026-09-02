import { copyFileSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const configPath = resolve(root, "src-tauri", "commander-free", "tauri.conf.json");
const generatedConfigPath = resolve(root, "src-tauri", "commander-free", "tauri.release.generated.json");
const serviceBuildPath = resolve(root, "src-tauri", "target", "release", "wincommander-svc.exe");
const contextShredBuildPath = resolve(root, "src-tauri", "target", "release", "wincommander-context-shred.exe");
// Tauri validates resource paths relative to the application directory. Stage
// the service in that `resources/` folder, therefore producing
// `$INSTDIR\\resources\\wincommander-svc.exe`. The NSIS hook uses that separate
// copy to replace the root service only after it has backed up a running
// installation's previous executable.
const stagedServicePath = resolve(root, "src-tauri", "commander-free", "resources", "wincommander-svc.exe");
// Explorer must invoke a separate asInvoker binary for secure erase. The main
// Tauri executable intentionally keeps its highestAvailable manifest for
// privileged features, which would otherwise show UAC for every selected item.
const stagedContextShredPath = resolve(root, "src-tauri", "commander-free", "resources", "wincommander-context-shred.exe");
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
  ["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-svc", "--release"],
  "WinCommander service release build",
);
run(
  ["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-context-shred", "--release"],
  "WinCommander context-delete helper release build",
);

const config = JSON.parse(readFileSync(configPath, "utf8")) as {
  bundle: { resources: string[]; targets: string | string[] };
};
const serviceResource = "resources/wincommander-svc.exe";
const contextShredResource = "resources/wincommander-context-shred.exe";
config.bundle.resources = [
  ...config.bundle.resources.filter(resource => resource !== serviceResource && resource !== contextShredResource),
  serviceResource,
  contextShredResource,
];
// Only NSIS executes the service lifecycle hooks. Emitting an MSI here would
// produce a package that installs the UI but silently omits WinCommanderSvc.
config.bundle.targets = ["nsis"];

copyFileSync(serviceBuildPath, stagedServicePath);
copyFileSync(contextShredBuildPath, stagedContextShredPath);
writeFileSync(generatedConfigPath, `${JSON.stringify(config, null, 2)}\n`);
try {
  run(["bun", "x", "tauri", "build", "--config", generatedConfigPath], "Tauri release bundle");
} finally {
  rmSync(generatedConfigPath, { force: true });
  rmSync(stagedServicePath, { force: true });
  rmSync(stagedContextShredPath, { force: true });
}
