import { describe, expect, test } from "bun:test";
import { fleetLinkLabel, fleetLinkState, type FleetLinkStatus } from "./fleetLinkPresentation";

const status = (patch: Partial<FleetLinkStatus> = {}): FleetLinkStatus => ({
  connected: false,
  deviceId: "device-123",
  serverUrl: "http://fleet.example.test:8787",
  lastEnrollAt: null,
  lastError: null,
  retrying: false,
  ...patch,
});

describe("Fleet link presentation", () => {
  test("uses the shared agent status rather than requiring a user app.fleet setting", () => {
    expect(fleetLinkState(status({ connected: true }))).toBe("linked");
    expect(fleetLinkLabel("linked")).toBe("Linked");
  });

  test("keeps an admitted-but-unapproved device distinct from a fully linked device", () => {
    expect(fleetLinkState(status({ connected: true, pendingApproval: true }))).toBe("pending");
  });

  test("shows a known device as reconnecting, offline, or failed instead of not linked", () => {
    expect(fleetLinkState(status({ retrying: true }))).toBe("reconnecting");
    expect(fleetLinkState(status())).toBe("offline");
    expect(fleetLinkState(status({ lastError: "agent session unavailable" }))).toBe("error");
  });

  test("only calls a device unlinked when the status bridge has no device identity", () => {
    expect(fleetLinkState(status({ deviceId: "", serverUrl: "", lastError: null }))).toBe("not_linked");
  });
});
