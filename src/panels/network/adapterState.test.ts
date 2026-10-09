import { describe, expect, test } from "bun:test";
import type { AdapterMacResult, PhysicalNetworkAdapter } from "../../hooks/useBackend";
import { adapterMacDescription, adapterOperationMessage } from "./adapterState";

describe("adapter observed state", () => {
    test("a partial or older backend success cannot become a verified toast", () => {
        for (const status of ["partial", "ok", "unverified", "rolled_back", "failed", "blocked"]) {
            expect(adapterOperationMessage({ status, observedMac: "021122334455" } as AdapterMacResult, "randomize").verified).toBe(false);
        }
        expect(adapterOperationMessage({ status: "verified" }, "factory").verified).toBe(false);
    });
    test("MAC verification does not claim a disconnected link is online", () => {
        const outcome = adapterOperationMessage({ status: "verified", observedMac: "021122334455", linkStatus: "Disconnected" }, "randomize");
        expect(outcome.verified).toBe(true);
        expect(outcome.message).toContain("Disconnected");
        expect(outcome.message).not.toContain("link is up");
    });
    test("unknown factory address does not imply factory mode", () => {
        expect(adapterMacDescription({ isSpoofed: false } as PhysicalNetworkAdapter)).toContain("unavailable");
    });
    test("configured override mismatch remains visible", () => {
        expect(adapterMacDescription({ currentMac: "00:11:22:33:44:55", configuredMac: "021122334455", macState: "factory" } as PhysicalNetworkAdapter)).toContain("not currently applied");
    });
});
