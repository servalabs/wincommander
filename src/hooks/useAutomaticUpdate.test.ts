import { describe, expect, test } from "bun:test";
import { proNeedsUpdate } from "./useAutomaticUpdate";

describe("automatic Pro updates", () => {
    test("updates legacy installed Pro copies that do not yet have a saved hash", () => {
        expect(proNeedsUpdate(null, "verified-latest-hash")).toBe(true);
    });

    test("does not update when the installed hash already matches", () => {
        expect(proNeedsUpdate("verified-latest-hash", "verified-latest-hash")).toBe(false);
    });
});
