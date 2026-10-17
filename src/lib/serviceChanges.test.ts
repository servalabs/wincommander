import { describe, expect, it } from "bun:test";
import { runServiceChange, type ServiceChangePlan, type ServiceBackendCall, type ServiceChangeResult } from "./serviceChanges";

const plan: ServiceChangePlan = {
    id: "a".repeat(32), action: "Stop", name: "Parent", expiresAt: "2026-10-16T12:00:00Z", warnings: ["Other services will stop."],
    steps: [
        { action: "Stop", name: "Child", displayName: "Child service", startMode: "", reason: "Depends on Parent" },
        { action: "Stop", name: "Parent", displayName: "Parent service", startMode: "", reason: "Requested service" },
    ],
};
const done: ServiceChangeResult = { success: true, status: "done", snapshotId: "b".repeat(32), completed: plan.steps, failures: [], message: "Verified" };

function backend(preview: unknown = plan, result: unknown = done) {
    const calls: { command: string; params: unknown }[] = [];
    const call: ServiceBackendCall = async <T>(command: string, params?: Record<string, string | number | boolean>) => {
        calls.push({ command, params });
        return { success: true, data: (command === "Get-ServiceChangePlan" ? preview : result) as T };
    };
    return { calls, call };
}

describe("service preview approval", () => {
    it("cancelling a dependency preview performs no service change", async () => {
        const { call, calls } = backend();
        expect(await runServiceChange({ action: "Stop", name: "Parent" }, call, async shown => {
            expect(shown.steps.map(step => step.name)).toEqual(["Child", "Parent"]);
            return false;
        })).toBeNull();
        expect(calls.map(entry => entry.command)).toEqual(["Get-ServiceChangePlan"]);
    });
    it("executes only the backend-issued plan identifier after approval", async () => {
        const { call, calls } = backend();
        const result = await runServiceChange({ action: "Stop", name: "Parent" }, call, async () => true);
        expect(calls[1]).toEqual({ command: "Invoke-ServiceChangePlan", params: { PlanId: plan.id } });
        expect(result?.data?.snapshotId).toBe(done.snapshotId);
    });
    it("keeps a partial failure and its before-change snapshot visible", async () => {
        const { call } = backend(plan, { ...done, success: false, status: "partial", message: "Parent refused to stop", completed: [plan.steps[0]] });
        const result = await runServiceChange({ action: "Stop", name: "Parent" }, call, async () => true);
        expect(result?.success).toBe(false);
        expect(result?.error).toBe("Parent refused to stop");
        expect(result?.data?.snapshotId).toBe(done.snapshotId);
    });
    it("does not approve or execute malformed or unavailable previews", async () => {
        for (const preview of [{}, { ...plan, id: "../../other" }]) {
            const { call, calls } = backend(preview);
            const result = await runServiceChange({ action: "Profile" }, call, async () => { throw new Error("Must not confirm"); });
            expect(result?.success).toBe(false);
            expect(calls).toHaveLength(1);
        }
    });
    it("does not prompt or create another snapshot for already-applied settings", async () => {
        const { call, calls } = backend({ ...plan, steps: [] });
        expect((await runServiceChange({ action: "Profile" }, call, async () => { throw new Error("Must not confirm"); }))?.success).toBe(true);
        expect(calls).toHaveLength(1);
    });
});
