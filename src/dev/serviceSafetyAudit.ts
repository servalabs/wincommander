import type { ServiceChangePlan, ServicePlanStep, ServiceSnapshot } from "../lib/serviceChanges";

const snapshot: ServiceSnapshot = { id: "b".repeat(32), createdAt: "2026-10-16T08:00:00Z", note: "Synthetic audit snapshot", serviceCount: 3 };

export function serviceSafetyAuditResponse(command: string, params: Record<string, unknown>): unknown | undefined {
    if (command === "Get-ServiceSnapshots") return { snapshots: [snapshot] };
    if (command === "New-ServiceSnapshot") return snapshot;
    if (command === "Compare-ServiceSnapshot") return {
        snapshot, configurationCount: 1, runtimeCount: 1, unavailableCount: 0, excludedPerUserCount: 1,
        changes: [
            { name: "WCAuditIndexer", field: "startMode", category: "configuration", before: "Auto", after: "Manual" },
            { name: "WCAuditIndexer", field: "state", category: "runtime", before: "Running", after: "Stopped" },
        ],
    };
    if (command === "Get-ServiceChangePlan") {
        const action = String(params.Action || "Stop") as ServiceChangePlan["action"];
        const name = String(params.Name || "WCAuditIndexer");
        const target = name === "WCAuditLegacy" ? "WinCommander Audit Legacy Helper" : "WinCommander Audit Indexer";
        const steps: ServicePlanStep[] = action === "Profile"
            ? [{ action: "SetStartMode", name: "WCAuditLegacy", displayName: "WinCommander Audit Legacy Helper", startMode: "Disabled", reason: "Recommended profile" }]
            : action === "SetStartMode"
                ? [{ action, name, displayName: target, startMode: String(params.StartMode || "Manual"), reason: "Change startup setting" }]
                : [
                    ...(action === "Stop" ? [{ action: "Stop" as const, name: "WCAuditDependent", displayName: "Synthetic audit delivery helper", startMode: "", reason: "Depends on WCAuditIndexer" }] : []),
                    { action: action === "Stop" ? "Stop" : "Start", name, displayName: target, startMode: "", reason: "Requested service" },
                ];
        return { id: "a".repeat(32), expiresAt: new Date(Date.now() + 600_000).toISOString(), action, name, steps, warnings: ["Synthetic audit preview. No Windows services are changed in this screen."] } satisfies ServiceChangePlan;
    }
    if (command === "Invoke-ServiceChangePlan" || command === "Set-ServicesManual") return {
        success: true, status: "done", completed: [], failures: [], snapshotId: snapshot.id, message: "Synthetic audit result. No Windows services changed.",
    };
    return undefined;
}
