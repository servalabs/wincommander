import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { HardDrive, Thermometer, ChevronDown } from "lucide-react";
import type { SystemInfo, DefenderStatus, UpdateStatus } from "../../hooks/useBackend";
import type { PanelId } from "../../types/panels";

interface ProcessMetric {
    name: string;
    cpuUsage: number;
    ramMb: number;
}

export interface ScoreImprovement {
    label: string;
    category: string;
    panelId: PanelId;
}

interface Props {
    isLoading: boolean;
    isSovereign: boolean;
    displayScore: number;
    finalScore: number;
    systemInfo: SystemInfo | null;
    defenderStatus: DefenderStatus | null;
    updateStatus: UpdateStatus | null;
    improvements?: ScoreImprovement[];
}

function RingGauge({ value, size = 68, strokeWidth = 6, color }: { value: number; size?: number; strokeWidth?: number; color: string }) {
    const r = (size - strokeWidth) / 2;
    const circ = 2 * Math.PI * r;
    const offset = circ * (1 - Math.min(100, Math.max(0, value)) / 100);
    return (
        <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
            <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--color-bg-tertiary)" strokeWidth={strokeWidth} />
            <circle
                cx={size / 2} cy={size / 2} r={r}
                fill="none" stroke={color} strokeWidth={strokeWidth}
                strokeDasharray={circ} strokeDashoffset={offset}
                strokeLinecap="round"
                transform={`rotate(-90 ${size / 2} ${size / 2})`}
                style={{ transition: 'stroke-dashoffset 0.8s cubic-bezier(0.4,0,0.2,1)', filter: `drop-shadow(0 0 3px ${color}80)` }}
            />
        </svg>
    );
}

function parseRamGb(ramStr: string | null | undefined): number {
    if (!ramStr) return 0;
    const n = parseFloat(ramStr);
    return isNaN(n) ? 0 : n;
}

function formatGb(value: number): string {
    if (value <= 0) return '—';
    const rounded = Math.round(value * 10) / 10;
    return `${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)} GB`;
}

function formatRamMb(value: number): string {
    if (value >= 1024) {
        const gb = value / 1024;
        return `${gb >= 10 ? gb.toFixed(0) : gb.toFixed(1)} GB`;
    }
    return `${Math.round(value)} MB`;
}

function trimCpuName(cpu: string) {
    return cpu
        .replace(/\s*with Radeon.*?Graphics/i, '')
        .replace(/\([Cc][^)]*\)/g, '')
        .replace(/@.*$/, '')
        .replace(/\s+/g, ' ')
        .trim();
}

function usageColor(pct: number): string {
    if (pct > 85) return 'var(--color-danger)';
    if (pct > 60) return 'var(--color-warning)';
    return 'var(--color-accent)';
}

function processUsageColor(pct: number): string {
    if (pct >= 25) return 'var(--color-danger)';
    if (pct >= 10) return 'var(--color-warning)';
    return 'var(--color-accent)';
}

const CATEGORY_LABEL: Record<string, string> = {
    telemetry: 'Telemetry',
    surface: 'Surface Tracking',
    hardening: 'Hardening',
    capabilities: 'Capabilities',
};

function Accordion({ title, count, children, open, onToggle }: {
    title: string;
    count?: number;
    children: React.ReactNode;
    open: boolean;
    onToggle: () => void;
}) {
    return (
        <div className="dsp-accordion">
            <button
                type="button"
                className="dsp-accordion-header"
                onClick={onToggle}
                aria-expanded={open}
            >
                <span className="dsp-acc-title">{title}</span>
                {count != null && count > 0 && (
                    <span className="dsp-acc-count">{count}</span>
                )}
                <ChevronDown
                    size={12}
                    className="dsp-acc-chevron"
                    style={{ transform: open ? 'rotate(180deg)' : 'rotate(0deg)' }}
                />
            </button>
            {open && <div className="dsp-accordion-body">{children}</div>}
        </div>
    );
}

