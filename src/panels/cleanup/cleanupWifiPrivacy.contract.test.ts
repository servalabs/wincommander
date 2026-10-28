import { expect, test } from "bun:test";

test("the cleanup Wi-Fi getter explicitly requests no secrets", async () => {
  const source = await Bun.file("src/hooks/useBackend.ts").text();
  expect(source).toMatch(/getWlanProfiles:\s*\(\)\s*=>\s*execute[^\n]+"Get-WlanProfiles",\s*\{\s*IncludeSecrets:\s*false\s*\}/);
});
