import { afterAll, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { stripVTControlCharacters } from "node:util";

const directory = mkdtempSync(join(tmpdir(), "wincommander-beta-pair-"));
const binaryPath = join(directory, "wincommander-pro.exe");
const manifestPath = join(directory, "latest.json");
const receiptPath = join(directory, "release-receipt.json");
const freeCommit = "c".repeat(40);
const freeTree = "d".repeat(40);
const version = "3.6.7";
const domain = "https://updates.example.test";
const script = join(process.cwd(), "tools", "assert-pro-beta-pair.ps1");

afterAll(() => {
  const resolved = realpathSync(directory);
  if (!resolved.startsWith(realpathSync(tmpdir()))) throw new Error("Unsafe fixture cleanup target");
  rmSync(resolved, { recursive: true, force: true });
});

function sha(bytes: Buffer | string) {
  return createHash("sha256").update(bytes).digest("hex");
}

function writeFixture(overrides: Record<string, unknown> = {}) {
  const binary = Buffer.from("synthetic-pro-binary");
  writeFileSync(binaryPath, binary);
  const manifest = JSON.stringify({
    version,
    url: `${domain}/pro/v${version}/wincommander-pro.exe`,
    sha256: sha(binary),
    size: binary.length,
  });
  writeFileSync(manifestPath, manifest);
  writeFileSync(receiptPath, JSON.stringify({
    version,
    pro_commit: "a".repeat(40),
    pro_tree: "b".repeat(40),
    free_commit: freeCommit,
    free_tree: freeTree,
    pro_sha256: sha(binary),
    manifest_sha256: sha(manifest),
    size: binary.length,
    ...overrides,
  }));
}

function verify() {
  return Bun.spawnSync({
    cmd: ["pwsh", "-NoProfile", "-NonInteractive", "-File", script, "-Version", version, "-FreeCommit", freeCommit,
      "-FreeTree", freeTree, "-ManifestPath", manifestPath, "-BinaryPath", binaryPath,
      "-ReceiptPath", receiptPath, "-UpdateDomain", domain],
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
    timeout: 20_000,
    killSignal: "SIGKILL",
  });
}

function normalizeError(stderr: string) {
  return stripVTControlCharacters(stderr).replace(/\r?\n\s*\|\s*/g, " ").replace(/\s+/g, " ");
}

function expectRejected(result: ReturnType<typeof verify>) {
  expect(result.signalCode).toBeUndefined();
  expect(result.exitCode).toBe(1);
  const error = normalizeError(result.stderr.toString());
  expect(error).toContain("Pro release receipt, manifest, and binary do not match the selected Free beta commit.");
}

describe("Free beta requires the exact paired Pro release", () => {
  test("normalizes colored and wrapped PowerShell diagnostics", () => {
    const error = "\u001b[31;1mPro release receipt, manifest, and binary do not match the selected Free\u001b[0m\n\u001b[36;1m | \u001b[31;1mbeta commit.\u001b[0m";
    expect(normalizeError(error)).toBe("Pro release receipt, manifest, and binary do not match the selected Free beta commit.");
  });

  test("accepts the matching commit, manifest, binary, and receipt", () => {
    writeFixture();
    const result = verify();
    expect(result.signalCode).toBeUndefined();
    expect(result.exitCode).toBe(0);
  }, 30_000);

  test("rejects a Pro receipt pinned to another Free commit", () => {
    writeFixture({ free_commit: "e".repeat(40) });
    expectRejected(verify());
  }, 30_000);

  test("rejects changed Pro bytes after the receipt was issued", () => {
    writeFixture();
    writeFileSync(binaryPath, Buffer.from("tampered-pro-binary"));
    expectRejected(verify());
  }, 30_000);
});
