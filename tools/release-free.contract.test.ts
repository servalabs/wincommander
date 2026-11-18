import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const publisher = readFileSync("tools/release-free.ps1", "utf8");

describe("manual Free release publisher", () => {


  test("uses pinned SBOM tooling and promotes the updater pointer only after verification", () => {
    expect(publisher).toContain('"syft_${pinnedVersion}_windows_amd64.zip"');
    expect(publisher).toContain("815ee6973ec5dff6a671d7f41b0e78835a8c45b91d5a39f4743ea1cee833d3be");
    expect(publisher).toContain("Downloaded Syft archive does not match the pinned SHA-256.");
    expect(publisher).toContain("wincommander-free.sbom.cdx.json");
    expect(publisher).toContain("r2:windows/free/latest.exe");
    expect(publisher).toContain("r2:windows/free/latest.json");
    expect(publisher.indexOf("foreach ($asset in $immutable) { Assert-RemoteHash")).toBeLessThan(publisher.indexOf("r2:windows/free/latest.exe"));
    expect(publisher.indexOf("r2:windows/free/latest.exe")).toBeLessThan(publisher.indexOf("r2:windows/free/latest.json"));
  });


  test("refuses local beta publication so GitHub review cannot be bypassed", () => {
    expect(publisher).toContain("if ($Beta -and -not $StageOnly)");
    expect(publisher).toContain("Free beta publication requires the protected GitHub workflow.");
    expect(publisher).toContain("if ($Beta -and $Version -notmatch '^\\d+\\.\\d+\\.\\d+$')");
  });
});
