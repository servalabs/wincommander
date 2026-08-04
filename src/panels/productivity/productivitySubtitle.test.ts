import { describe, expect, it } from "bun:test";
import { productivitySubtitle } from "./productivitySubtitle";

describe("productivity disclosure", () => {
  it("does not claim activity stays local when an enabled fleet status request fails", () => {
    const subtitle = productivitySubtitle(true, "unverified");

    expect(subtitle).toContain("could not be verified");
    expect(subtitle).not.toContain("not fleet-enrolled");
  });
});
