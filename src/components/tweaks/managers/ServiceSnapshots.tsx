import { useEffect, useRef, useState } from "react";
import { Button, HTMLSelect, Spinner } from "@/components/ui/bp";
import { executeBackendCommand } from "../../../hooks/useBackend";
import { serviceSnapshotField, serviceSnapshotValue, type ServiceSnapshot, type ServiceSnapshotComparison } from "../../../lib/serviceChanges";

export default function ServiceSnapshots({ revision, disabled }: { revision: number; disabled: boolean }) {
    const [open, setOpen] = useState(false);
    const [snapshots, setSnapshots] = useState<ServiceSnapshot[]>([]);
    const [selected, setSelected] = useState("");
    const [comparison, setComparison] = useState<ServiceSnapshotComparison | null>(null);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    const [saved, setSaved] = useState(0);
    const generation = useRef(0);
    useEffect(() => {
        const current = ++generation.current;
        if (!open) return;
        setBusy(true);
        setComparison(null);
        setError("");
        void executeBackendCommand<{ snapshots: ServiceSnapshot[] }>("Get-ServiceSnapshots").then(result => {
            if (current !== generation.current) return;
            if (result.success && Array.isArray(result.data?.snapshots)) {
                setSnapshots(result.data.snapshots);
                setSelected(result.data.snapshots[0]?.id ?? "");
            } else { setError(result.error || "Service snapshots unavailable."); }
            setBusy(false);
        });
        return () => { generation.current = current + 1; };
    }, [open, revision, saved]);
    const save = async () => {
        setBusy(true);
        setError("");
        const result = await executeBackendCommand<ServiceSnapshot>("New-ServiceSnapshot");
        if (result.success) setSaved(value => value + 1);
        else { setError(result.error || "Could not save service settings."); setBusy(false); }
    };
    const compare = async () => {
        const current = generation.current;
        setBusy(true);
        setError("");
        setComparison(null);
        const result = await executeBackendCommand<ServiceSnapshotComparison>("Compare-ServiceSnapshot", { SnapshotId: selected });
        if (current !== generation.current) return;
        if (result.success && Array.isArray(result.data?.changes)) setComparison(result.data);
        else setError(result.error || "Could not compare service settings.");
        setBusy(false);
    };
    return (
        <details className="mt-3 text-xs" onToggle={event => setOpen(event.currentTarget.open)}>
            <summary className="cursor-pointer py-2">Service changes</summary>
            {open && <div className="space-y-3 py-2">
                <p>Settings are saved automatically before WinCommander changes services. Compare a saved copy with this PC to investigate a problem. Snapshots compare settings; they do not restore them.</p>
                <div className="flex flex-wrap items-center gap-2">
                    <Button small disabled={busy || disabled} onClick={() => void save()}>Save settings now</Button>
                    {snapshots.length > 0 && <>
                        <HTMLSelect aria-label="Saved service settings" value={selected} disabled={busy || disabled}
                            onChange={event => { setSelected(event.currentTarget.value); setComparison(null); }}
                            options={snapshots.map(snapshot => ({ value: snapshot.id, label: `${new Date(snapshot.createdAt).toLocaleString()} · ${snapshot.note}` }))} />
                        <Button small disabled={!selected || busy || disabled} onClick={() => void compare()}>Compare with now</Button>
                    </>}
                    {busy && <Spinner size={16} />}
                </div>
                {!busy && !error && snapshots.length === 0 && <p>No snapshots yet. WinCommander will save one before its next service change.</p>}
                {error && <p role="alert" className="text-[var(--danger)]">{error}</p>}
                {comparison && <>
                    <p role="status">{comparison.configurationCount} setting changes · {comparison.runtimeCount} running-state changes · {comparison.unavailableCount} fields unavailable</p>
                    {comparison.excludedPerUserCount > 0 && <p>Temporary per-user service copies are excluded from this comparison.</p>}
                    {comparison.changes.length > 0 && <div className="max-h-72 overflow-auto">
                        <table className="w-full text-left">
                            <thead><tr><th>Service</th><th>Setting</th><th>Saved</th><th>Now</th></tr></thead>
                            <tbody>{comparison.changes.map(change => <tr key={`${change.name}:${change.field}`}>
                                <td className="align-top p-1 break-all">{change.name}</td>
                                <td className="align-top p-1">{serviceSnapshotField(change.field)}</td>
                                <td className="align-top p-1 break-all">{serviceSnapshotValue(change.before)}</td>
                                <td className="align-top p-1 break-all">{serviceSnapshotValue(change.after)}</td>
                            </tr>)}</tbody>
                        </table>
                    </div>}
                </>}
            </div>}
        </details>
    );
}
