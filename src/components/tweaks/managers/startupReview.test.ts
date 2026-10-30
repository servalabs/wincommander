import { describe, expect, test } from "bun:test";
import { matchesStartupSearch, startupIdentity, startupSignature, type StartupItem } from "./startupReview";
import type { StartupImpactEntry } from "../../../hooks/useBackend";

const item: StartupItem = { Name: "Sync", Command: '"C:\\Apps\\sync.exe" --login', Location: "HKCU:\\Run", RamUsageMB: 32, Status: "Running", IsEnabled: true, Source: "Registry", Recommendation: "Neutral", Category: "Cloud", Description: "Cloud sync" };
const signature: StartupImpactEntry = { id: "1", name: "Sync", source: "registry", location: item.Location, command: item.Command, executablePath: "C:\\Apps\\sync.exe", pathExists: true, signatureStatus: "Valid", signer: "Example Publisher", impact: "unknown", recommendation: "review" };

describe("Startup review identity and search", () => {
  test("does not attach a similarly named launcher's publisher", () => {
    expect(startupSignature(item, [{ ...signature, command: "C:\\Other\\sync.exe" }])).toBeUndefined();
    expect(startupSignature(item, [signature])).toEqual(signature);
    expect(startupIdentity(item)).not.toBe(startupIdentity({ ...item, Location: "HKLM:\\Run" }));
  });
  test("searches actual publisher and launcher paths without inventing missing metadata", () => {
    expect(matchesStartupSearch(item, signature, " EXAMPLE publisher ")).toBe(true);
    expect(matchesStartupSearch(item, undefined, "Example Publisher")).toBe(false);
    expect(matchesStartupSearch(item, undefined, "apps\\sync.exe")).toBe(true);
    expect(matchesStartupSearch(item, undefined, "cloud")).toBe(true);
    expect(matchesStartupSearch(item, undefined, "unrelated")).toBe(false);
  });
});
