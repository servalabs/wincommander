// Shared formatter for maintenance command results. Backend functions like
// Set-ServicesManual return per-bucket counters (touched / alreadyOK / missing
// / failed[]) plus the actual touched-service names in touchedNames[] -- the
// formatter surfaces both so users see WHICH services were touched and WHICH
// failed at a glance.

interface FailedEntry {
    name?: string;
    error?: string;
}

interface MaintenanceBucket {
    total?: number;
    touched?: number;
    touchedNames?: string[];
    alreadyOK?: number;
    missing?: number;
    failed?: Array<FailedEntry> | number;
}

interface MaintenancePayload {
    durationMs?: number;
    manual?: MaintenanceBucket;
    disable?: MaintenanceBucket;
}

interface BackendEnvelope<T> {
    data?: T;
    success?: boolean;
}

const NAME_LIST_LIMIT = 6;

function summariseNames(names: string[] | undefined): string {
    if (!names || names.length === 0) return "";
    if (names.length <= NAME_LIST_LIMIT) return names.join(", ");
    const head = names.slice(0, NAME_LIST_LIMIT).join(", ");
    return `${head} +${names.length - NAME_LIST_LIMIT} more`;
}

function summariseFailures(failed: Array<FailedEntry> | number | undefined): string {
    if (!failed || typeof failed === "number") return "";
    if (failed.length === 0) return "";
    // First failure with its error gives the user actionable context;
    // remaining ones are summarised by name only to keep the toast short.
    const [first, ...rest] = failed;
    const firstStr = first.name
        ? first.error
            ? `${first.name} (${first.error})`
            : first.name
        : first.error ?? "unknown";
    if (rest.length === 0) return firstStr;
    const restNames = rest
        .map((f) => f.name)
        .filter((n): n is string => Boolean(n));
    if (restNames.length === 0) return `${firstStr} +${rest.length} more`;
    return `${firstStr}, ${summariseNames(restNames)}`;
}

export function formatMaintenanceSuccess(label: string, captured: unknown): string {
    if (!captured || typeof captured !== "object") return `${label} completed.`;
    const env = captured as BackendEnvelope<MaintenancePayload>;
    const data = (env.data ?? captured) as MaintenancePayload;
    if (!data || typeof data !== "object") return `${label} completed.`;

    const buckets: Array<{ name: string; b: MaintenanceBucket }> = [];
    if (data.manual) buckets.push({ name: "manual", b: data.manual });
    if (data.disable) buckets.push({ name: "disabled", b: data.disable });
    if (!buckets.length) return `${label} completed.`;

    const parts: string[] = [];
    for (const { name, b } of buckets) {
        const touched = b?.touched ?? 0;
        const alreadyOK = b?.alreadyOK ?? 0;
        const missing = b?.missing ?? 0;
        const failedCount = Array.isArray(b?.failed) ? b.failed.length : (b?.failed ?? 0);

        const segments: string[] = [`${touched} → ${name}`];
        if (touched > 0) {
            const list = summariseNames(b.touchedNames);
            if (list) segments.push(`(${list})`);
        }
        const tail: string[] = [];
        if (alreadyOK > 0) tail.push(`${alreadyOK} already OK`);
        if (missing > 0) tail.push(`${missing} missing`);
        if (failedCount > 0) {
            const failureSummary = summariseFailures(b.failed as FailedEntry[]);
            tail.push(failureSummary ? `${failedCount} failed: ${failureSummary}` : `${failedCount} failed`);
        }
        if (tail.length) segments.push(`(${tail.join(", ")})`);
        parts.push(segments.join(" "));
    }
    const ms = typeof data.durationMs === "number" ? ` in ${(data.durationMs / 1000).toFixed(1)}s` : "";
    return `${label}${ms}: ${parts.join(" · ")}`;
}
