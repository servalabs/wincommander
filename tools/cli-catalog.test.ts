import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import catalog from "../src-tauri/commander-free/src/cli_catalog.generated.json";
import { getRadarDriftToggles } from "../src/registry";

type Entry = (typeof catalog.commands)[number];

const byId = new Map(catalog.commands.map((entry) => [entry.id, entry]));

describe("generated WinCommander CLI catalog", () => {
  test("covers the real Tauri and backend command surfaces", () => {
    expect(catalog.schemaVersion).toBe(1);
    expect(catalog.commands.length).toBeGreaterThan(400);
    expect(byId.get("tauri:get_settings")?.registered).toBe(true);
    expect(byId.get("tauri:run_backend_script")?.registered).toBe(true);
    expect(byId.get("backend:Get-SystemInfo")?.registered).toBe(true);
    expect(byId.get("backend:Get-ShellBags")?.registered).toBe(true);
    expect(byId.get("backend:Clear-MFTResidentSlack")?.registered).toBe(true);
    expect(byId.has("backend:size-descending")).toBe(false);
    expect(byId.has("backend:lockdown-step")).toBe(false);
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
