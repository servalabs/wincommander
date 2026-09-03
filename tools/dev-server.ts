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

import { spawn, spawnSync } from "node:child_process";
import { resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const FREE_ONLY = process.argv.includes("--free");
const MULTI_USER = process.argv.includes("--multi-user");
// Tauri runs this script as its beforeDevCommand.  It must release only stale
// processes, not the desktop process whose startup is waiting for this server.
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

async function main(): Promise<void> {
  console.log("[dev-server] kill:dev and bun install running in parallel...");

  const steps: Array<{ name: string; promise: Promise<number> }> = [
    {
      name: "kill:dev",
      promise: run("[kill]", "powershell", [
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "tools/kill-dev.ps1",
        ...(PRESERVE_WINCOMMANDER ? ["-PreserveWinCommander"] : []),
      ]),
    },
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
    console.log("[dev-server] build:pro finished — starting vite.");
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
