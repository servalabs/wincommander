import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import { Button, Tag, Spinner, InputGroup, HTMLSelect } from "@/components/ui/bp";
import SectionCard from "../../shared/SectionCard";
import { executeBackendCommand } from "../../../hooks/useBackend";
import { useServiceChanges } from "../../../hooks/useServiceChanges";
import ServiceSnapshots from "./ServiceSnapshots";
import { useAppState } from "../../../context/AppContext";
import { showSuccess, showError } from "../../../utils/toast";
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

export default function ServiceManager({ embedded = false, scanKey = 0 }: { embedded?: boolean; scanKey?: number }) {
    const { appSettings, patchAppSettings } = useAppState();
    const { changeServices, busy, dialog } = useServiceChanges();
    const [snapshotRevision, setSnapshotRevision] = useState(0);
    // Collapsed by default + lazy fetch — see StartupManager comment.
    const [isOpen, setIsOpen] = useState(false);
    const [rows, setRows] = useState<ServiceRow[]>([]);
    const [loading, setLoading] = useState(false);
    const [filter, setFilter] = useState("");
    const [pending, setPending] = useState<Set<string>>(new Set());
    const [showOnlyRecommended, setShowOnlyRecommended] = useState(false);
    const [applyingRecommended, setApplyingRecommended] = useState(false);
    const embeddedScanKey = useRef<number | undefined>(undefined);

    const refresh = useCallback(async () => {
        setLoading(true);
        const res = await executeBackendCommand<ServiceRow[]>("Get-AllServices");
        setLoading(false);
        if (res.success && Array.isArray(res.data)) setRows(res.data);
        else if (!res.success) showError(res.error || "Failed to load services");
    }, []);

    useEffect(() => {
        if (embedded) {
            if (embeddedScanKey.current !== scanKey) {
                embeddedScanKey.current = scanKey;
                void refresh();
            }
            return;
        }
        if (isOpen && rows.length === 0 && !loading) void refresh();
    }, [embedded, isOpen, loading, refresh, rows.length, scanKey]);

    const applyRecommended = useCallback(async () => {
        setApplyingRecommended(true);
        try {
            const result = await changeServices({ action: "Profile" });
            if (!result) return;
            setSnapshotRevision(value => value + 1);
            void refresh();
            if (!result.success) { showError(result.error || "Service profile failed."); return; }
            const previous = appSettings?.ideal?.tweaks?.maintenanceRuns?.services;
            await patchAppSettings({ ideal: { tweaks: { maintenanceRuns: {
                services: { lastRunAt: new Date().toISOString(), runCount: (previous?.runCount ?? 0) + 1 },
            } } } });
            showSuccess(result.data?.message || "Service settings verified.");
        } catch (error) {
            showError(error instanceof Error ? error.message : "Could not record the service profile result.");
        } finally { setApplyingRecommended(false); }
    }, [changeServices, refresh, appSettings?.ideal?.tweaks?.maintenanceRuns, patchAppSettings]);

    const changeRow = useCallback(async (svc: ServiceRow, mode?: string) => {
        setPending(prev => new Set(prev).add(svc.Name));
        try {
            const result = await changeServices({
                action: mode ? "SetStartMode" : svc.State === "Running" ? "Stop" : "Start",
                name: svc.Name, startMode: mode,
            });
            if (!result) return;
            setSnapshotRevision(value => value + 1);
            void refresh();
            if (result.success) showSuccess(result.data?.message || "Service settings verified.");
            else showError(result.error || "Could not change this service.");
        } finally { setPending(prev => { const next = new Set(prev); next.delete(svc.Name); return next; }); }
    }, [changeServices, refresh]);
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
                    loading={applyingRecommended} disabled={busy}
                    onClick={(e) => { e.stopPropagation(); applyRecommended(); }}
                    title="Apply WinCommander's recommended start mode (Manual / Disabled) to a curated set of services">
                    Apply Recommended Tweaks
                </Button>
                <Tag minimal>{`${filtered.length}/${rows.length}`}</Tag>
                {!embedded && <Button minimal icon="refresh" aria-label="Refresh services" onClick={(e) => { e.stopPropagation(); refresh(); }} loading={loading} />}
            </div>
        </>
    );

    const body = (
        <>
            <div className="system-manager-toolbar">
                <InputGroup aria-label="Search services" placeholder="Search services..." leftIcon="search" value={filter}
                    onChange={e => setFilter(e.currentTarget.value)} className="system-manager-toolbar-filter" />
                <Button small minimal active={showOnlyRecommended} icon="filter"
                    aria-pressed={showOnlyRecommended}
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
                                    aria-label={`Startup mode for ${svc.DisplayName}`}
                                    value={svc.StartMode === "Auto" ? "Automatic" : svc.StartMode}
                                    onChange={e => void changeRow(svc, e.currentTarget.value)}
                                    disabled={isPending || busy}
                                    options={START_MODES.map(m => ({ label: svc.Recommended === m ? `${m} ★` : m, value: m }))}
                                    fill
                                />
                                <Button small loading={isPending} disabled={busy || (!svc.CanStop && svc.State === "Running")}
                                    aria-label={`${svc.State === "Running" ? "Stop" : "Start"} ${svc.DisplayName}`}
                                    onClick={() => void changeRow(svc)}>
                                    {svc.State === "Running" ? "Stop" : "Start"}
                                </Button>
                            </div>
                        </AnimatedTableRow>
                    );
                })}
            </AnimatedList>
            <ServiceSnapshots revision={snapshotRevision} disabled={busy} />
            {dialog}
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
