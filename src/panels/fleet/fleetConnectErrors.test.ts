import { describe, expect, test } from "bun:test";
import { describeFleetConnectError, fleetAddressWarning, FLEET_ADDRESS_HINT, FLEET_HTTPS_HINT } from "./fleetConnectErrors";

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
});
