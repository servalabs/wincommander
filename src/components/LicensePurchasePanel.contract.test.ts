import { describe, expect, test } from "bun:test";

declare const Bun: {
  file(path: string): {
    text(): Promise<string>;
  };
};

function read(path: string): Promise<string> {
  return Bun.file(path).text();
}

/** JSX wraps long sentences across lines; compare on collapsed whitespace. */
function flat(source: string): string {
  return source.replace(/\s+/g, " ");
}

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

describe("Purchase surface: shortest path to payment", () => {
  test("plan terms come from the catalogue once, as points, not a client-side restatement", async () => {
    const panel = await read("src/components/LicensePurchasePanel.tsx");
    const points = await read("src/components/LicenseOfferPoints.tsx");

    // OFFER_PROMISES restated offer.detail/offer.deviceRule a second and third
    // time for the same plan; the catalogue is now the only source.
    expect(panel).not.toContain("OFFER_PROMISES");
    expect(points).toContain("offer.detail");
    expect(points).toContain("offer.deviceRule");
    // Points render once, for the selection only.
    expect(panel).toContain("{selected && !purchase.pending && <LicenseOfferPoints offer={selected} />}");
    expect(panel).not.toContain("license-gate-offer-detail");
    expect(panel).not.toContain("license-gate-offer-device");
  });

  test("prices are only ever the server's labels", async () => {
    const panel = await read("src/components/LicensePurchasePanel.tsx");
    const card = await read("src/components/LicenseOfferCard.tsx");

    expect(card).toContain("offer.priceLabel");
    expect(panel).toContain("Checkout total: ");
    // No literal money anywhere in the purchase UI.
    expect(/[₹$]\s?\d/.test(panel)).toBe(false);
    expect(/[₹$]\s?\d/.test(card)).toBe(false);
  });

  test("required email gates the CTA and the optional phone stays behind a toggle", async () => {
    const panel = await read("src/components/LicensePurchasePanel.tsx");

    expect(panel).toContain("/^[^\\s@]+@[^\\s@]+\\.[^\\s@]+$/");
    expect(panel).toContain("disabled={!emailOk || purchase.isStarting || !selected || !selected.checkoutEligible}");
    expect(panel).toContain('replace(/[^\\d+]/g, "")');
    expect(panel).toContain("+ Add phone (optional)");
    expect(panel).toContain("isPhoneShown");
    expect(panel).toContain('htmlFor={emailId}');
    expect(panel).toContain('id={emailId}');
    expect(panel).toContain('htmlFor={phoneId}');
    expect(panel).toContain('id={phoneId}');
  });

  test("checkout creation and the Fleet seat bounds are enforced and announced", async () => {
    const panel = await read("src/components/LicensePurchasePanel.tsx");

    expect(panel).toContain("await purchase.start({");
    expect(panel).toContain("await open(pending.checkoutUrl)");
    expect(panel).toContain("Math.max(selected.minSeats!, value - 1)");
    expect(panel).toContain("Math.min(selected.maxSeats!, value + 1)");
    expect(panel).toContain("Math.max(selected.minSeats!, value)");
    expect(panel).toContain('aria-label={`Decrease managed devices; minimum ${selected.minSeats}`}');
    expect(panel).toContain('aria-label={`Increase managed devices; maximum ${selected.maxSeats}`}');
    expect(panel).toContain('aria-label={`${fleetSeats} managed devices selected`}');
  });

  test("the modal and existing-key field expose their purpose to assistive technology", async () => {
    const gate = await read("src/components/LicenseGate.tsx");

    expect(gate).toContain('role={inline ? "region" : "dialog"}');
    expect(gate).toContain('aria-modal={inline ? undefined : true}');
    expect(gate).toContain("aria-labelledby={titleId}");
    expect(gate).toContain("aria-describedby={subtitleId}");
    expect(gate).toContain('htmlFor={licenseKeyId}');
    expect(gate).toContain('id={licenseKeyId}');
  });

  test("the existing-key dialog stays open until explicitly closed", async () => {
    const gate = await read("src/components/LicenseGate.tsx");

    expect(gate).not.toContain('onClick={inline ? undefined : closeModal}');
    expect(gate).toContain('if (event.key !== "Escape") return;');
    expect(gate).toContain('ref={licenseKeyInputRef}');
    expect(gate).toContain('licenseKeyInputRef.current?.focus()');
  });

  test("the pending checkout state keeps its key box, hints and recovery buttons", async () => {
    const pending = await read("src/components/LicensePendingPurchase.tsx");

    expect(pending).toContain("license-gate-license-key");
    expect(pending).toContain("Open secure checkout");
    expect(pending).toContain("I’ve paid — verify with Razorpay");
    expect(pending).toContain("Copy key");
    expect(pending).toContain("Resend email");
    expect(pending).toContain("Start over");
    expect(pending).toContain('purchase.status?.providerStatus === "authorized"');
    expect(pending).toContain("This checkout resumes automatically after an app restart.");
  });

  test("the animated primitives replace the hand-rolled tab underline and CTA", async () => {
    const gate = await read("src/components/LicenseGate.tsx");
    const panel = await read("src/components/LicensePurchasePanel.tsx");
    const card = await read("src/components/LicenseOfferCard.tsx");

    expect(gate).toContain('from "@/components/ui/tabs"');
    expect(gate).toContain('<TabsTrigger value="buy"');
    expect(gate).toContain('<TabsTrigger value="activate"');
    expect(gate).not.toContain("license-gate-tab");
    expect(panel).toContain('variant="primary"');
    // Selection state animates as a shared element, scoped per panel instance.
    expect(card).toContain("layoutId={`license-offer-ring-${ringId}`}");
  });

  test("trust and compliance copy survived the move into the terms popover", async () => {
    const terms = flat(await read("src/components/LicensePurchaseTerms.tsx"));
    const panel = await read("src/components/LicensePurchasePanel.tsx");

    // Was the "Free stays free" box.
    expect(terms).toContain("Free stays free");
    expect(terms).toContain(
      "The core WinCommander app works without a licence or payment. Upgrade only for Pro capabilities, services, or managed Fleet endpoints.",
    );
    // Was the server-quote disclaimer.
    expect(terms).toContain("The secure checkout service confirms the current price before payment.");
    // Was the .license-gate-buy-note footer.
    expect(terms).toContain(
      "Checkout opens in your normal browser. WinCommander never handles payment credentials. Pro Lifetime keeps normal Pro usable permanently; memberships retain the last eligible normal Pro build if they end. Netwall is a hosted, term-based service.",
    );
    expect(terms).toContain("Investigator is available only while its explicit entitlement is active.");
    // None of it left behind as a full-width block in the pay path.
    expect(panel).not.toContain("license-gate-free");
    expect(panel).not.toContain("license-gate-buy-note");
  });
});

/** Lives here because this is the licence UI's only contract test; the bug was
 *  three sidebar facts painting over each other inside ~70px columns. */
describe("Sidebar licence card cannot overflow", () => {
  test("stat values truncate and the action row wraps instead of clipping", async () => {
    const css = await read("src/components/Sidebar.css");
    const stats = await read("src/components/LicenseQuickStats.tsx");

    expect(css).toContain(".license-quick-panel .license-stat-value");
    expect(css).not.toContain("grid-template-columns: repeat(3, minmax(0, 1fr)) !important");

    const buttonRule = css.slice(css.indexOf("\n        .license-btn {"));
    const buttonBody = buttonRule.slice(0, buttonRule.indexOf("}"));
    // A fixed height is what let the longest label spill out of its box, and
    // ellipsis is what stops it painting over the neighbouring button.
    expect(/[^-]height: 24px/.test(buttonBody)).toBe(false);
    expect(buttonBody).toContain("min-height: 24px");
    expect(buttonBody).toContain("text-overflow: ellipsis");
    // Every value carries its full form in a title attribute.
    expect(stats).toContain("title={remainingTitle(daysRemaining)}");
    expect(stats).toContain("title={licensePlanLabel(plan)}");
  });
});
