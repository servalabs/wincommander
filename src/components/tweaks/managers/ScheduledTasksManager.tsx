import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import { Button, Tag, Spinner, InputGroup, Switch, Icon } from "@/components/ui/bp";
import SectionCard from "../../shared/SectionCard";
import { executeBackendCommand } from "../../../hooks/useBackend";
import { showSuccess, showError } from "../../../utils/toast";
import "./ScheduledTasksManager.css";
import { enabledRowsFirst } from "./systemManagerSort";
import { AnimatedList, AnimatedTableRow, staggerDelay } from "../../shared/AnimatedList";
import { useAppConfirm } from "../../shared/AppConfirmDialog";

interface ScheduledTask {
    Name: string;
    Path: string;
    State: string;
    Description: string | null;
    Author: string | null;
    IsMicrosoft: boolean;
    LastRunTime: string | null;
    NextRunTime: string | null;
    LastResult: number | null;
}

function hasTaskAuthor(author: string | null): author is string {
    const normalized = author?.trim();
    return Boolean(normalized && normalized !== "\\");
}

export default function ScheduledTasksManager({ embedded = false, scanKey = 0 }: { embedded?: boolean; scanKey?: number }) {
    const requestConfirm = useAppConfirm();
    // Collapsed by default + lazy fetch — see StartupManager comment.
    const [isOpen, setIsOpen] = useState(false);
    const [tasks, setTasks] = useState<ScheduledTask[]>([]);
    const [loading, setLoading] = useState(false);
    const [pending, setPending] = useState<Set<string>>(new Set());
    const [filter, setFilter] = useState("");
    const [showMicrosoft, setShowMicrosoft] = useState(false);
    const embeddedScanKey = useRef<number | undefined>(undefined);

    const refresh = useCallback(async () => {
        setLoading(true);
        const res = await executeBackendCommand<ScheduledTask[]>("Get-AllScheduledTasks");
        setLoading(false);
        if (res.success && Array.isArray(res.data)) {
            setTasks(res.data);
        } else if (!res.success) {
            showError(res.error || "Failed to load scheduled tasks");
        }
    }, []);

    useEffect(() => {
        if (embedded) {
            if (embeddedScanKey.current !== scanKey) {
                embeddedScanKey.current = scanKey;
                void refresh();
            }
            return;
        }
        if (isOpen && tasks.length === 0 && !loading) void refresh();
    }, [embedded, isOpen, loading, refresh, scanKey, tasks.length]);

    const callAction = useCallback(async (cmd: string, t: ScheduledTask, verb: string) => {
        const key = t.Path + t.Name;
        setPending(prev => new Set(prev).add(key));
        const res = await executeBackendCommand<{ success?: boolean; message?: string }>(cmd, {
            Path: t.Path,
            Name: t.Name,
        });
        setPending(prev => { const n = new Set(prev); n.delete(key); return n; });
        if (res.success && res.data?.success !== false) {
            showSuccess(`${verb} ${t.Name}`);
            refresh();
        } else {
            showError(res.error || res.data?.message || `Failed to ${verb.toLowerCase()} ${t.Name}`);
        }
    }, [refresh]);

    const filtered = useMemo(() => {
        const q = filter.trim().toLowerCase();
        const matches = tasks.filter(t => {
            if (!showMicrosoft && t.IsMicrosoft) return false;
            if (!q) return true;
            return t.Name.toLowerCase().includes(q)
                || t.Path.toLowerCase().includes(q)
                || (t.Description || "").toLowerCase().includes(q);
        });
        return enabledRowsFirst(matches, task => task.State !== "Disabled");
    }, [tasks, filter, showMicrosoft]);

    const headerRight = (
        <>
            <span className="system-manager-caption">Scheduled jobs, Task Scheduler path, vendor signal, run state, and actions.</span>
            <div className="system-manager-actions">
                <Tag minimal>{`${filtered.length}/${tasks.length}`}</Tag>
                {!embedded && <Button minimal icon="refresh" aria-label="Refresh scheduled tasks" onClick={(e) => { e.stopPropagation(); refresh(); }} loading={loading} />}
            </div>
        </>
    );

    const body = (
        <>
            <div className="scheduled-tasks-manager__toolbar">
                <InputGroup
                    aria-label="Search scheduled tasks"
                    placeholder="Search by name / path..."
                    leftIcon="search"
                    value={filter}
                    onChange={e => setFilter(e.currentTarget.value)}
                    fill
                    className="scheduled-tasks-manager__filter"
                />
                <Switch
                    checked={showMicrosoft}
                    label="Show Microsoft tasks"
                    onChange={() => setShowMicrosoft(v => !v)}
                    className="scheduled-tasks-manager__switch"
                />
            </div>
            <div className="scheduled-tasks-manager__table-head">
                <span>Task / path / schedule details</span>
                <span>State</span>
                <span>Actions: Run now · Disable/Enable schedule · Delete permanently</span>
            </div>
            <AnimatedList className="scheduled-tasks-manager__list">
                {loading && tasks.length === 0 && <Spinner size={20} />}
                {!loading && filtered.length === 0 && (
                    <div style={{ color: "var(--color-text-muted)", padding: 12, fontSize: 12 }}>No tasks match.</div>
                )}
                {filtered.map((t, idx) => {
                    const key = t.Path + t.Name;
                    const isPending = pending.has(key);
                    const disabled = t.State === "Disabled";
                    return (
                        <AnimatedTableRow
                            key={key}
                            layoutId={key}
                            entranceDelay={staggerDelay(idx)}
                        >
                            <div
                                className="scheduled-tasks-manager__row"
                                style={{
                                    // opacity dims disabled rows — no height/left/width change
                                    opacity: disabled ? 0.6 : 1,
                                    transition: "opacity var(--dur-normal) var(--ease)",
                                }}
                            >
                                <div className="scheduled-tasks-manager__task">
                                    <div className="scheduled-tasks-manager__name">
                                        <span className="scheduled-tasks-manager__name-text">{t.Name}</span>
                                        {t.IsMicrosoft && <Icon icon="symbol-square" size={10} title="Microsoft-shipped task" />}
                                        {hasTaskAuthor(t.Author) && <span className="scheduled-tasks-manager__author" title={t.Author}>{t.Author}</span>}
                                    </div>
                                    <div className="scheduled-tasks-manager__path" title={t.Path}>
                                        {t.Path}
                                    </div>
                                    {(t.Description || t.LastRunTime || t.NextRunTime) && (
                                        <div className="scheduled-tasks-manager__detail">
                                            {t.Description && <span>{t.Description}</span>}
                                            {t.LastRunTime && <span>Last: {t.LastRunTime}</span>}
                                            {t.NextRunTime && <span>Next: {t.NextRunTime}</span>}
                                        </div>
                                    )}
                                </div>
                                <Tag className="scheduled-tasks-manager__state" minimal intent={disabled ? "none" : "primary"}>{t.State}</Tag>
                                <div className="scheduled-tasks-manager__actions">
                                    <Button small icon="play" disabled={disabled} loading={isPending} aria-label={`Run ${t.Name}`}
                                        onClick={() => callAction("Start-ScheduledTaskByPath", t, "Started")}>Run</Button>
                                    <Button small icon={disabled ? "tick" : "disable"} intent={disabled ? "success" : "danger"} loading={isPending} aria-label={`${disabled ? "Enable" : "Disable"} ${t.Name}`}
                                        onClick={() => callAction(disabled ? "Enable-ScheduledTaskByPath" : "Disable-ScheduledTaskByPath", t, disabled ? "Enabled" : "Disabled")}>
                                        {disabled ? "Enable" : "Disable"}
                                    </Button>
                                    <Button small icon="trash" intent="danger" loading={isPending} aria-label={`Delete ${t.Name}`}
                                        onClick={() => void (async () => {
                                            const accepted = await requestConfirm({
                                                title: "Delete scheduled task?",
                                                description: `Permanently delete “${t.Name}” at ${t.Path}? This cannot be undone.`,
                                                confirmLabel: "Delete task",
                                            });
                                            if (accepted) await callAction("Remove-ScheduledTaskByPath", t, "Removed");
                                        })()}>Delete</Button>
                                </div>
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
            title="Scheduled Tasks"
            icon="time"
            collapsible
            isOpen={isOpen}
            onToggle={() => setIsOpen(v => !v)}
            headerRight={headerRight}
        >
            {body}
        </SectionCard>
    );
}
