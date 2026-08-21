import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const configPath = resolve(root, "src-tauri", "commander-free", "tauri.conf.json");
const generatedConfigPath = resolve(root, "src-tauri", "commander-free", "tauri.release.generated.json");

function run(command: string[], label: string) {
  const result = Bun.spawnSync(command, { cwd: root, stdout: "inherit", stderr: "inherit" });
  if (result.exitCode !== 0) throw new Error(`${label} failed with exit code ${result.exitCode}`);
}

run(
  ["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-svc", "--release"],
  "WinCommander service release build",
);

const config = JSON.parse(readFileSync(configPath, "utf8")) as {
  bundle: { resources: string[] };
};
const serviceResource = "../target/release/wincommander-svc.exe";
config.bundle.resources = [...config.bundle.resources.filter(resource => resource !== serviceResource), serviceResource];

writeFileSync(generatedConfigPath, `${JSON.stringify(config, null, 2)}\n`);
try {
  run(["bun", "x", "tauri", "build", "--config", generatedConfigPath], "Tauri release bundle");
} finally {
  rmSync(generatedConfigPath, { force: true });
}
