import { afterAll, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

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
    cmd: ["pwsh", "-NoProfile", "-File", script, "-Version", version, "-FreeCommit", freeCommit,
      "-FreeTree", freeTree, "-ManifestPath", manifestPath, "-BinaryPath", binaryPath,
      "-ReceiptPath", receiptPath, "-UpdateDomain", domain],
    stdout: "pipe",
    stderr: "pipe",
  });
}

describe("Free beta requires the exact paired Pro release", () => {
  test("accepts the matching commit, manifest, binary, and receipt", () => {
    writeFixture();
    expect(verify().exitCode).toBe(0);
  });

  test("rejects a Pro receipt pinned to another Free commit", () => {
    writeFixture({ free_commit: "e".repeat(40) });
    expect(verify().exitCode).not.toBe(0);
  });

  test("rejects changed Pro bytes after the receipt was issued", () => {
    writeFixture();
    writeFileSync(binaryPath, Buffer.from("tampered-pro-binary"));
    expect(verify().exitCode).not.toBe(0);
  });
});
