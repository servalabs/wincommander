import { describe, expect, test } from "bun:test";
import { resolveBundledBrandSlug } from "./appIcons";

describe("resolveBundledBrandSlug", () => {
  test("skips bundled lookups for optional icons we do not ship", () => {
    expect(resolveBundledBrandSlug("Google.Chrome")).toBeUndefined();
    expect(resolveBundledBrandSlug("Opera.Opera")).toBeUndefined();
    expect(resolveBundledBrandSlug("Opera.OperaGX")).toBeUndefined();
    expect(resolveBundledBrandSlug("Skype.Skype")).toBeUndefined();
    expect(resolveBundledBrandSlug("Audacity.Audacity")).toBeUndefined();
    expect(resolveBundledBrandSlug("JetBrains.PyCharm.Community")).toBeUndefined();
    expect(resolveBundledBrandSlug("GOG.Galaxy")).toBeUndefined();
  });

  test("keeps bundled lookups for other curated apps", () => {
    expect(resolveBundledBrandSlug("Discord.Discord")).toBe("discord");
    expect(resolveBundledBrandSlug("JetBrains.IntelliJIDEA.Community")).toBe("intellij");
    expect(resolveBundledBrandSlug("Google.GoogleDrive")).toBe("google-drive");
    expect(resolveBundledBrandSlug("AnyDesk.AnyDesk")).toBe("anydesk");
    expect(resolveBundledBrandSlug("DucFabulous.UltraViewer")).toBe("ultraviewer");
  });
});
