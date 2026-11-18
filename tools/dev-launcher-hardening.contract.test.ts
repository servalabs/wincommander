import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";

const cargoConfig = readFileSync("src-tauri/.cargo/config.toml", "utf8");
const devServer = readFileSync("tools/dev-server.ts", "utf8");
const devLauncher = readFileSync("tools/dev.ps1", "utf8");

describe("development launcher hardening", () => {
  test("cargo finds a native Perl for vendored OpenSSL without caller setup", () => {
    expect(cargoConfig).toMatch(/\[env\]/);
    expect(cargoConfig).toMatch(
      /OPENSSL_SRC_PERL\s*=\s*\{\s*value\s*=\s*"\.\.\/tools\/dev-perl-shim\.cmd",\s*relative\s*=\s*true,\s*force\s*=\s*false\s*\}/,
    );
    expect(existsSync("tools/dev-perl-shim.cmd")).toBe(true);
  });

  test("the Perl shim is a CRLF batch file that never falls back to Git's MSYS Perl", () => {
    const shim = readFileSync("tools/dev-perl-shim.cmd", "utf8");
    expect(shim.includes("\r\n")).toBe(true);
    expect(shim.replace(/\r\n/g, "")).not.toContain("\n");
    expect(shim).toContain("strawberry-perl-5.40.5.1");
    expect(shim).toContain("Ensure-DevPerl");
    expect(shim.toLowerCase()).not.toContain("git\\usr\\bin");
    // `timeout` rejects redirected stdin, which Cargo build scripts always have.
    expect(shim).not.toMatch(/^\s*timeout\s/m);
  });

  test("an unverified Vault state is retried and never treated as free", () => {
    expect(devServer).toContain("inspectVaultStateWithRetry");
    expect(devServer).toMatch(/VAULT_INSPECTION_ATTEMPTS\s*=\s*3/);
    expect(devServer).toMatch(/VAULT_INSPECTION_TIMEOUT_MS\s*=\s*45_000/);
    expect(devServer).toContain('vaultState?.state !== "free"');
  });

  test("a plain tauri dev starts Vite before the long native preparation", () => {
    const tauriConfig = readFileSync("src-tauri/commander-free/tauri.conf.json", "utf8");
    // The raw hook installs missing tools first (setup:dev), then must reach Vite
    // inside Tauri's fixed frontend wait.
    expect(tauriConfig).toMatch(/"beforeDevCommand":\s*"[^"]*bun run setup:dev[^"]*dev:server --precleaned --vite-first"/);
    expect(devServer).toContain('process.argv.includes("--vite-first")');
    // Vite starts before the Pro build/service sync, never frees its own port,
    // and is stopped if preparation fails.
    expect(devServer.indexOf("earlyVite = startVite()")).toBeGreaterThan(0);
    expect(devServer.indexOf("earlyVite = startVite()")).toBeLessThan(devServer.indexOf("tools/build-pro.ts"));
    expect(devServer).toContain("policy.releasePort && !earlyVite");
    expect(devServer).toContain("failAfterStoppingVite(buildProResult)");
    // The supported prepared launcher keeps its own vite-only hook.
    expect(readFileSync("src-tauri/commander-free/tauri.prepared-dev.conf.json", "utf8")).toContain("--vite-only");
  });

  test("a fresh checkout builds in debug without generated .enc release artifacts", () => {
    const buildRs = readFileSync("src-tauri/commander-free/build.rs", "utf8");
    const call = buildRs.indexOf("ensure_debug_resource_placeholders();");
    expect(call).toBeGreaterThan(0);
    // Only the debug branch (after `} else {` of the release check) may create placeholders.
    const releaseBranch = buildRs.indexOf("if !is_development_profile {");
    expect(releaseBranch).toBeGreaterThan(0);
    expect(call).toBeGreaterThan(buildRs.indexOf("} else {", releaseBranch));
    expect(buildRs).toContain('"scripts/core/dev-placeholder.enc"');
    expect(buildRs).toContain('"scripts/modules/dev-placeholder.enc"');
  });

  test("the WebView2 debug port is opt-in, validated and loopback only", () => {
    expect(devLauncher).toContain("WINCOMMANDER_DEV_DEBUG_PORT");
    expect(devLauncher).toContain("--remote-debugging-port=$debugPort");
    expect(devLauncher).toContain("http://127.0.0.1:$debugPort");
    expect(devLauncher).toContain("must be a TCP port between 1000 and 65535");
    expect(devLauncher).not.toContain("0.0.0.0");
  });
});
