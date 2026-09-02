import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dir, "..");
const read = (path: string) => readFileSync(resolve(root, path), "utf8");

describe("autonomous-test build boundary", () => {
  test("keeps the typed adapter out of the generic catalog and release route", () => {
    const main = read("src-tauri/commander-free/src/main.rs");
    const lib = read("src-tauri/commander-free/src/lib.rs");
    const cli = read("src-tauri/commander-free/src/cli.rs");
    const catalog = read("src-tauri/commander-free/src/cli_catalog.generated.json");

    expect(main).toContain('#[cfg(all(feature = "autonomous-test", debug_assertions))]');
    expect(lib).toContain('pub mod autonomous_test;');
    expect(cli).not.toContain("agent-test");
    expect(catalog).not.toContain("agent-test");
  });

  test("refuses autonomous-test in a release Cargo profile", () => {
    const manifest = read("src-tauri/commander-free/Cargo.toml");
    const build = read("src-tauri/commander-free/build.rs");

    expect(manifest).toContain('autonomous-test = ["dep:fleet-proto", "fleet-proto/autonomous-test"]');
    expect(build).toContain("CARGO_FEATURE_AUTONOMOUS_TEST");
    expect(build).toContain("restricted to debug test artifacts");
  });
});
