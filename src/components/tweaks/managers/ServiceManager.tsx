import { useEffect, useState, useCallback, useMemo } from "react";
import { Button, Tag, Spinner, InputGroup, HTMLSelect } from "@/components/ui/bp";
import SectionCard from "../../shared/SectionCard";
import useBackend, { executeBackendCommand } from "../../../hooks/useBackend";
import { useAppState } from "../../../context/AppContext";
import { runOperation } from "../../../context/OperationContext";
import { showSuccess, showError } from "../../../utils/toast";
import { formatMaintenanceSuccess } from "../../../utils/maintenance";
import { enabledRowsFirst } from "./systemManagerSort";
import "./SystemManagers.css";
import { AnimatedList, AnimatedTableRow, staggerDelay } from "../../shared/AnimatedList";

interface ServiceRow {
    Name: string;
    DisplayName: string;
    Description: string | null;
    StartMode: string;             // "Auto" | "Manual" | "Disabled" | ...
    State: string;                  // "Running" | "Stopped" | ...
    Status: string;
    CanPauseAndContinue: boolean;
    CanStop: boolean;
    Recommended: string | null;     // "Disabled" | "Manual" | ...
}

const START_MODES = ["Automatic", "AutomaticDelayedStart", "Manual", "Disabled"];

export default function ServiceManager({ embedded = false }: { embedded?: boolean }) {
    const { appSettings, patchAppSettings } = useAppState();
    const { setServicesManual } = useBackend();
    // Collapsed by default + lazy fetch — see StartupManager comment.
    const [isOpen, setIsOpen] = useState(false);
    const [rows, setRows] = useState<ServiceRow[]>([]);
    const [loading, setLoading] = useState(false);
    const [filter, setFilter] = useState("");
    const [pending, setPending] = useState<Set<string>>(new Set());
    const [showOnlyRecommended, setShowOnlyRecommended] = useState(false);
    const [applyingRecommended, setApplyingRecommended] = useState(false);

    // "Apply Recommended Tweaks" is the granular Service Manager's
    // one-shot mode — calls the same Set-ServicesManual backend command
    // that used to live as a button in the System Maintenance card.
    // Moved here so the one-shot and the per-row controls are in the
    // same panel.
    const applyRecommended = useCallback(async () => {
        setApplyingRecommended(true);
        let captured: any = null;
        const wrapped = async () => {
            const r = await setServicesManual();
            captured = r;
            return r;
        };
        try {
            await runOperation(
                "Apply Recommended Service Profile",
                [{ label: "Optimizing services...", fn: wrapped }],
                { mode: "sequential", accent: "neutral" },
            );
            const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.["services"];
            await patchAppSettings({
                ideal: { tweaks: { maintenanceRuns: {
                    services: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
                } } },
            });
            showSuccess(formatMaintenanceSuccess("Apply Recommended Service Profile", captured));
            refresh();
        } catch {
            // runOperation already surfaces the error
        } finally {
            setApplyingRecommended(false);
        }
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [setServicesManual, appSettings?.ideal?.tweaks?.maintenanceRuns, patchAppSettings]);

    const refresh = useCallback(async () => {
        setLoading(true);
        const res = await executeBackendCommand<ServiceRow[]>("Get-AllServices");
        setLoading(false);
        if (res.success && Array.isArray(res.data)) setRows(res.data);
        else if (!res.success) showError(res.error || "Failed to load services");
    }, []);

    useEffect(() => {
        if ((isOpen || embedded) && rows.length === 0 && !loading) refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isOpen, embedded]);

    const setMode = useCallback(async (svc: ServiceRow, mode: string) => {
        setPending(prev => new Set(prev).add(svc.Name));
        const res = await executeBackendCommand<{ success?: boolean; message?: string }>("Set-ServiceStartMode", {
            Name: svc.Name, StartMode: mode,
        });
        setPending(prev => { const n = new Set(prev); n.delete(svc.Name); return n; });
        if (res.success && res.data?.success !== false) {
            showSuccess(`${svc.DisplayName || svc.Name} → ${mode}`);
            refresh();
        } else {
            showError(res.error || res.data?.message || `Could not change ${svc.Name}`);
        }
    }, [refresh]);

    const startStop = useCallback(async (svc: ServiceRow) => {
        setPending(prev => new Set(prev).add(svc.Name));
        const cmd = svc.State === "Running" ? "Stop-ServiceByName" : "Start-ServiceByName";
        const res = await executeBackendCommand<{ success?: boolean; message?: string }>(cmd, { Name: svc.Name });
        setPending(prev => { const n = new Set(prev); n.delete(svc.Name); return n; });
        if (res.success && res.data?.success !== false) {
            showSuccess(`${svc.State === "Running" ? "Stopped" : "Started"} ${svc.Name}`);
            refresh();
        } else {
            showError(res.error || res.data?.message || `Could not change state of ${svc.Name}`);
        }
    }, [refresh]);

    const filtered = useMemo(() => {
        const q = filter.trim().toLowerCase();
        const matches = rows.filter(r => {
            if (showOnlyRecommended && !r.Recommended) return false;
            if (!q) return true;
            return (r.DisplayName || "").toLowerCase().includes(q)
                || r.Name.toLowerCase().includes(q)
                || (r.Description || "").toLowerCase().includes(q);
        });
        return enabledRowsFirst(matches, svc => svc.StartMode !== "Disabled");
    }, [rows, filter, showOnlyRecommended]);

    const headerRight = (
        <>
            <span className="system-manager-caption">Windows services, live state, startup mode, stop/start ability, and recommendations.</span>
            <div className="system-manager-actions">
                <Button small intent="primary" icon="confirm"
                    loading={applyingRecommended}
                    onClick={(e) => { e.stopPropagation(); applyRecommended(); }}
                    title="Apply WinCommander's recommended start mode (Manual / Disabled) to a curated set of services">
                    Apply Recommended Tweaks
                </Button>
                <Tag minimal>{`${filtered.length}/${rows.length}`}</Tag>
                <Button minimal icon="refresh" onClick={(e) => { e.stopPropagation(); refresh(); }} loading={loading} />
            </div>
        </>
    );

    const body = (
        <>
            <div className="system-manager-toolbar">
                <InputGroup placeholder="Search services..." leftIcon="search" value={filter}
                    onChange={e => setFilter(e.currentTarget.value)} className="system-manager-toolbar-filter" />
                <Button small minimal active={showOnlyRecommended} icon="filter"
                    onClick={() => setShowOnlyRecommended(v => !v)}>Recommended only</Button>
            </div>
            <AnimatedList className="system-manager-list system-manager-list--services">
                {loading && rows.length === 0 && <Spinner size={20} />}
                {!loading && filtered.length === 0 && (
                    <div style={{ color: "var(--color-text-muted)", padding: 12, fontSize: 12 }}>No services match.</div>
                )}
                {filtered.map((svc, idx) => {
                    const isPending = pending.has(svc.Name);
                    return (
                        <AnimatedTableRow
                            key={svc.Name}
                            layoutId={svc.Name}
                            entranceDelay={staggerDelay(idx)}
                        >
                            <div
                                className="system-manager-row system-manager-row--service"
                                style={{
                                    // opacity dims disabled rows — compositor-only, no reflow
                                    opacity: svc.StartMode === "Disabled" ? 0.55 : 1,
                                    transition: "opacity var(--dur-normal) var(--ease)",
                                }}
                            >
                                <div style={{ minWidth: 0 }}>
                                    <div style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
                                        <span style={{ fontWeight: 600, fontSize: 12, minWidth: 0, overflowWrap: "anywhere" }}>{svc.DisplayName || svc.Name}</span>
                                        <span style={{ fontSize: 10, color: "var(--color-text-muted)", minWidth: 0, overflowWrap: "anywhere" }}>· {svc.Name}</span>
                                    </div>
                                    {svc.Description && (
                                        <div style={{ fontSize: 10, color: "var(--color-text-muted)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                                            {svc.Description}
                                        </div>
                                    )}
                                </div>
                                <Tag minimal intent={svc.State === "Running" ? "success" : "none"}>{svc.State}</Tag>
                                <HTMLSelect
                                    value={svc.StartMode === "Auto" ? "Automatic" : svc.StartMode}
                                    onChange={e => setMode(svc, e.currentTarget.value)}
                                    disabled={isPending}
                                    options={START_MODES.map(m => ({ label: svc.Recommended === m ? `${m} ★` : m, value: m }))}
                                    fill
                                />
                                <Button small loading={isPending} disabled={!svc.CanStop && svc.State === "Running"}
                                    onClick={() => startStop(svc)}>
                                    {svc.State === "Running" ? "Stop" : "Start"}
                                </Button>
                            </div>
                        </AnimatedTableRow>
                    );
                })}
            </AnimatedList>
        </>
    );

    if (embedded) {
        return (
            <div className="system-manager-embedded">
                <div className="system-manager-embedded__meta">{headerRight}</div>
                {body}
            </div>
        );
    }

    return (
        <SectionCard
            title="Service Manager"
            icon="settings"
            collapsible
            isOpen={isOpen}
            onToggle={() => setIsOpen(v => !v)}
            headerRight={headerRight}
        >
            {body}
        </SectionCard>
    );
}
