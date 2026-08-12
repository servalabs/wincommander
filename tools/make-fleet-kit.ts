// ════════════════════════════════════════════════════════════════════════
// make-fleet-kit.ts — build a copyable, self-contained fleet TEST KIT.
//
//   bun run tools/make-fleet-kit.ts
//
// Produces ./fleet-kit/ containing all three release binaries plus a server
// launcher + README, so you can copy the folder to each test machine and
// exercise the fleet features WITHOUT the dev server:
//
//   fleet-kit/
//     wincommander-free.exe     ← the app + admin panel + agent spawner
//     wincommander-pro.exe      ← Pro sidecar (fleet agent lives here)
//     fleet-server.exe          ← the self-host fleet server (run on ONE host)
//     start-fleet-server.ps1    ← launches the server (0.0.0.0:8787, signing key)
//     README.txt                ← 3-step per-machine flow + the keypair
//
// Why this works portably: wincommander-free.exe embeds the frontend +
// encrypted PS modules at compile time and locates wincommander-pro.exe NEXT TO
// itself (sidecar.rs), so the two exes side-by-side are self-contained. The
// fleet server is a separate binary the host runs. First run starts the 16-day
// trial, unlocking Pro/fleet.
// ════════════════════════════════════════════════════════════════════════

import { execSync } from "node:child_process";
import { generateKeyPairSync } from "node:crypto";
import { mkdirSync, copyFileSync, writeFileSync, existsSync, rmSync, readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { findProWorkspace, getProManifestPath } from "./pro-workspace";

const ROOT = resolve(import.meta.dir, "..");        // D:\GitHub\commander
const SRC_TAURI = resolve(ROOT, "src-tauri");
const PRO_WORKSPACE = findProWorkspace(ROOT);
const PRO_MANIFEST = getProManifestPath(ROOT);
const OUT = resolve(ROOT, "fleet-kit");

// Default = DEBUG (fast, dev-unlock baked in → Pro/fleet unlocked for testing with
// no trial). Pass --release for a slow LTO build suitable for distribution.
const RELEASE = process.argv.includes("--release");
const PROFILE = RELEASE ? "release" : "debug";
const CARGO = RELEASE ? "--release " : "";
// --assemble-only: skip the (slow) build and just (re)package binaries that are
// already built in target/. Use when the build succeeded but the final copy
// failed (e.g. EACCES because the previous kit was still open/locked).
const SKIP_BUILD = process.argv.includes("--assemble-only");

// The Pro build redirects its target into src-tauri/target (matches build:pro);
// the Free build lands there too; fleet-server uses the commander-pro workspace target.
const FREE_EXE = resolve(SRC_TAURI, "target", PROFILE, "wincommander-free.exe");
const PRO_EXE = resolve(SRC_TAURI, "target", PROFILE, "wincommander-pro.exe");
const FLEET_EXE = resolve(PRO_WORKSPACE, "target", PROFILE, "fleet-server.exe");

// ── Load .env so cargo inherits WINCMD_LICENSE_API_BASE etc. at build time ──
// Bun auto-loads .env into process.env when running scripts via `bun run`,
// but when execSync spawns a child cargo process we pass env explicitly.
// Parse the .env file manually so the vars reach the cargo build subprocess.
const envOverrides: Record<string, string> = {};
const dotEnvPath = resolve(ROOT, ".env");
if (existsSync(dotEnvPath)) {
  for (const line of readFileSync(dotEnvPath, "utf8").split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq < 1) continue;
    const key = trimmed.slice(0, eq).trim();
    const val = trimmed.slice(eq + 1).trim().replace(/^["']|["']$/g, "");
    if (key && val && !val.startsWith("<")) envOverrides[key] = val;
  }
}

// Pre-flight: release builds bake licensing at compile time.
// Debug builds skip this entirely (dev-unlock covers all features).
if (RELEASE && !SKIP_BUILD) {
  const apiBase = envOverrides["WINCMD_LICENSE_API_BASE"] ?? process.env["WINCMD_LICENSE_API_BASE"] ?? "";
  const pubKey  = envOverrides["WINCMD_LICENSE_PUBLIC_KEY"] ?? process.env["WINCMD_LICENSE_PUBLIC_KEY"] ?? "";
  if (!apiBase || !pubKey) {
    console.error(`
❌  Release build requires licensing to be configured at compile time.

    The app will show "Licensing is not configured" at runtime without these.

    Fix — create a .env file at the repo root with:
      WINCMD_LICENSE_API_BASE=https://your-worker.workers.dev
      WINCMD_LICENSE_PUBLIC_KEY=<ed25519-public-key-base64>

    See .env.example for the full template.

    For fleet TESTING (no license worker needed), omit --release:
      bun run tools/make-fleet-kit.ts
    Debug builds have dev-unlock on — all Pro/fleet features work immediately.
`);
    process.exit(1);
  }
}

const env = { ...process.env, ...envOverrides, RUST_MIN_STACK: "16777216" };
function run(cmd: string, cwd: string) {
  console.log(`\n▶ ${cmd}\n   (cwd: ${cwd})`);
  execSync(cmd, { cwd, env, stdio: "inherit" });
}

// ── 1. Generate a signing keypair so command dispatch works out of the box ──
// Server gets the hex seed (FLEET_SIGNING_KEY_HEX); agents paste the base64
// public key into the Connect screen's "Signing public key" field.
const { privateKey, publicKey } = generateKeyPairSync("ed25519");
const seedHex = Buffer.from(privateKey.export({ type: "pkcs8", format: "der" })).subarray(-32).toString("hex");
const pubB64 = Buffer.from(publicKey.export({ type: "spki", format: "der" })).subarray(-32).toString("base64");

// ── 2. Build all three binaries ───────────────────────────────────────────
if (SKIP_BUILD) {
  console.log(`Skipping build (--assemble-only); packaging existing ${PROFILE} binaries.`);
} else {
  console.log(`Building ${PROFILE} binaries${RELEASE ? " (release LTO — slow)" : " (debug — fast; dev-unlock on)"}…`);
run("bun run encrypt-backend", ROOT);                                   // refresh embedded .enc modules
// Build Pro first; for a release build, hash-pro then pins its SHA-256 into Free so
// the sidecar handshake accepts it (debug builds tolerate the sidecar without it).
run(`cargo build -p commander-pro ${CARGO}--manifest-path "${PRO_MANIFEST}" --target-dir "${resolve(SRC_TAURI, "target")}"`, ROOT);
if (RELEASE) run("bun run hash-pro", ROOT);
run("bun x vite build", ROOT);                                          // frontend dist (embedded in Free)
// `tauri/custom-protocol` flips Tauri out of dev mode (tauri/build.rs: dev = !custom-protocol).
// Without it a raw `cargo build` — even --release — loads the Vite dev server (http://[::1]:1420)
// instead of the embedded frontend, so the copied exe shows "can't reach this page". `tauri build`
// sets this automatically; we build via raw cargo here, so we must pass it explicitly.
run(`cargo build -p commander-free ${CARGO}--features tauri/custom-protocol`, SRC_TAURI);  // Free exe (embeds frontend + .enc; finds Pro next to it)
run(`cargo build -p fleet-server ${CARGO}--manifest-path "${PRO_MANIFEST}"`, ROOT);
}

// ── 3. Assemble the kit folder ───────────────────────────────────────────
for (const [label, p] of [["free", FREE_EXE], ["pro", PRO_EXE], ["fleet-server", FLEET_EXE]] as const) {
  if (!existsSync(p)) throw new Error(`expected ${label} binary missing: ${p}`);
}
// Clear contents without deleting the parent dir — avoids EBUSY on Windows
// when Explorer or antivirus has the folder itself open.
if (existsSync(OUT)) {
  for (const entry of readdirSync(OUT)) {
    rmSync(resolve(OUT, entry), { recursive: true, force: true });
  }
} else {
  mkdirSync(OUT, { recursive: true });
}
copyFileSync(FREE_EXE, resolve(OUT, "wincommander-free.exe"));
copyFileSync(PRO_EXE, resolve(OUT, "wincommander-pro.exe"));
copyFileSync(FLEET_EXE, resolve(OUT, "fleet-server.exe"));

// Server launcher — in-memory store, binds all interfaces (tailnet-reachable),
// fixed signing key so agents can pin it + command dispatch verifies.
writeFileSync(resolve(OUT, "start-fleet-server.ps1"), `# WinCommander fleet test server (in-memory; data resets on restart).
$env:FLEET_BIND_ADDR = "0.0.0.0:8787"   # all interfaces (Tailscale-reachable)
$env:FLEET_SIGNING_KEY_HEX = "${seedHex}"
$env:FLEET_ADMIN_EMAIL = "admin@local"       # release builds don't auto-seed an admin; bootstrap one here
$env:FLEET_ADMIN_PASSWORD = "admin"
$env:RUST_LOG = "warn,fleet_server=info"   # quiet: server lines only, no dep spam
Write-Host "Fleet server starting on 0.0.0.0:8787 -- admin: admin@local / admin"
Write-Host "On THIS machine connect to http://127.0.0.1:8787 (NOT 'localhost')."
& "$PSScriptRoot\\fleet-server.exe"
`);

// One-command launcher: starts the fleet-server as a background job then
// launches wincommander-free.exe so the agent auto-enrolls.  ASCII-only
// (no em-dash, no smart quotes, no bullets) — PowerShell 5.1 mis-parses
// UTF-8 special characters when the file is saved as UTF-8-with-BOM and
// the console codepage does not match.
writeFileSync(resolve(OUT, "start-all.ps1"), `# WinCommander -- one-command dev/test launcher.
# Starts fleet-server.exe hidden, then launches the app detached from PowerShell.
# The app auto-enrolls to the local server via env vars seeded below.
#
# NOTE: use 127.0.0.1, NOT localhost -- on Windows localhost may resolve
# to IPv6 (::1) while the server binds IPv4 only.

$signingKeyHex = "${seedHex}"
$signingKeyPub = "${pubB64}"

$env:FLEET_BIND_ADDR       = "0.0.0.0:8787"
$env:FLEET_SIGNING_KEY_HEX = $signingKeyHex
$env:FLEET_ADMIN_EMAIL     = "admin@local"   # release builds don't auto-seed an admin
$env:FLEET_ADMIN_PASSWORD  = "admin"
$env:RUST_LOG              = "warn"
Start-Process -FilePath "$PSScriptRoot\\fleet-server.exe" -WorkingDirectory $PSScriptRoot -WindowStyle Hidden
Start-Sleep -Seconds 2

$env:FLEET_URL              = "http://127.0.0.1:8787"
$env:FLEET_DISPATCH         = "1"
$env:FLEET_SIGNING_KEY_PUB  = $signingKeyPub
Start-Process -FilePath "$PSScriptRoot\\wincommander-free.exe" -WorkingDirectory $PSScriptRoot
`, "ascii");

writeFileSync(resolve(OUT, "README.txt"), `WinCommander -- Fleet Test Kit
==============================
Four files. Copy this whole folder to each test machine.

QUICK START (single machine -- server + app in one click)
  Right-click start-all.ps1 -> Run with PowerShell.
  It starts the fleet server hidden then launches the app.
  The app auto-enrolls to the local server -- no manual URL entry needed.

MULTI-MACHINE SETUP
  ON THE HOST MACHINE (one machine runs the server):
    Right-click start-fleet-server.ps1 -> Run with PowerShell (keep open).
    - In-memory store (data resets on restart); seeds admin@local / admin
    - Listens on 0.0.0.0:8787 -- reachable at this machine's Tailscale IP.
    Find this machine's Tailscale IP: tailscale ip -4  (e.g. 100.x.y.z)

  ON EACH CLIENT MACHINE (including the host):
    Run wincommander-free.exe (auto-launches wincommander-pro.exe next to it).
    First run starts a 16-day trial -- Pro + fleet features unlock.
    App -> Fleet panel -> "Enroll this device":
      Fleet Server URL: http://<HOST-TAILSCALE-IP>:8787
        (on the host itself use http://127.0.0.1:8787 -- NOT localhost)
      Toggle "Enable command dispatch" and paste the signing public key:
        ${pubB64}
      Click Connect. The device appears in the admin console.
    "Admin console" tab -> same URL -> admin@local / admin

NOTES
  Use 127.0.0.1, NOT "localhost" -- on Windows, localhost may resolve to
  IPv6 (::1) while the server binds IPv4, causing connection refused.
  In-memory server: enrollments reset when the server restarts. Set
  DATABASE_URL to a Postgres instance for persistence.
  Build profile: ${PROFILE}.${RELEASE ? "" : "  Debug -- Pro/fleet unlocked via dev-unlock (no trial needed); larger exes; for TESTING only (rebuild with --release for distribution)."}
  Unsigned local build -- Windows SmartScreen may warn; More info -> Run anyway.
  Signing keypair for THIS kit (regenerated each build):
    server seed  (FLEET_SIGNING_KEY_HEX): ${seedHex}
    agent pubkey (paste in Connect screen): ${pubB64}
`);

console.log(`\n✅ Fleet test kit ready: ${OUT}`);
console.log(`   Signing pubkey (paste in the agent Connect screen): ${pubB64}`);
