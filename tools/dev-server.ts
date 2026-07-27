// tools/dev-server.ts — parallelized dev-server bootstrap
//
// Replaces the old sequential chain (kill:dev && encrypt-backend && build:pro
// && vite) with real parallelism wherever the dependency graph allows it.
//
// Real dependencies (verified, not assumed):
//   - kill:dev, `bun install`, and encrypt-backend have ZERO dependencies on
//     each other — encrypt.ts and build-pro.ts import only node: builtins +
//     a local module, so neither needs node_modules to run.
//   - vite needs `bun install` done (its own deps) AND kill:dev done (frees
//     port 1420 — vite.config.ts sets strictPort: true, so an occupied port
//     aborts startup outright).
//   - build:pro needs kill:dev done (a stale `wincommander-pro.exe` from a
//     previous session would otherwise hold its own file locked, blocking
//     cargo's rebuild) — but NOTHING downstream needs build:pro's own output
//     at compile time. commander-free's Rust build doesn't statically link
//     the Pro sidecar; it only needs the exe to exist on disk at RUNTIME,
//     when a Pro-gated feature is actually used.
//   - Tauri's `beforeDevCommand` + `devUrl` polling means Tauri starts
//     compiling commander-free (which embeds the .enc files via
//     include_bytes!) the MOMENT vite responds on :1420 — running
//     concurrently with this script's own vite process, not waiting for it
//     to exit. So encrypt-backend MUST be fully finished before vite starts
//     (else Tauri's Rust compile could embed stale/partial .enc bytes), even
//     though encrypt-backend and vite have no direct data dependency.
//
// Net effect: kill→encrypt→build:pro→vite (4 sequential steps) becomes
// max(kill, install, encrypt) → vite, with build:pro finishing in the
// background whenever it finishes — normally well before the first
// Pro-gated click, and non-fatal to the dev session if it isn't.
//
// Usage: bun run tools/dev-server.ts [--free]
//   --free: skip build:pro entirely (matches the old dev:free script, which
//           never built the Pro sidecar at all).

import { spawn } from "node:child_process";
import { resolve } from "node:path";
import type { ChildProcess } from "node:child_process";

const ROOT = resolve(import.meta.dir, "..");
const FREE_ONLY = process.argv.includes("--free");

function run(tag: string, cmd: string, args: string[]): Promise<number> {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cmd, args, { cwd: ROOT, env: process.env, stdio: "inherit", shell: false });
    child.on("error", reject);
    child.on("exit", (code) => resolvePromise(code ?? 1));
  });
}

async function main(): Promise<void> {
  console.log("[dev-server] kill:dev, bun install, encrypt-backend running in parallel...");

  const steps: Array<{ name: string; promise: Promise<number> }> = [
    { name: "kill:dev", promise: run("[kill]", "powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "tools/kill-dev.ps1"]) },
    { name: "bun install", promise: run("[install]", "bun", ["install", "--frozen-lockfile"]) },
    { name: "encrypt-backend", promise: run("[encrypt]", "bun", ["run", "tools/encrypt.ts"]) },
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
      ? "[dev-server] kill:dev/install/encrypt done — starting vite..."
      : "[dev-server] kill:dev/install/encrypt done — starting build:pro in background + vite in foreground...",
  );

  let buildPro: ChildProcess | null = null;
  if (!FREE_ONLY) {
    // Background, non-blocking: vite/Tauri's compile step don't need this at
    // compile time, only at runtime when a Pro-gated feature is exercised —
    // so a slow or failed Pro build must never hold up frontend dev.
    buildPro = spawn("bun", ["run", "tools/build-pro.ts"], { cwd: ROOT, env: process.env, stdio: "inherit", shell: false });
    buildPro.on("exit", (code) => {
      if (code !== 0) {
        console.error(
          `[dev-server] build:pro failed (exit ${code}) — Pro sidecar features won't work until this succeeds; re-run "bun run build:pro" manually.`,
        );
      } else {
        console.log("[dev-server] build:pro finished.");
      }
    });
  }

  // vite is long-running/foreground — its exit code becomes this script's.
  const vite = spawn("bun", ["x", "vite"], { cwd: ROOT, env: process.env, stdio: "inherit", shell: false });

  const shutdown = (signal: NodeJS.Signals): void => {
    vite.kill(signal);
    buildPro?.kill(signal);
  };
  process.on("SIGINT", () => shutdown("SIGINT"));
  process.on("SIGTERM", () => shutdown("SIGTERM"));

  vite.on("exit", (code) => {
    buildPro?.kill();
    process.exit(code ?? 0);
  });
}

main().catch((err) => {
  console.error("[dev-server] fatal:", err);
  process.exit(1);
});
