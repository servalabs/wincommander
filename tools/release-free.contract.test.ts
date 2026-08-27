import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const publisher = readFileSync("tools/release-free.ps1", "utf8");

describe("manual Free release publisher", () => {
  test("requires an exact clean, tagged, version-aligned main source for publication", () => {
    expect(publisher).toContain("Refusing to release from a dirty worktree.");
    expect(publisher).toContain("@(git -C $script:Root status --porcelain).Count");
    expect(publisher).toContain("param([switch]$AllowUntaggedStage)");
    expect(publisher).toContain("if (-not $AllowUntaggedStage)");
    expect(publisher).toContain('"$script:Tag^{commit}"');
    expect(publisher).toContain('"refs/heads/main:refs/remotes/origin/main"');
    expect(publisher).toContain('"refs/tags/${script:Tag}:refs/tags/${script:Tag}"');
    expect(publisher).toContain('origin/main');
    expect(publisher).toContain("'package.json'");
    expect(publisher).toContain("'Free Tauri config'");
    expect(publisher).toContain("'Free Cargo manifest'");
    expect(publisher).toContain("'Free Cargo lock record'");
    expect(publisher).not.toContain("Set-CargoVersion");
    expect(publisher).toContain("Assert-ReleaseSource -AllowUntaggedStage:$StageOnly");
    expect(publisher).toContain("patch versions stop at .9");
    expect(publisher).toContain("[int]$versionMatch.Groups['patch'].Value -gt 9");
  });

  test("builds the signed NSIS artifact and validates its updater sidecar", () => {
    expect(publisher).toContain("bun run build:free:release-installer");
    expect(publisher).toContain('bundle\\nsis');
    expect(publisher).toContain('_x64-setup.exe');
    expect(publisher).toContain("Tauri updater signature is not valid Base64.");
    expect(publisher).toContain("The updater signature does not name");
    expect(publisher).not.toContain("latest.msi");
  });

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

  test("keeps local staging non-publishing and does not fake GitHub provenance", () => {
    expect(publisher).toContain("No R2 or GitHub publication was attempted.");
    expect(publisher).toContain("No GitHub Actions provenance attestation was generated");
    expect(publisher).toContain("@('release', 'create'");
    expect(publisher).toContain("& gh release upload");
    expect(publisher).toContain("gh release edit $script:Tag --draft=false");
  });
});
