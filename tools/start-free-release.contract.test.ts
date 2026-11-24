import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const script = readFileSync(join(import.meta.dir, "start-free-release.ps1"), "utf8");

describe("start-free-release.ps1 release profile", () => {
  test("offers classical (default) and hybrid only", () => {
    expect(script).toContain("[ValidateSet('classical', 'hybrid')]");
    expect(script).toContain("[string]$ReleaseProfile = 'classical'");
  });

  test("starts the classical workflow run explicitly against the pushed tag", () => {
    expect(script).toContain("gh workflow run release.yml");
    expect(script).toContain('-f "release_tag=$tag"');
    expect(script).toContain("-f 'release_profile=classical'");
    // The dispatch must come after the tag is pushed.
    expect(script.indexOf("'push', 'origin', \"refs/tags/$tag\"")).toBeLessThan(script.indexOf("gh workflow run release.yml"));
  });

  test("beta candidates stay on the hybrid path", () => {
    expect(script).toContain("if ($Beta -and $ReleaseProfile -eq 'classical')");
  });

  test("hybrid keeps the old behaviour and does not dispatch", () => {
    const hybridBranch = script.slice(script.indexOf("else {\n        Write-Output \"Started Free release") === -1
      ? script.indexOf("else {\r\n        Write-Output \"Started Free release")
      : script.indexOf("else {\n        Write-Output \"Started Free release"));
    expect(hybridBranch).not.toContain("gh workflow run");
  });
});
