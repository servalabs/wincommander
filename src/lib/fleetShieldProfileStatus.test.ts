import { describe, expect, test } from "bun:test";
import { fleetShieldProfileStatus } from "./fleetShieldProfileStatus";

describe("Fleet Shield profile status", () => {
  test("accepts only the closed service status vocabulary", () => {
    expect(fleetShieldProfileStatus({ trustBound: true, shieldPolicyStatus: "verified" })).toBe("verified");
    expect(fleetShieldProfileStatus({ trustBound: true, shieldPolicyStatus: "effective_config_unavailable" })).toBe("effective_config_unavailable");
    for (const value of [null, {}, { trustBound: false, shieldPolicyStatus: "verified" }, { trustBound: "yes", shieldPolicyStatus: "verified" }, { trustBound: true }, { trustBound: true, shieldPolicyStatus: "secret" }]) {
      expect(fleetShieldProfileStatus(value)).toBe("unverified");
    }
  });
});
