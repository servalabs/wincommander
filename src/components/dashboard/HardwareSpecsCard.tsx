import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Cpu, MemoryStick, BatteryFull, ChevronDown, ChevronUp, Thermometer, Bell, BellOff } from "lucide-react";
import useBackend, { type SystemInfo, type BatteryHealthResult } from "../../hooks/useBackend";
import MetricAlertRow from "./MetricAlertRow";
import { useMetricAlerts } from "../../hooks/useMetricAlerts";

interface HardwareSpecsCardProps {
    systemInfo: SystemInfo | null;
    isLoading: boolean;
    metricsStatus?: "loading" | "live" | "stale";
    expanded?: boolean;
    onToggle?: () => void;
    /** Controlled by the dashboard so only one right-panel alert drawer is open. */
    alertOpen?: boolean;
    onAlertOpenChange?: (open: boolean) => void;
}

const BATTERY_REFRESH_INTERVAL_MS = 6 * 60 * 60 * 1000;

function batteryTone(pct: number): "good" | "warn" | "bad" {
    if (pct >= 80) return "good";
    if (pct >= 50) return "warn";
    return "bad";
}

function trimCpuName(cpu: string) {
    return cpu.replace(/\s*with Radeon.*?Graphics/i, '').trim();
}

function formatGb(value: number): string {
    if (value <= 0) return "—";
    const rounded = Math.round(value * 10) / 10;
    return `${Number.isInteger(rounded) ? rounded.toFixed(0) : rounded.toFixed(1)} GB`;
}

function usageColor(pct: number): string {
    if (pct >= 85) return 'var(--color-danger)';
    if (pct >= 60) return 'var(--color-warning)';
    return 'var(--color-success)';
}

function tempColor(celsius: number): string {
    if (celsius >= 80) return 'var(--color-danger)';
    if (celsius >= 65) return 'var(--color-warning)';
    return 'var(--color-text-muted)';
}

function isValidCpuTemp(temp: number | null | undefined): temp is number {
    return typeof temp === 'number' && temp > 30 && temp < 120;
}

/**
 * HardwareSpecsCard — CPU + Memory monitor.
 *   Collapsed: bar + percentage for CPU and RAM.
 *   Expanded: bar + percentage (same) PLUS a compact detail sub-row
 *   beneath each metric (model/temp, GB usage, battery health) — both
 *   the visual and the context in one tight block per metric.
 */
