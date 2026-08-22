import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const publisher = readFileSync("tools/release-free.ps1", "utf8");

describe("manual Free release publisher", () => {
  test("requires an exact clean, tagged, version-aligned main source", () => {
    expect(publisher).toContain("Refusing to release from a dirty worktree.");
    expect(publisher).toContain('"$script:Tag^{commit}"');
    expect(publisher).toContain('origin/main');
    expect(publisher).toContain("'package.json'");
    expect(publisher).toContain("'Free Tauri config'");
    expect(publisher).toContain("'Free Cargo manifest'");
    expect(publisher).toContain("'Free Cargo lock record'");
    expect(publisher).not.toContain("Set-CargoVersion");
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
  });
});
