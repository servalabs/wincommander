import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const hostsModule = readFileSync(
  "src-tauri/commander-free/scripts/modules/network/hosts.ps1",
  "utf8",
);

describe("Network Control blocklist availability", () => {
  test("returns Altium and SOLIDWORKS to the UI status endpoint", () => {
    const availableNames = hostsModule.split("function Get-HostsBlocklistNames")[1]
      ?.split("}")[0] ?? "";

    expect(availableNames).toContain("'altium'");
    expect(availableNames).toContain("'solidworks'");
    expect(hostsModule).toContain("$availableBlocklistNames = Get-HostsBlocklistNames");
  });
});