export default function HardwareSpecsCard({
    systemInfo,
    isLoading,
    metricsStatus = "loading",
    expanded = true,
    onToggle,
    alertOpen: controlledAlertOpen,
    onAlertOpenChange,
}: HardwareSpecsCardProps) {
    const { getBatteryHealth } = useBackend();
    const { config: alerts } = useMetricAlerts();
    const cpuAlertOn = !!alerts?.cpu.enabled;
    const ramAlertOn = !!alerts?.ram.enabled;
    const [uncontrolledAlertOpen, setUncontrolledAlertOpen] = useState(false);
    const [liveCpuTemp, setLiveCpuTemp] = useState<number | null>(null);
    const [battery, setBattery] = useState<BatteryHealthResult | null>(null);
    const warmRef = useRef(false);

    const alertOpen = controlledAlertOpen ?? uncontrolledAlertOpen;
    const setAlertOpen = onAlertOpenChange ?? setUncontrolledAlertOpen;

    useEffect(() => {
        let cancelled = false;
        const fetchBattery = async () => {
            try {
                const res = await getBatteryHealth();
                if (cancelled) return;
                if (res.success && res.data) setBattery(res.data);
            } catch { /* best-effort */ }
        };
        fetchBattery();
        const id = setInterval(fetchBattery, BATTERY_REFRESH_INTERVAL_MS);
        return () => { cancelled = true; clearInterval(id); };
    }, [getBatteryHealth]);

    useEffect(() => {
        let cancelled = false;
        const fetch = async () => {
            try {
                const metrics = await invoke<{ cpuTemp: number | null }>('get_live_metrics');
                if (!cancelled) setLiveCpuTemp(metrics.cpuTemp ?? null);
            } catch { /* hardware may not expose temp */ }
        };
        fetch();
        const warmTimer = setTimeout(() => {
            if (!cancelled && !warmRef.current) { warmRef.current = true; fetch(); }
        }, 1200);
        const id = setInterval(fetch, 10000);
        return () => { cancelled = true; clearTimeout(warmTimer); clearInterval(id); };
    }, []);

    if (isLoading || !systemInfo) {
        return (
            <div className="hardware-specs-card loading">
                <div className="card-header">
                    <Cpu size={14} className="icon-pulse" />
                    <span>SYSTEM INFO...</span>
                </div>
            </div>
        );
    }

    const cpuUsage = Math.min(100, Math.max(0, systemInfo.cpuUsage ?? 0));
    const ramUsage = Math.min(100, Math.max(0, systemInfo.ramUsage ?? 0));
    const hasBattery = battery?.status === "ok" && typeof battery.healthPct === "number";

    return (
        <div className="hardware-specs-card">
            <div className="card-header hw-card-header">
                <span className="sr-only" role="status">
                    {metricsStatus === "loading"
                        ? "System metrics are loading."
                        : metricsStatus === "stale"
                            ? "System metrics are showing the last available reading."
                            : "System metrics are current."}
                </span>
                <button
                    type="button"
                    className="hardware-card-toggle"
                    onClick={onToggle}
                    aria-expanded={expanded}
                >
                    <Cpu size={14} />
                    <span>SYSTEM INFO</span>
                    {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                </button>
                <button
                    type="button"
                    className={`hw-alert-btn ${cpuAlertOn || ramAlertOn ? "on" : ""}`}
                    onClick={() => setAlertOpen(!alertOpen)}
                    title={cpuAlertOn || ramAlertOn ? "Hardware alert on — configure" : "Configure CPU and RAM usage alerts"}
                    aria-label="CPU and RAM usage alerts"
                >
                    {cpuAlertOn || ramAlertOn ? <Bell size={13} /> : <BellOff size={13} />}
                </button>
            </div>

            {alertOpen && (
                <div className="hw-cpu-alert">
                    <MetricAlertRow
                        metric="cpu"
                        label="CPU"
                        unit="%"
                        reportToFleetMetrics={["cpu", "ram"]}
                        reportToFleetOnLabelRow
                    />
                    <MetricAlertRow metric="ram" label="RAM" unit="%" showReportToFleet={false} />
                </div>
            )}

            <div className="hw-bars">
                {/* ── CPU ── */}
                <div className="hw-bar-row">
                    <Cpu size={12} className="hw-bar-icon" />
                    <span className="hw-bar-label">CPU</span>
                    <div className="hw-bar-track">
                        <div className="hw-bar-fill" style={{ width: `${cpuUsage}%`, background: usageColor(cpuUsage) }} />
                    </div>
                    <span className="hw-bar-pct">{Math.round(cpuUsage)}%</span>
                </div>
                {expanded && systemInfo.cpu && (
                    <div className="hw-detail-sub">
                        <span className="hw-detail-text">{trimCpuName(systemInfo.cpu)}</span>
                        {isValidCpuTemp(liveCpuTemp) && (
                            <span className="hw-detail-badge" style={{ color: tempColor(liveCpuTemp) }}>
                                <Thermometer size={10} />
                                {Math.round(liveCpuTemp)}°C
                            </span>
                        )}
                    </div>
                )}

                {/* ── RAM ── */}
                <div className="hw-bar-row">
                    <MemoryStick size={12} className="hw-bar-icon" />
                    <span className="hw-bar-label">RAM</span>
                    <div className="hw-bar-track">
                        <div className="hw-bar-fill" style={{ width: `${ramUsage}%`, background: usageColor(ramUsage) }} />
                    </div>
                    <span className="hw-bar-pct">{Math.round(ramUsage)}%</span>
                </div>
                {expanded && (
                    <div className="hw-detail-sub">
                        <span className="hw-detail-text">
                            {formatGb(systemInfo.ramUsedGb ?? 0)} / {formatGb(systemInfo.ramTotalGb ?? 0)} used
                        </span>
                    </div>
                )}

                {/* ── Battery (expanded only, shown as health bar) ── */}
                {expanded && hasBattery && (() => {
                    const pct = battery!.healthPct as number;
                    const tone = batteryTone(pct);
                    return (
                        <>
                            <div className="hw-bar-row">
                                <BatteryFull size={12} className="hw-bar-icon" />
                                <span className="hw-bar-label">BATT</span>
                                <div className="hw-bar-track">
                                    <div
                                        className="hw-bar-fill"
                                        style={{
                                            width: `${pct}%`,
                                            background: tone === "good" ? "var(--color-success)" : tone === "warn" ? "var(--color-warning)" : "var(--color-danger)",
                                        }}
                                    />
                                </div>
                                <span className="hw-bar-pct">{pct.toFixed(0)}%</span>
                            </div>
                            <div className="hw-detail-sub">
                                <span className="hw-detail-text">
                                    {battery!.cycleCount != null
                                        ? `${battery!.cycleCount} cycles`
                                        : (battery!.chemistry ?? battery!.manufacturer ?? "battery")}
                                </span>
                                <span className={`disk-health-badge ${tone}`}>{tone}</span>
                            </div>
                        </>
                    );
                })()}
            </div>
        </div>
    );
}
