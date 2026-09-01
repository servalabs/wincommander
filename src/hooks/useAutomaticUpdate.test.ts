import { describe, expect, test } from "bun:test";
import { canAutomaticallyUpdatePro, proNeedsUpdate } from "./useAutomaticUpdate";

describe("automatic Pro updates", () => {
    test("updates legacy installed Pro copies that do not yet have a saved hash", () => {
        expect(proNeedsUpdate(null, "verified-latest-hash")).toBe(true);
    });

    test("does not update when the installed hash already matches", () => {
        expect(proNeedsUpdate("verified-latest-hash", "verified-latest-hash")).toBe(false);
    });

    test("updates an installed Pro copy without requiring a new Defender exclusion", () => {
        expect(canAutomaticallyUpdatePro(true, true)).toBe(true);
    });

    test("never auto-installs Pro for an unpaid user or a missing sidecar", () => {
        expect(canAutomaticallyUpdatePro(false, true)).toBe(false);
        expect(canAutomaticallyUpdatePro(true, false)).toBe(false);
        expect(canAutomaticallyUpdatePro(true, null)).toBe(false);
    });
});
