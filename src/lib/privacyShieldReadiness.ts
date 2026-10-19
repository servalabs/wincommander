// SPDX-License-Identifier: AGPL-3.0-or-later
/** A process can still be starting, stalled, or failing inference. */
export function privacyShieldReadinessLabel(running: boolean | null, ready: unknown): string {
    return running === true && ready === true ? "Active" : "Protection unverified";
}
/** Every refresh replaces prior evidence, including failed/malformed refreshes. */
export function privacyShieldProtectionReady(status: unknown): boolean {
    if (!status || typeof status !== "object") return false;
    const observed = status as { running?: unknown; protectionReady?: unknown };
    return observed.running === true && observed.protectionReady === true;
}
/** Keep dependency failures actionable through the existing explicit setup UI. */
export function privacyShieldRuntimeSetupNeeded(message: unknown): boolean {
    return typeof message === "string" && /python is required|missing python dependency|runtime_dependency_missing|runtime_unavailable|ai runtime is (?:incomplete|unavailable)/i.test(message);
}

/** Mutations invalidate observations from both before and during the operation. */
export function createPrivacyShieldPollGate() {
    let generation = 0;
    let inFlight = false;
    return {
        begin() {
            if (inFlight) return null;
            inFlight = true;
            return ++generation;
        },
        isCurrent(observation: number) { return observation === generation; },
        finish() { inFlight = false; },
        invalidate() { ++generation; },
    };
}
