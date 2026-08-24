import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import catalog from "../src-tauri/commander-free/src/cli_catalog.generated.json";
import { getRadarDriftToggles } from "../src/registry";

type Entry = (typeof catalog.commands)[number];

const byId = new Map(catalog.commands.map((entry) => [entry.id, entry]));

function commandTotals(entries: readonly Entry[]) {
  const tauri = entries.filter((entry) => entry.transport === "tauri");
  const backend = entries.filter((entry) => entry.transport === "backend-script");
  const releaseExecutable = entries.filter((entry) => entry.registered && !entry.debugOnly);
  return { total: entries.length, tauri: tauri.length, backend: backend.length, releaseExecutable: releaseExecutable.length };
}

function grouped(value: number) {
  return value.toLocaleString("en-US");
}

describe("generated WinCommander CLI catalog", () => {
  test("covers the real Tauri and backend command surfaces", () => {
    expect(catalog.schemaVersion).toBe(2);
    expect(catalog.commands.length).toBeGreaterThan(400);
    expect(byId.get("tauri:get_settings")?.registered).toBe(true);
    expect(byId.get("tauri:configure_wifi_guard")?.registered).toBe(true);
    expect(byId.get("tauri:get_wifi_guard_baseline")?.registered).toBe(true);
    expect(byId.get("tauri:run_backend_script")?.registered).toBe(true);
    expect(byId.get("backend:Get-SystemInfo")?.registered).toBe(true);
    expect(byId.get("backend:Get-ShellBags")?.registered).toBe(true);
    expect(byId.get("backend:Clear-MFTResidentSlack")?.registered).toBe(true);
    expect(byId.has("backend:size-descending")).toBe(false);
    expect(byId.has("backend:lockdown-step")).toBe(false);
  });

  // docs/cli.md owns these totals. The generated catalog is the source of
  // truth; test the single public rendering so a catalog change cannot stale it.
  test("matches the command totals quoted in the CLI docs", () => {
    const totals = commandTotals(catalog.commands as Entry[]);
    expect(totals).toEqual({ total: 1270, tauri: 464, backend: 806, releaseExecutable: 1266 });

    const total = grouped(totals.total);
    const tauri = grouped(totals.tauri);
    const backend = grouped(totals.backend);
    expect(readFileSync("docs/cli.md", "utf8")).toContain(
      `The generated catalog contains ${total} entries: ${backend} backend-script commands and ${tauri} Tauri handlers.`,
    );
  });

  test("has stable unique identifiers and valid references", () => {
    expect(byId.size).toBe(catalog.commands.length);
    for (const entry of catalog.commands as Entry[]) {
      expect(entry.id).toBe(`${entry.transport === "tauri" ? "tauri" : "backend"}:${entry.name}`);
      expect(entry.frontendReferences.every((path) => path.startsWith("src/") && !path.includes("\\"))).toBe(true);
    }
  });

  test("flags frontend invocations that are absent from their desktop dispatcher", () => {
    const missing = (catalog.commands as Entry[]).filter(
      (entry) => entry.frontendReferences.length > 0 && !entry.registered,
    );
    expect(missing).toEqual([]);
  });

  test("does not route bespoke radar cards through the generic backend auto-healer", () => {
    const ids = getRadarDriftToggles().map((toggle) => toggle.id);
    expect(ids).not.toContain("contextMenuShred");
    expect(ids).not.toContain("contextMenuScrub");
  });

  test("derives debug-only handlers from their cfg gate instead of a hardcoded list", () => {
    const flagged = (catalog.commands as Entry[]).filter((entry) => entry.debugOnly).map((entry) => entry.id);
    expect(flagged).toEqual([
      "tauri:dev_reset_state",
      "tauri:dev_simulate_event",
      "tauri:open_devtools",
      "tauri:test_pro_dispatch",
    ]);
    // A debug-gated handler is absent from the release invoke registry, so the
    // release binary must refuse exactly these four and no others.
    const lib = readFileSync(resolve(import.meta.dir, "../src-tauri/commander-free/src/lib.rs"), "utf8");
    const gateCount = [...lib.matchAll(/#\[cfg\(debug_assertions\)\]\s*\n\s*(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*,/g)].length;
    expect(gateCount).toBe(flagged.length);
  });

  test("does not embed sensitive paid command names contiguously in Free", () => {
    const raw = readFileSync(
      resolve(import.meta.dir, "../src-tauri/commander-free/src/cli_catalog.generated.json"),
      "utf8",
    );
    expect(raw).not.toContain("Clear-MFTResidentSlack");
    expect(raw).not.toContain("Clear-NTFSLogFile");
    expect(raw).not.toContain("Destroy-VeraCryptHeader");
  });
});
