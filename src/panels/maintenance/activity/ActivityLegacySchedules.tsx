import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/bp";
import { useAppConfirm } from "@/components/shared/AppConfirmDialog";
import useBackend from "@/hooks/useBackend";
import useEntitlements from "@/hooks/useEntitlements";

const LEGACY_TIMERS: Record<string, string> = { browserFootprints: "Browser cleanup", officeMru: "Office history cleanup" };
type Schedule = { categoryId: string; taskName: string; enabled: boolean; intervalMinutes: number; targetUser: string | null; ownerAccount?: string | null };

/** Keep older configured timers visible without creating broad cleanup tasks. */
export default function ActivityLegacySchedules() {
    const backend = useBackend(), backendRef = useRef(backend);
    backendRef.current = backend;
    const { hasPaid, isInvestigator } = useEntitlements();
    const requestConfirm = useAppConfirm();
    const [schedules, setSchedules] = useState<Schedule[]>([]);
    const [error, setError] = useState("");
    const [busy, setBusy] = useState(false);
    useEffect(() => {
        let active = true;
        void backendRef.current.getAutoEraseSchedules().then(response => {
            if (!active) return;
            if (response.success && response.data) setSchedules(response.data.schedules.filter(row => Object.hasOwn(LEGACY_TIMERS, row.categoryId)));
            else setError(response.error ?? "Existing cleanup schedules could not be checked.");
        }).catch(cause => { if (active) setError(String(cause)); });
        return () => { active = false; };
    }, []);
    async function disable(schedule: Schedule) {
        if (busy || !hasPaid || isInvestigator || schedule.targetUser !== null || !Object.hasOwn(LEGACY_TIMERS, schedule.categoryId)) return;
        if (!await requestConfirm({ title: "Disable this older cleanup timer?", description: `Disable current-user and SYSTEM timers for ${LEGACY_TIMERS[schedule.categoryId]}: ${schedules.filter(row => row.categoryId === schedule.categoryId && row.targetUser === null).map(row => row.taskName).join(", ")}. This stops future runs; a running cleanup may finish. Existing records are kept.`, confirmLabel: "Disable timer" })) return;
        setBusy(true); setError("");
        try {
            const response = await backendRef.current.removeAutoEraseSchedule(schedule.categoryId);
            if (response.success) {
                const refreshed = await backendRef.current.getAutoEraseSchedules();
                if (refreshed.success && refreshed.data) setSchedules(refreshed.data.schedules.filter(row => Object.hasOwn(LEGACY_TIMERS, row.categoryId)));
                else setError(refreshed.error ?? "Timer removal returned success; its current state could not be refreshed.");
            } else setError(response.error ?? "The timer could not be disabled.");
        } catch (cause) { setError(String(cause)); }
        finally { setBusy(false); }
    }
    if (!schedules.length && !error) return null;
    return <details className="activity-legacy"><summary>Existing automatic cleanup timers</summary>
        <p>These older timers clear whole stores independently of record review. Moving these tools has not changed an existing timer. Disabling a timer preserves its current data.</p>
        {error && <p role="alert" className="activity-error">{error}</p>}
        {schedules.map(schedule => <div key={schedule.taskName}><strong>{LEGACY_TIMERS[schedule.categoryId]}</strong><p>{schedule.taskName} · {schedule.enabled ? "Enabled" : "Disabled"} · {schedule.intervalMinutes > 0 ? `${schedule.intervalMinutes} minutes` : "Custom schedule"}{schedule.targetUser ? ` · Other Windows user: ${schedule.targetUser}` : ` · ${schedule.ownerAccount ?? "Current user or SYSTEM"}`}</p>
            <Button small outlined disabled={busy || !hasPaid || isInvestigator || schedule.targetUser !== null} onClick={() => void disable(schedule)}>Disable current-user/SYSTEM timers</Button>{schedule.targetUser !== null && <small>Review this other-user timer in Windows Task Scheduler.</small>}</div>)}
    </details>;
}
