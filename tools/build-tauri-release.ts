import { copyFileSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const configPath = resolve(root, "src-tauri", "commander-free", "tauri.conf.json");
const generatedConfigPath = resolve(root, "src-tauri", "commander-free", "tauri.release.generated.json");
const serviceBuildPath = resolve(root, "src-tauri", "target", "release", "wincommander-svc.exe");
// Tauri resolves bundle-resource sources from the repository root, then places
// them below the installer's `resources` directory. Stage the file at that
// root so the installed path is exactly
// `$INSTDIR\\resources\\wincommander-svc.exe`, which is where the NSIS hooks
// load it from. Staging it under a source `resources/` folder would instead
// create a nested `resources\\resources\\...` path and abort installation.
const stagedServicePath = resolve(root, "wincommander-svc.exe");
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

const config = JSON.parse(readFileSync(configPath, "utf8")) as {
  bundle: { resources: string[]; targets: string | string[] };
};
const serviceResource = "wincommander-svc.exe";
config.bundle.resources = [...config.bundle.resources.filter(resource => resource !== serviceResource), serviceResource];
// Only NSIS executes the service lifecycle hooks. Emitting an MSI here would
// produce a package that installs the UI but silently omits WinCommanderSvc.
config.bundle.targets = ["nsis"];

copyFileSync(serviceBuildPath, stagedServicePath);
writeFileSync(generatedConfigPath, `${JSON.stringify(config, null, 2)}\n`);
try {
  run(["bun", "x", "tauri", "build", "--config", generatedConfigPath], "Tauri release bundle");
} finally {
  rmSync(generatedConfigPath, { force: true });
  rmSync(stagedServicePath, { force: true });
}
