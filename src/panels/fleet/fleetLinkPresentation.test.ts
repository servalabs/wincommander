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

  test("active retry takes precedence over its last transient error, but not a confirmed connection", () => {
    const retry = status({ retrying: true, lastError: "server temporarily unavailable" });
    expect(fleetLinkState(retry)).toBe("reconnecting");
    expect(fleetLinkState({ ...retry, retrying: false })).toBe("error");
    expect(fleetLinkState({ ...retry, connected: true })).toBe("linked");
    expect(fleetLinkState({ ...retry, deviceId: "" })).toBe("error");
    expect(fleetLinkState({ ...retry, serverUrl: " " })).toBe("error");
  });

  test("only calls a device unlinked when the status bridge has no device identity", () => {
    expect(fleetLinkState(status({ deviceId: "", serverUrl: "", lastError: null }))).toBe("not_linked");
  });

  test("does not turn a historical signed contact into connection health", () => {
    const lastSignedContactAt = "2026-11-03T12:00:00Z";
    expect(fleetLinkState(status({ lastSignedContactAt }))).toBe("offline");
    expect(fleetLinkState(status({ connected: true, lastSignedContactAt: null }))).toBe("linked");
  });
});

describe("Fleet link presentation while awaiting approval", () => {
  const accepted = "2026-11-17T10:00:00Z";

  test("a connected, unapproved device is pending", () => {
    expect(fleetLinkState(status({ connected: true, pendingApproval: true, lastError: "stale" }))).toBe("pending");
  });

  test("a retrying agent is pending only with an accepted enrollment and no current failure", () => {
    const held = status({ pendingApproval: true, retrying: true, lastEnrollAt: accepted });
    expect(fleetLinkState(held)).toBe("pending");
    // A current error means the last contact failed: pendingApproval may be stale.
    expect(fleetLinkState({ ...held, lastError: "check-in timed out" })).toBe("reconnecting");
    // No evidence the server ever accepted the enrollment.
    expect(fleetLinkState({ ...held, lastEnrollAt: null })).toBe("reconnecting");
    // Not retrying: nothing is running, so a stale flag must not claim approval is awaited.
    expect(fleetLinkState({ ...held, retrying: false })).toBe("offline");
    expect(fleetLinkState({ ...held, retrying: false, lastError: "removed" })).toBe("error");
    expect(fleetLinkState({ ...held, deviceId: "" })).toBe("not_linked");
  });
});
