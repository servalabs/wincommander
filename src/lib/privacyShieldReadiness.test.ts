// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "bun:test";
import { createPrivacyShieldPollGate, privacyShieldReadinessLabel, privacyShieldProtectionReady, privacyShieldRuntimeSetupNeeded } from "./privacyShieldReadiness";

describe("Privacy Shield readiness", () => {
    it("requires an observed process and fresh successful inference", () => {
        for (const running of [false, null]) {
            expect(privacyShieldReadinessLabel(running, true)).toBe("Protection unverified");
        }
        for (const ready of [false, undefined, null, "true"]) {
            expect(privacyShieldReadinessLabel(true, ready)).toBe("Protection unverified");
        }
        expect(privacyShieldReadinessLabel(true, true)).toBe("Active");
    });
});

it("replaces a prior ready observation after failed refresh, restart, or delayed inference", () => {
    let ready = privacyShieldProtectionReady({ running: true, protectionReady: true });
    expect(privacyShieldReadinessLabel(true, ready)).toBe("Active");
    for (const next of [null, {}, { running: true }, { running: true, protectionReady: false }, { running: false, protectionReady: true }]) {
        ready = privacyShieldProtectionReady(next);
        expect(privacyShieldReadinessLabel(true, ready)).toBe("Protection unverified");
    }
});

it("offers explicit runtime setup instead of hidden start-time package replacement", () => {
    expect(privacyShieldRuntimeSetupNeeded("Privacy Shield AI runtime is incomplete. Open AI runtime setup in Privacy Shield, then retry.")).toBe(true);
    expect(privacyShieldRuntimeSetupNeeded("runtime_dependency_missing")).toBe(true);
    expect(privacyShieldRuntimeSetupNeeded("Camera is busy")).toBe(false);
    expect(privacyShieldRuntimeSetupNeeded(null)).toBe(false);
});

it("a late poll started during Stop cannot restore Active after Stop completes", async () => {
    const gate = createPrivacyShieldPollGate();
    let finishObservation!: (value: unknown) => void;
    let ready = true;
    gate.invalidate(); // Stop begins.
    ready = false;
    const observation = gate.begin()!;
    const response = new Promise<unknown>((resolve) => { finishObservation = resolve; });
    const poll = response.then((status) => {
        if (gate.isCurrent(observation)) ready = privacyShieldProtectionReady(status);
    }).finally(() => gate.finish());
    expect(gate.begin()).toBeNull();
    gate.invalidate(); // Stop completes while the old process observation is pending.
    finishObservation({ running: true, protectionReady: true });
    await poll;
    expect(ready).toBe(false);
    const fresh = gate.begin()!;
    expect(gate.isCurrent(fresh)).toBe(true);
    gate.finish();
});

it("a mutation also invalidates polls that began before it", () => {
    const gate = createPrivacyShieldPollGate();
    const observation = gate.begin()!;
    gate.invalidate();
    expect(gate.isCurrent(observation)).toBe(false);
    gate.finish();
    expect(gate.isCurrent(gate.begin()!)).toBe(true);
    gate.finish();
});
