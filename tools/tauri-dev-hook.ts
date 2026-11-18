import { spawn, spawnSync, type SpawnSyncReturns } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { vitePortOwner } from "./dev-startup-policy";
import { getOverrideWorkspacePath, getProWorkspaceCandidates } from "./pro-workspace";

const root = resolve(import.meta.dirname, "..");
const hook = resolve(import.meta.dirname, "tauri-dev-hook.ts");
const windowsPowerShell = resolve(
  process.env.SystemRoot ?? "C:\\Windows", "System32/WindowsPowerShell/v1.0/powershell.exe",
);

function requireSuccess(step: string, result: SpawnSyncReturns<unknown>): void {
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${step} failed (exit ${result.status ?? "unknown"}).`);
}

function runBun(args: string[]): void {
  requireSuccess(args.join(" "), spawnSync(process.execPath, args, { cwd: root, stdio: "inherit" }));
}

function tauriOwnerPid(): number {
  // On Windows Tauri runs the hook as a direct command inside cmd.exe.
  const command = `(Get-CimInstance Win32_Process -Filter 'ProcessId=${process.ppid} AND Name="cmd.exe"').ParentProcessId`;
  const result = spawnSync(windowsPowerShell, ["-NoProfile", "-NonInteractive", "-Command", command], { encoding: "utf8" });
  requireSuccess("Tauri owner lookup", result);
  const owner = Number(result.stdout.trim());
  if (!Number.isSafeInteger(owner) || owner <= 0) throw new Error("Could not determine the Tauri process that owns this hook.");
  return owner;
}

async function startOwnedVite(owner: number): Promise<void> {
  const child = spawn(process.execPath, [hook, "--serve", String(owner)], {
    cwd: root,
    detached: true,
    windowsHide: true,
    stdio: ["ignore", "inherit", "inherit", "ipc"],
  });
  await new Promise<void>((resolveReady, reject) => {
    child.once("error", reject);
    child.once("exit", code => reject(new Error(`Vite exited before listening (exit ${code ?? "unknown"}).`)));
    child.once("message", message => {
      if (message === "ready") resolveReady();
      else reject(new Error("Vite did not report readiness."));
    });
  });
  child.disconnect();
  child.unref();
}

async function serve(owner: number): Promise<void> {
  // The blocking Tauri hook must exit once Vite is ready. Stop Vite when
  // that same CLI exits, including when Windows terminates it directly.
  const checkOwner = (): void => {
    try {
      process.kill(owner, 0);
    } catch {
      process.exit(0);
    }
  };
  checkOwner();
  setInterval(checkOwner, 1_000).unref();
  const { createServer } = await import("vite");
  const vite = await createServer({ root, configLoader: "native" });
  await vite.listen();
  vite.printUrls();
  process.send?.("ready");
}

async function main(): Promise<void> {
  if (process.argv[2] === "--serve") {
    const owner = Number(process.argv[3]);
    if (!process.send || !Number.isSafeInteger(owner) || owner <= 0) throw new Error("Missing Vite owner.");
    await serve(owner);
    return;
  }

  const listener = spawnSync("netstat", ["-ano", "-p", "tcp"], { encoding: "utf8" });
  if (vitePortOwner(listener.stdout ?? "", listener.status, true)) {
    throw new Error("Port 1420 is already in use. Close that dev session before starting another.");
  }
  const owner = tauriOwnerPid();
  const freeOnly = process.env.WINCOMMANDER_DEV_FREE_ONLY === "1"
    || (!getOverrideWorkspacePath(root, process.env)
      && !getProWorkspaceCandidates(root).some(path => existsSync(resolve(path, "Cargo.toml"))));

  runBun(["run", "setup:dev"]);
  runBun(["run", "tools/dev-server.ts", "--prepare-only", "--keep-frontend", ...(freeOnly ? ["--free"] : [])]);
  await startOwnedVite(owner);
}

main().catch((error: unknown) => {
  console.error("[tauri-dev]", error instanceof Error ? error.message : error);
  process.exitCode = 1;
});
