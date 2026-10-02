import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { assertSharedAssets, REQUIRED_SHARED_ASSETS } from "./check-shared-assets";

const fixtures: string[] = [];
function fixture(omit?: string): string {
  const root = mkdtempSync(join(tmpdir(), "wincommander-assets-test-"));
  fixtures.push(root);
  for (const relative of REQUIRED_SHARED_ASSETS) {
    if (relative === omit) continue;
    const path = join(root, "assets", relative);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, "fixture");
  }
  return root;
}

afterEach(() => {
  for (const root of fixtures.splice(0)) rmSync(root, { recursive: true, force: true });
});

test("accepts complete pinned assets in a source archive without Git metadata", () => {
  expect(() => assertSharedAssets(fixture())).not.toThrow();
});

test("missing Risk Matrix reports the pinned submodule repair command", () => {
  expect(() => assertSharedAssets(fixture("components/risk-matrix/index.ts")))
    .toThrow("git submodule update --init --recursive -- assets");
});

test("rejects a directory in place of the Risk Matrix module", () => {
  const root = fixture("components/risk-matrix/index.ts");
  mkdirSync(join(root, "assets/components/risk-matrix/index.ts"));
  expect(() => assertSharedAssets(root)).toThrow("components/risk-matrix/index.ts");
});

test("rejects missing transitive shared media", () => {
  expect(() => assertSharedAssets(fixture("entities/rsa-logo.svg")))
    .toThrow("entities/rsa-logo.svg");
});
