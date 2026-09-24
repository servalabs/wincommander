// tools/dev-server.ts — parallelized dev-server bootstrap
//
// Replaces the old sequential chain (kill:dev && encrypt-backend && build:pro
// && vite) with real parallelism wherever the dependency graph allows it.
//
// Real dependencies (verified, not assumed):
//   - kill:dev and `bun install` have no dependency on each other.
//   - vite needs `bun install` done (its own deps) AND kill:dev done (frees
//     port 1420 — vite.config.ts sets strictPort: true, so an occupied port
//     aborts startup outright).
//   - build:pro needs kill:dev done (a stale `wincommander-pro.exe` from a
//     previous session would otherwise hold its own file locked, blocking
//     cargo's rebuild) — but NOTHING downstream needs build:pro's own output
//     at compile time. commander-free's Rust build doesn't statically link
//     the Pro sidecar; it only needs the exe to exist on disk at RUNTIME,
//     when a Pro-gated feature is actually used.
//   - Debug commander-free builds embed the plaintext source modules directly.
//     Encryption is an explicit release-preparation step, so an ordinary dev
//     launch never rewrites the ignored salt and ciphertext tree.
//
// Once the independent preparation steps finish, build Pro before exposing
// Vite's dev URL. Tauri starts the desktop app as soon as that URL responds;
// starting Vite first lets the old sidecar start and lock the exact .exe Cargo
// is trying to replace, which makes the Pro build fail on Windows.
//
// Usage: bun run tools/dev-server.ts [--free]
//   --free: skip build:pro entirely (matches the old dev:free script, which
//           never built the Pro sidecar at all).
//   WINCOMMANDER_DEV_FREE_ONLY=1: selected by tools/dev.ps1 when a public
//           checkout has no private Pro sibling. This lets `bun run dev`
//           start the Free desktop instead of failing before Vite starts.

import { spawn, spawnSync } from "node:child_process";
import { resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
// The Codex/runtime environment can place a PowerShell-compatible shim ahead
// of Windows PowerShell on PATH.  The service synchronizer needs Windows'
// built-in utility module (notably Get-FileHash), so make that boundary
// explicit instead of relying on PATH ordering.
const WINDOWS_POWERSHELL = process.platform === "win32"
  ? `${process.env.SystemRoot ?? "C:\\Windows"}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe`
  : "powershell";
const FREE_ONLY = process.argv.includes("--free") || process.env.WINCOMMANDER_DEV_FREE_ONLY === "1";
const MULTI_USER = process.argv.includes("--multi-user");
// Tauri starts its Rust dev command alongside beforeDevCommand. The Tauri
// config therefore runs kill:dev before its slower setup steps, then passes
// this flag so this later server bootstrap does not kill the newly built app.
const PRE_CLEANED = process.argv.includes("--precleaned");
// Multi-user development intentionally shares a Vite server; refuse a second
// multi-user bootstrap if a desktop already owns that server.
const PRESERVE_WINCOMMANDER = MULTI_USER || process.argv.includes("--preserve-wincommander");

function run(tag: string, cmd: string, args: string[]): Promise<number> {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cmd, args, { cwd: ROOT, env: process.env, stdio: "inherit", shell: false });
    child.on("error", reject);
    child.on("exit", (code) => resolvePromise(code ?? 1));
  });
}

/**
 * A Pro build can take long enough for another dev invocation to claim Vite's
 * strict port after the initial kill step. Release only that listener again
 * immediately before launching Vite; do not kill the freshly built sidecar.
 */
function freeVitePort(): void {
  const netstat = spawnSync("netstat", ["-ano"], { cwd: ROOT, encoding: "utf8", shell: false });
  if (netstat.status !== 0) {
    throw new Error(`could not inspect Vite port 1420 (exit ${netstat.status ?? "unknown"})`);
  }
  const listener = netstat.stdout
    .split(/\r?\n/)
    .map((line) => line.match(/:1420\s+\S+\s+LISTENING\s+(\d+)\s*$/i))
    .find((match): match is RegExpMatchArray => match !== null);
  if (listener) {
    const result = spawnSync("taskkill", ["/PID", listener[1], "/F"], {
      cwd: ROOT,
      stdio: "inherit",
      shell: false,
    });
    if (result.status !== 0) {
      throw new Error(`could not release Vite port 1420 (exit ${result.status ?? "unknown"})`);
    }
  }
}