export default function DashboardSidePanel({
    isLoading, isSovereign, displayScore, finalScore,
    systemInfo, defenderStatus, updateStatus, improvements = [],
}: Props) {
    const [processes, setProcesses] = useState<ProcessMetric[]>([]);
    const warmRef = useRef(false);
    const [openAccordion, setOpenAccordion] = useState<'security' | 'processes' | null>(null);
    const toggleAccordion = (id: 'security' | 'processes') =>
        setOpenAccordion(prev => prev === id ? null : id);

    useEffect(() => {
        let cancelled = false;

        const fetch = async () => {
            try {
                const result = await invoke<ProcessMetric[]>('get_top_processes', { limit: 8 });
                if (!cancelled) setProcesses(result.filter(p => p.cpuUsage > 0.1));
            } catch {
                // best-effort
            }
        };

        // Double-poll on mount: sysinfo needs two snapshots for accurate CPU deltas
        fetch();
        const warmTimer = setTimeout(() => {
            if (!cancelled && !warmRef.current) {
                warmRef.current = true;
                fetch();
            }
        }, 1200);

        const id = setInterval(fetch, 6000);
        return () => {
            cancelled = true;
            clearTimeout(warmTimer);
            clearInterval(id);
        };
    }, []);

    const scoreColor = isLoading
        ? 'var(--color-warning)'
        : isSovereign ? 'var(--color-accent)' : 'var(--color-danger)';

    const cpuUsage = systemInfo?.cpuUsage ?? 0;
    const cpuTemp = systemInfo?.cpuTemp ?? 0;
    const ramUsedGb = systemInfo?.ramUsedGb ?? 0;
    const ramTotalGb = systemInfo?.ramTotalGb ?? parseRamGb(systemInfo?.ram);
    const ramUsage = ramUsedGb > 0 && ramTotalGb > 0
        ? Math.min(100, Math.round((ramUsedGb / ramTotalGb) * 100))
        : systemInfo?.ramUsage ?? 0;

    const cpuColor = usageColor(cpuUsage);
    const ramColor = usageColor(ramUsage);
    const tempColor = cpuTemp > 85 ? 'var(--color-danger)' : cpuTemp > 70 ? 'var(--color-warning)' : 'var(--color-text-muted)';

    const disks = (Array.isArray(systemInfo?.disks) ? systemInfo!.disks : [])
        .filter(d => !(d.totalGb === 0 && d.freeGb === 0 && d.percent === 0));

    const maxCpu = processes.length > 0 ? Math.max(...processes.map(p => Math.min(100, p.cpuUsage)), 1) : 1;

    const navigateTo = (panelId: PanelId) => {
        window.dispatchEvent(new CustomEvent('navigate-panel', { detail: panelId }));
    };

    return (
        <div className="dash-side-panel">
            {/* ── Brand + Score ─────────────────────────────── */}
            <div className="dsp-brand">
                <span className="dsp-brand-name">WINCOMMANDER</span>
                <div className="dsp-score-row">
                    <div className="dsp-score-indicator" style={{ background: scoreColor, boxShadow: `0 0 7px ${scoreColor}` }} />
                    <div>
                        <div className="dsp-status-label">{isLoading ? 'CHECKING...' : isSovereign ? 'HEALTHY' : 'NEEDS REVIEW'}</div>
                        <div className="dsp-score-value" style={{ color: scoreColor }}>
                            {isLoading ? `${displayScore}%` : `${finalScore}%`}
                        </div>
                    </div>
                </div>
            </div>

            {/* Accordion: security improvements — directly under score */}
            {improvements.length > 0 && (
                <Accordion title="INCREASE YOUR SECURITY" count={improvements.length}
                    open={openAccordion === 'security'} onToggle={() => toggleAccordion('security')}>
                    <div className="dsp-improvements">
                        {improvements.map(imp => (
                            <button
                                key={imp.label}
                                type="button"
                                className="dsp-imp-row"
                                onClick={() => navigateTo(imp.panelId)}
                                title={`Go to ${imp.panelId} settings`}
                            >
                                <div className="dsp-imp-left">
                                    <span className="dsp-imp-cat">{CATEGORY_LABEL[imp.category] ?? imp.category}</span>
                                    <span className="dsp-imp-label">{imp.label}</span>
                                </div>
                                <span className="dsp-imp-arrow">›</span>
                            </button>
                        ))}
                    </div>
                </Accordion>
            )}

            <div className="dsp-divider" />

            {/* ── Security ──────────────────────────────────── */}
            <div className="dsp-section-label">SECURITY</div>
            <div className="dsp-security-grid">
                <div className="dsp-security-item">
                    <div className="dsp-sec-tag">DEFENDER</div>
                    <div className="dsp-sec-val"
                        style={{ color: defenderStatus == null ? 'var(--color-text-muted)' : defenderStatus.realtimeEnabled ? 'var(--color-danger)' : 'var(--color-accent)' }}>
                        {defenderStatus == null ? '—' : defenderStatus.realtimeEnabled ? 'ON' : 'OFF'}
                    </div>
                </div>
                <div className="dsp-security-item">
                    <div className="dsp-sec-tag">UPDATES</div>
                    <div className="dsp-sec-val"
                        style={{ color: updateStatus == null ? 'var(--color-text-muted)' : updateStatus.paused ? 'var(--color-accent)' : 'var(--color-danger)' }}>
                        {updateStatus == null ? '—' : updateStatus.paused ? 'PAUSED' : 'ACTIVE'}
                    </div>
                </div>
            </div>

            <div className="dsp-divider" />

            {/* ── CPU ───────────────────────────────────────── */}
            <div className="dsp-section-label">PROCESSOR</div>
            <div className="dsp-resource-row">
                <div className="dsp-ring-wrap">
                    <RingGauge value={cpuUsage} size={68} strokeWidth={6} color={cpuColor} />
                    <span className="dsp-ring-center" style={{ color: cpuColor }}>{cpuUsage}%</span>
                </div>
                <div className="dsp-resource-info">
                    <div className="dsp-resource-name">{systemInfo?.cpu ? trimCpuName(systemInfo.cpu) : (isLoading ? 'Loading...' : '—')}</div>
                    <div className="dsp-cpu-meta">
                        <span className="dsp-resource-sub">CPU USAGE</span>
                        {cpuTemp > 0 && (
                            <span className="dsp-temp-badge" style={{ color: tempColor }}>
                                <Thermometer size={12} />
                                {cpuTemp}°C
                            </span>
                        )}
                    </div>
                    <div className="dsp-bar-wrap">
                        <div className="dsp-bar-fill" style={{ width: `${cpuUsage}%`, background: cpuColor }} />
                    </div>
                </div>
            </div>

            {/* Accordion: top processes — directly under CPU */}
            <Accordion title="RUNNING PROCESSES" count={processes.length}
                open={openAccordion === 'processes'} onToggle={() => toggleAccordion('processes')}>
                {processes.length === 0 ? (
                    <div className="dsp-empty">Collecting data...</div>
                ) : (
                    <div className="dsp-processes">
                        {processes.map(p => {
                            const cpu = Math.min(100, Math.max(0, p.cpuUsage));
                            const procColor = processUsageColor(cpu);
                            return (
                                <div key={p.name} className="dsp-proc-row">
                                    <span className="dsp-proc-name">{p.name}</span>
                                    <div className="dsp-proc-bar-wrap">
                                        <div
                                            className="dsp-proc-bar-fill"
                                            style={{
                                                width: `${Math.round(cpu / maxCpu * 100)}%`,
                                                background: procColor,
                                            }}
                                        />
                                    </div>
                                    <span className="dsp-proc-pct" style={{ color: procColor }}>
                                        {cpu.toFixed(1)}%
                                    </span>
                                    <span className="dsp-proc-ram">{formatRamMb(p.ramMb)}</span>
                                </div>
                            );
                        })}
                    </div>
                )}
            </Accordion>

            <div className="dsp-divider" />

            {/* ── RAM ───────────────────────────────────────── */}
            <div className="dsp-section-label">MEMORY</div>
            <div className="dsp-ram-section">
                <div className="dsp-ram-stats-row">
                    <span className="dsp-ram-used" style={{ color: ramColor }}>{formatGb(ramUsedGb)}</span>
                    <span className="dsp-ram-sep">/</span>
                    <span className="dsp-ram-total">{ramTotalGb > 0 ? formatGb(ramTotalGb) : systemInfo?.ram || '—'}</span>
                    <span className="dsp-ram-pct" style={{ color: ramColor }}>{ramUsage}%</span>
                </div>
                <div className="dsp-bar-wrap dsp-bar-wrap--lg">
                    <div className="dsp-bar-fill" style={{ width: `${ramUsage}%`, background: ramColor }} />
                </div>
            </div>

            <div className="dsp-divider" />

            {/* ── Storage ───────────────────────────────────── */}
            <div className="dsp-section-label">STORAGE</div>
            {disks.length === 0 ? (
                <div className="dsp-empty">{isLoading ? 'Loading drives...' : 'No drives detected'}</div>
            ) : (
                <div className="dsp-disks-compact">
                    {disks.map(disk => {
                        const diskColor = disk.percent > 90 ? 'var(--color-danger)' : disk.percent > 75 ? 'var(--color-warning)' : 'var(--color-accent)';
                        return (
                            <div key={disk.id} className="dsp-disk-compact">
                                <div className="dsp-disk-compact-row">
                                    <HardDrive size={9} className="dsp-disk-icon" />
                                    <span className="dsp-disk-label">{disk.label}</span>
                                    <span className="dsp-disk-pct" style={{ color: diskColor }}>{disk.percent}%</span>
                                    <span className="dsp-disk-free">{disk.freeGb} GB free</span>
                                </div>
                                <div className="dsp-disk-bar">
                                    <div className="dsp-disk-bar-fill" style={{ width: `${disk.percent}%`, background: diskColor }} />
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
}
