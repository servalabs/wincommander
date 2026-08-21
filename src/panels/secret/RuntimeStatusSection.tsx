import SectionCard from "../../components/shared/SectionCard";
import { Button } from "../../components/ui/button";
import { Icon } from "../../components/ui/icon";
import { useRuntimeDiagnostics, type FleetRuntimeStatus } from "../../hooks/useRuntimeDiagnostics";

type StatusTone = "neutral" | "success" | "warning" | "danger";

function fleetSummary(status: FleetRuntimeStatus | null, error: string | null) {
    if (error) return error;
    if (!status) return "No status received yet.";
    if (status.pendingApproval) return "Connected — awaiting administrator approval.";
    if (status.connected) return status.deviceId || "Connected.";
    if (status.retrying) return status.lastError || "Reconnecting to the Fleet server.";
    return status.lastError || "Disconnected.";
}

export default function RuntimeStatusSection() {
    const {
        proStatus,
        isProLoading,
        testProConnection,
        fleetStatus,
        fleetError,
        isFleetLoading,
        isFleetEntitled,
        isLicenseLoading,
        refreshFleet,
    } = useRuntimeDiagnostics();

    const proTone: StatusTone = proStatus === null
        ? "neutral"
        : proStatus.ok ? "success" : "danger";
    const fleetTone: StatusTone = !isFleetEntitled || fleetStatus === null
        ? "neutral"
        : fleetStatus.pendingApproval || fleetStatus.retrying
            ? "warning"
            : fleetStatus.connected ? "success" : "danger";
    const proDetail = proStatus === null
        ? "Not tested — the check starts a temporary Pro process."
        : proStatus.ok
            ? `Handshake passed${proStatus.pro_version ? ` — version ${proStatus.pro_version}` : ""}.`
            : proStatus.error || "Handshake failed.";
    const fleetDetail = isLicenseLoading
        ? "Checking Fleet entitlement…"
        : isFleetEntitled
            ? fleetSummary(fleetStatus, fleetError)
            : "Fleet isn't enabled for this license.";

    return (
        <SectionCard title="Runtime Status" icon="pulse" className="secret-runtime-card secret-grid__wide">
            <div className="secret-runtime-list">
                <div className="secret-runtime-row">
                    <span className={`secret-runtime-dot secret-runtime-dot--${proTone}`} aria-hidden="true" />
                    <div className="secret-runtime-copy">
                        <span className="secret-runtime-label">Pro connection</span>
                        <span className="secret-runtime-detail">{proDetail}</span>
                    </div>
                    <Button variant="outline" size="sm" disabled={isProLoading} onClick={testProConnection}>
                        <Icon icon="refresh" size={14} />
                        {isProLoading ? "Testing…" : "Test connection"}
                    </Button>
                </div>
                <div className="secret-runtime-row">
                    <span className={`secret-runtime-dot secret-runtime-dot--${fleetTone}`} aria-hidden="true" />
                    <div className="secret-runtime-copy">
                        <span className="secret-runtime-label">Fleet agent</span>
                        <span className="secret-runtime-detail">{fleetDetail}</span>
                        {fleetStatus?.connected && fleetStatus.serverUrl && (
                            <span className="secret-runtime-meta">{fleetStatus.serverUrl}</span>
                        )}
                    </div>
                    <Button
                        variant="outline"
                        size="sm"
                        disabled={isLicenseLoading || !isFleetEntitled || isFleetLoading}
                        onClick={refreshFleet}
                    >
                        <Icon icon="refresh" size={14} />
                        {isFleetLoading ? "Refreshing…" : "Refresh"}
                    </Button>
                </div>
            </div>
        </SectionCard>
    );
}
