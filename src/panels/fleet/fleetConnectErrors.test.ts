import { describe, expect, test } from "bun:test";
import { describeFleetConnectError, describeTrustBridgeError, trustBridgeNeedsRepair, fleetAddressWarning, FLEET_ADDRESS_HINT, FLEET_HTTPS_HINT } from "./fleetConnectErrors";

describe("Fleet connect error wording", () => {
  test("warns about plain http:// while typing and accepts https://", () => {
    expect(fleetAddressWarning("http://fleet.example.test:9443")).toBe(FLEET_HTTPS_HINT);
    expect(fleetAddressWarning(" HTTP://fleet.example.test ")).toBe(FLEET_HTTPS_HINT);
    expect(fleetAddressWarning("fleet.example.test:9443")).toBe(FLEET_ADDRESS_HINT);
    expect(fleetAddressWarning("https://test-linux.leo-ordinal.ts.net:9443")).toBeNull();
    expect(fleetAddressWarning("  ")).toBeNull();
  });

  test("turns backend rejections into actionable hints and leaves others untouched", () => {
    expect(describeFleetConnectError("Fleet requires an HTTPS server with post-quantum hybrid TLS support.")).toBe(FLEET_HTTPS_HINT);
    expect(describeFleetConnectError("Fleet requires an https:// address (an HTTPS server). Change http:// to https://")).toBe(FLEET_HTTPS_HINT);
    expect(describeFleetConnectError("Fleet server URL must be a valid http(s) origin.")).toBe(FLEET_ADDRESS_HINT);
    expect(describeFleetConnectError("A Fleet connection change is already in progress")).toBe("A Fleet connection change is already in progress");
  });

  test("trust bridge failures map to closed-vocabulary hints", () => {
    expect(describeTrustBridgeError(null)).toBeNull();
    expect(describeTrustBridgeError("identity_change_unconfirmed")).toContain("not confirmed");
    expect(describeTrustBridgeError("proof_timeout")).toContain("did not finish enrolling");
    // An unknown or hostile code never reaches the screen verbatim.
    expect(describeTrustBridgeError("C:\secret\path")).toBe(describeTrustBridgeError("other"));
  });

  test("only trust-mismatch codes offer the repair path", () => {
    for (const code of ["identity_change_unconfirmed", "binding_changed", "invalid_proof"]) {
      expect(trustBridgeNeedsRepair(code)).toBe(true);
    }
    for (const code of ["proof_timeout", "other", "hybrid_replacement_required", "replacement_refused", null, undefined]) {
      expect(trustBridgeNeedsRepair(code)).toBe(false);
    }
    expect(describeTrustBridgeError("binding_changed")).toContain("different enrollment");
  });
});
