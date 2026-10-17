import type { BackendResponse } from "../hooks/useBackend";

export interface ServiceChangeRequest {
    action: "Start" | "Stop" | "Restart" | "SetStartMode" | "Profile";
    name?: string;
    startMode?: string;
    machineWide?: boolean;
}

export interface ServicePlanStep {
    action: "Start" | "Stop" | "SetStartMode";
    name: string;
    displayName: string;
    startMode: string;
    reason: string;
}

export interface ServiceChangePlan {
    id: string;
    expiresAt: string;
    action: ServiceChangeRequest["action"];
    name: string;
    steps: ServicePlanStep[];
    warnings: string[];
}

export interface ServiceChangeResult {
    success: boolean;
    status: "done" | "partial" | "applied" | "failed";
    snapshotId: string | null;
    completed: ServicePlanStep[];
    failures: { name: string; action: string; message: string }[];
    message: string;
}

export interface ServiceSnapshot {
    id: string;
    createdAt: string;
    note: string;
    serviceCount?: number;
}

export interface ServiceSnapshotComparison {
    snapshot: ServiceSnapshot;
    configurationCount: number;
    runtimeCount: number;
    unavailableCount: number;
    excludedPerUserCount: number;
    changes: { name: string; field: string; category: "configuration" | "runtime" | "unavailable"; before: unknown; after: unknown }[];
}

export type ServiceBackendCall = <T>(command: string, params?: Record<string, string | number | boolean>) => Promise<BackendResponse<T>>;

export async function runServiceChange(
    request: ServiceChangeRequest,
    call: ServiceBackendCall,
    confirm: (plan: ServiceChangePlan) => Promise<boolean>,
): Promise<BackendResponse<ServiceChangeResult> | null> {
    const preview = await call<ServiceChangePlan>("Get-ServiceChangePlan", {
        Action: request.action, Name: request.name ?? "", StartMode: request.startMode ?? "",
    });
    if (!preview.success || !preview.data) return { success: false, error: preview.error || "Service preview unavailable." };
    const plan = preview.data;
    if (!/^[a-f0-9]{32}$/.test(plan.id) || !Array.isArray(plan.steps) || !Array.isArray(plan.warnings)) {
        return { success: false, error: "Service preview unavailable." };
    }
    if (!plan.steps.length) return { success: true, data: { success: true, status: "done", snapshotId: null, completed: [], failures: [], message: "These service settings are already applied." } };
    if (!await confirm(plan)) return null;
    const result = await call<ServiceChangeResult>(request.action === "Profile" ? "Set-ServicesManual" : "Invoke-ServiceChangePlan", {
        PlanId: plan.id, ...(request.machineWide ? { MachineWide: true } : {}),
    });
    if (!result.success || !result.data) return { success: false, error: result.error || "Service change result unavailable." };
    if (!result.data.success) return { ...result, success: false, error: result.data.message || "Some service changes could not be completed." };
    return result;
}

export function serviceStepLabel(step: ServicePlanStep): string {
    const label = step.displayName || step.name;
    return step.action === "SetStartMode" ? `${label}: startup → ${step.startMode}` : `${step.action} ${label}`;
}

const FIELD_LABELS: Record<string, string> = {
    Service: "Service installed", startMode: "Startup", delayedAutoStart: "Delayed start",
    account: "Windows account", binaryPath: "Program", dependencies: "Depends on", state: "Running state",
};

export function serviceSnapshotField(field: string): string { return FIELD_LABELS[field] ?? field; }

export function serviceSnapshotValue(value: unknown): string {
    if (value === null || value === undefined) return "Unavailable";
    if (typeof value === "boolean") return value ? "Yes" : "No";
    return String(value) || "None";
}
