// tools/dev-all.ts — one-command dev launcher
//
// Starts the fleet-server (quiet) AND tauri dev together with all dev-unlock
// env vars pre-set so Pro features + fleet auto-enroll work from the first
// keystroke.  Ctrl+C stops both processes cleanly.
//
// Usage (added to package.json as "dev:all"):
//   bun run tools/dev-all.ts
//
// Fixed keypair (DEV ONLY — not a secret; only used locally):
//   FLEET_SIGNING_KEY_HEX  → server seed
//   FLEET_SIGNING_KEY_PUB  → agent public key (matches the seed above)
//
// KT: import.meta.env.DEV is false in a Vite *build*; always gate dev UX on
// the Rust cfg!(debug_assertions) flag surfaced via is_dev_build(), which is
// true for both `tauri dev` AND debug cargo builds.

import { spawn } from "node:child_process";
import { resolve } from "node:path";
import type { ChildProcess } from "node:child_process";
import { getProManifestPath } from "./pro-workspace";

const ROOT = resolve(import.meta.dir, "..");           // D:\GitHub\commander
const PRO_MANIFEST = getProManifestPath(ROOT);

// Stable dev keypair (committed; not a production secret — dev only).
const DEV_SEED_HEX = "41f8a43296aa0ca602e52d4e3a31c5fa340f7e58366707a1511cc3b83995c174";
const DEV_PUB_B64  = "IlTEngbNrLNvBB6tdABEQwIUw8VJ/flKumphz8P2xSs=";

// ── helpers ───────────────────────────────────────────────────────────────────

function prefix(tag: string, line: string): void {
  process.stdout.write(`${tag} ${line}\n`);
}

function spawnWithPrefix(
  tag: string,
  cmd: string,
  args: string[],
  opts: { cwd: string; env: NodeJS.ProcessEnv },
): ChildProcess {
  const child = spawn(cmd, args, {
    cwd: opts.cwd,
    env: opts.env,
    stdio: ["ignore", "pipe", "pipe"],
    shell: false,
  });
  child.stdout?.on("data", (buf: Buffer) => {
    for (const line of buf.toString().split(/\r?\n/)) {
      if (line) prefix(tag, line);
    }
  });
  child.stderr?.on("data", (buf: Buffer) => {
    for (const line of buf.toString().split(/\r?\n/)) {
      if (line) prefix(tag, line);
    }
  });
  return child;
}

// ── fleet-server ──────────────────────────────────────────────────────────────

const fleetEnv: NodeJS.ProcessEnv = {
  ...process.env,
  FLEET_BIND_ADDR:        "0.0.0.0:8787",
  FLEET_SIGNING_KEY_HEX: DEV_SEED_HEX,
  // "warn" suppresses the per-request INFO spam; errors + warns still show.
  RUST_LOG:               "warn,fleet_server=warn",
  RUST_MIN_STACK:         "16777216",
};

const fleetArgs = [
  "run", "-p", "fleet-server",
  "--manifest-path", PRO_MANIFEST,
];

// ── desktop development launcher ──────────────────────────────────────────────

const tauriEnv: NodeJS.ProcessEnv = {
  ...process.env,
  // Fleet auto-enroll: the agent picks these up via spawn_if_configured()
  // in fleet_push.rs and settings written by fleet_connect on first connect.
  FLEET_URL:               "http://127.0.0.1:8787",
  FLEET_DISPATCH:          "1",
  FLEET_SIGNING_KEY_PUB:   DEV_PUB_B64,
  RUST_MIN_STACK:          "16777216",
};

const tauriArgs = [
  // Use the supported launcher rather than invoking `tauri dev` directly.
  // tools/dev.ps1 synchronizes the SYSTEM Vault service before the desktop
  // starts, preventing this checkout from talking to an older installed service.
  "run", "dev:tauri",
];

// ── orchestration ─────────────────────────────────────────────────────────────

console.log("[dev:all] Starting fleet-server (quiet) + desktop development launcher ...");
console.log(`[dev:all] Dev keypair pub: ${DEV_PUB_B64}`);
console.log("[dev:all] Ctrl+C stops both processes.\n");

const fleetProc = spawnWithPrefix("[fleet]", "cargo", fleetArgs, {
  cwd: ROOT,
  env: fleetEnv,
});

// Give the fleet server a moment to bind before tauri dev kicks off its own
// build; the timing isn't critical (the agent will retry), but clean logs help.
setTimeout(() => {
  const tauriProc = spawnWithPrefix("[tauri]", "bun", tauriArgs, {
    cwd: ROOT,
    env: tauriEnv,
  });

  tauriProc.on("exit", (code) => {
    console.log(`\n[dev:all] tauri dev exited (${code ?? "signal"}); stopping fleet-server.`);
    fleetProc.kill("SIGTERM");
    process.exit(code ?? 0);
  });
}, 500);

fleetProc.on("exit", (code, signal) => {
  if (signal !== "SIGTERM") {
    // Unexpected fleet exit — let tauri dev keep running so the developer
    // sees the error rather than a silent kill.
    console.error(`[dev:all] fleet-server exited unexpectedly (code=${code} signal=${signal}).`);
  }
});

// Ctrl+C: kill both children then exit.
function shutdown(): void {
  console.log("\n[dev:all] Shutting down ...");
  fleetProc.kill("SIGTERM");
  process.exit(0);
}
process.on("SIGINT",  shutdown);
process.on("SIGTERM", shutdown);
