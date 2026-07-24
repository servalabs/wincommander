import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

describe("Investigator offer visibility", () => {
  test("the purchase surface filters Investigator unless the active licence explicitly grants it", async () => {
    const panel = await Bun.file("src/components/LicensePurchasePanel.tsx").text();

    expect(panel).toContain('licenseStatus?.valid === true');
    expect(panel).toContain('(licenseStatus.features ?? []).includes("advanced")');
    expect(panel).toContain(
      'offer.sku !== "investigator" || hasInvestigatorEntitlement',
    );
    expect(panel).toContain("catalog.offers?.filter(");
    expect(panel).not.toContain(
      "Fleet and Netwall are included; Investigator is not.",
    );
  });

  test("the entitled launcher updates a stale installed pair but still launches offline", async () => {
    const hook = await Bun.file("src/hooks/useInvestigatorInstall.ts").text();

    expect(hook).toContain('invoke<InvestigatorReleaseManifest>("fetch_investigator_manifest")');
    expect(hook).toContain("current.version !== latest.version");
    expect(hook).toContain("if (!current.installed) throw error");
    expect(hook).toContain('invoke("install_investigator_product")');
    expect(hook).toContain('invoke("launch_investigator_product")');
  });
});
