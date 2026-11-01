import { expect, test } from "bun:test";

test("the cleanup Wi-Fi getter explicitly requests no secrets", async () => {
  const source = await Bun.file("src/hooks/useBackend.ts").text();
  expect(source).toMatch(/getWlanProfiles:\s*\(\)\s*=>\s*execute[^\n]+"Get-WlanProfiles",\s*\{\s*IncludeSecrets:\s*false\s*\}/);
  expect(source).toMatch(/getWlanProfileSecrets:\s*\(\)\s*=>\s*execute[^\n]+"Get-WlanProfiles",\s*\{\s*IncludeSecrets:\s*true\s*\}/);
  const collector = await Bun.file("src-tauri/commander-free/scripts/modules/privacy/cleanup.ps1").text();
  expect(collector).toContain("Get-WlanProfiles -IncludeSecrets:$false");
});
