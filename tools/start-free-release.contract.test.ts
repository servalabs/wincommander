import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const script = readFileSync("tools/start-free-release.ps1", "utf8");

describe("prepared Free tag release", () => {
  test("tags an exactly prepared main source without recovery mode", () => {
    expect(script).toContain("A normal release may already have its four version records committed on");
    expect(script).toContain("$versionAlreadyPrepared = $current -eq $Version");
    expect(script).not.toContain("origin/main already declares $Version. Re-run with -ReplaceUnpublishedTag");
    expect(script).toContain("if ($remoteTag -and -not $ReplaceUnpublishedTag)");
  });

  test("refuses a request that would lower the version on main", () => {
    expect(script).toContain("origin/main already declares a newer version.");
    expect(script).toContain("Requested version is older than origin/main.");
    expect(script).toContain("return a.pre[i] < b.pre[i] ? -1 : 1;");
    expect(script.match(/return a\.pre\[i\] < b\.pre\[i\] \? -1 : 1;/g)).toHaveLength(2);
  });
});
