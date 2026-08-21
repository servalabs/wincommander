import { memo } from "react";
import { HardDrive, ChevronDown, ChevronUp } from "lucide-react";
import type { SystemInfo } from "../../hooks/useBackend";

interface StorageOverviewCardProps {
    systemInfo: SystemInfo | null;
    isLoading: boolean;
    expanded: boolean;
    onToggle: () => void;
}

function getHealthTone(healthPercent: number | null | undefined) {
    if (healthPercent == null) return "na";
    if (healthPercent >= 90) return "good";
    if (healthPercent >= 75) return "warn";
    return "bad";
}

const StorageOverviewCard = memo(function StorageOverviewCard({ systemInfo, isLoading, expanded, onToggle }: StorageOverviewCardProps) {
    if (isLoading || !systemInfo) {
        return (
            <div className="hardware-specs-card loading">
                <div className="card-header">
                    <HardDrive size={14} className="icon-pulse" />
                    <span>STORAGE...</span>
                </div>
            </div>
        );
    }

    const disks = (Array.isArray(systemInfo.disks) ? systemInfo.disks : [])
        .filter((disk) => !(disk.totalGb === 0 && disk.freeGb === 0 && disk.percent === 0));

    return (
        <div className="hardware-specs-card storage-overview-card">
            <div className="card-header">
                <button
                    type="button"
                    className="hardware-card-toggle"
                    onClick={onToggle}
                    aria-expanded={expanded}
                >
                    <HardDrive size={14} />
                    <span>STORAGE</span>
                    {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                </button>
            </div>

            {!expanded && disks.length > 0 && (
                <div className="hw-bars">
                    {disks.map((disk) => (
                        <div key={disk.id} className="hw-bar-row">
                            <HardDrive size={12} className="hw-bar-icon" />
                            <span className="hw-bar-label">{disk.label}</span>
                            <div className="hw-bar-track">
                                <div
                                    className="hw-bar-fill"
                                    style={{ width: `${disk.percent}%`, background: disk.percent > 90 ? "var(--color-danger)" : "var(--color-accent)" }}
                                />
                            </div>
                            <span className="hw-bar-pct">{disk.percent}%</span>
                        </div>
                    ))}
                </div>
            )}

            {expanded && (
                <div className="disk-grid">
                    {disks.length === 0 && (
                        <div className="storage-empty-state">No drive data available yet.</div>
                    )}
                    {disks.map((disk) => (
                        <div key={disk.id} className="disk-item">
                            <div className="disk-meta">
                                <div className="disk-id-group">
                                    <HardDrive size={12} className="disk-icon-small" />
                                    <span className="disk-id">{disk.label}</span>
                                </div>
                                {disk.healthPercent != null && (
                                    <span className={`disk-health-badge ${getHealthTone(disk.healthPercent)}`}>
                                        HEALTH {disk.healthPercent}%
                                    </span>
                                )}
                            </div>
                            <div className="disk-progress-container-thick">
                                <div
                                    className="disk-progress-bar-thick"
                                    style={{
                                        width: `${disk.percent}%`,
                                        background: disk.percent > 90 ? "var(--color-danger)" : "var(--color-accent)",
                                    }}
                                >
                                    {disk.percent > 18 && (
                                        <span className="inner-text used">{disk.usedGb}GB USED</span>
                                    )}
                                </div>
                                {disk.percent < 94 && (
                                    <span className="inner-text free">{disk.freeGb}GB FREE</span>
                                )}
                            </div>
                            <div className="disk-footer-simple">
                                <span className="disk-percent-badge">{disk.percent}% USED</span>
                                <span className="disk-total-label">{disk.totalGb}GB TOTAL</span>
                            </div>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
});

export default StorageOverviewCard;