function activeVitePortOwner(): string | null {
  const netstat = spawnSync("netstat", ["-ano"], { cwd: ROOT, encoding: "utf8", shell: false });
  if (netstat.status !== 0) return null;
  return netstat.stdout
    .split(/\r?\n/)
    .map((line) => line.match(/:1420\s+\S+\s+LISTENING\s+(\d+)\s*$/i))
    .find((match): match is RegExpMatchArray => match !== null)?.[1] ?? null;
}

function desktopDevWindowIsRunning(): boolean {
  if (process.platform !== "win32") return false;
  const tasklist = spawnSync("tasklist", ["/FI", "IMAGENAME eq wincommander-free.exe", "/NH"], {
    cwd: ROOT,
    encoding: "utf8",
    shell: false,
  });
  return tasklist.status === 0 && /wincommander-free\.exe/i.test(tasklist.stdout);
}

async function main(): Promise<void> {
  // Keep this guard for the shared multi-user server. Normal Tauri startup
  // clears stale processes synchronously at the start of beforeDevCommand and
  // must not classify its own newly launched app as an existing session here.
  const existingViteOwner = activeVitePortOwner();
  if (PRESERVE_WINCOMMANDER && existingViteOwner && desktopDevWindowIsRunning()) {
    throw new Error(
      `an existing WinCommander development session is already using port 1420 (PID ${existingViteOwner}). ` +
      "Close that session or run `bun run kill:dev` before starting a fresh one.",
    );
  }

  console.log(PRE_CLEANED
    ? "[dev-server] prior dev processes already cleared; installing dependencies..."
    : "[dev-server] kill:dev and bun install running in parallel...");

  const steps: Array<{ name: string; promise: Promise<number> }> = [
    ...(!PRE_CLEANED ? [{
      name: "kill:dev",
      promise: run("[kill]", WINDOWS_POWERSHELL, [
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "tools/kill-dev.ps1",
      ]),
    }] : []),
    { name: "bun install", promise: run("[install]", "bun", ["install", "--frozen-lockfile"]) },
  ];

  const results = await Promise.all(steps.map((s) => s.promise));
  for (let i = 0; i < steps.length; i++) {
    if (results[i] !== 0) {
      console.error(`[dev-server] ${steps[i].name} failed (exit ${results[i]}).`);
      process.exit(results[i]);
    }
  }

  console.log(
    FREE_ONLY
      ? "[dev-server] kill:dev/install done — starting vite..."
      : "[dev-server] kill:dev/install done — building Pro before starting vite...",
  );

  if (!FREE_ONLY) {
    const buildProResult = await run("[build:pro]", "bun", ["run", "tools/build-pro.ts"]);
    if (buildProResult !== 0) {
      console.error(`[dev-server] build:pro failed (exit ${buildProResult}).`);
      process.exit(buildProResult);
    }
    const serviceResult = await run("[service]", WINDOWS_POWERSHELL, [
      "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
      "tools/sync-dev-service.ps1", "-SyncPro",
    ]);
    if (serviceResult !== 0) {
      throw new Error(`development service synchronization failed (exit ${serviceResult}); Vite was not started`);
    }
    console.log("[dev-server] current Pro and SYSTEM service verified — starting vite.");
  }

  freeVitePort();

  // vite is long-running/foreground — its exit code becomes this script's.
  const vite = spawn("bun", ["x", "vite"], { cwd: ROOT, env: process.env, stdio: "inherit", shell: false });

  const shutdown = (signal: NodeJS.Signals): void => {
    vite.kill(signal);
  };
  process.on("SIGINT", () => shutdown("SIGINT"));
  process.on("SIGTERM", () => shutdown("SIGTERM"));

  vite.on("exit", (code) => {
    process.exit(code ?? 0);
  });
}

main().catch((err) => {
  console.error("[dev-server] fatal:", err);
  process.exit(1);
});
