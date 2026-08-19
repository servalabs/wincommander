import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLicenseQuery } from "./queries/useLicenseQuery";

export interface ProHandshakeResult {
    ok: boolean;
    pro_version: string | null;
    error: string | null;
}

export interface FleetRuntimeStatus {
    connected: boolean;
    deviceId: string;
    serverUrl: string;
    lastEnrollAt: string | null;
    lastError: string | null;
    retrying: boolean;
    pendingApproval?: boolean;
}

export function useRuntimeDiagnostics() {
    const { data: license, isLoading: isLicenseLoading } = useLicenseQuery();
    const [proStatus, setProStatus] = useState<ProHandshakeResult | null>(null);
    const [isProLoading, setIsProLoading] = useState(false);
    const [fleetStatus, setFleetStatus] = useState<FleetRuntimeStatus | null>(null);
    const [fleetError, setFleetError] = useState<string | null>(null);
    const [isFleetLoading, setIsFleetLoading] = useState(false);

    const fleetFeatures = license?.active_service_features ?? license?.features ?? [];
    const isFleetEntitled = license?.valid === true && fleetFeatures.includes("fleet");

    const testProConnection = useCallback(async () => {
        setIsProLoading(true);
        try {
            const result = await invoke<ProHandshakeResult>("test_pro_handshake");
            setProStatus(result);
        } catch (error) {
            setProStatus({ ok: false, pro_version: null, error: String(error) });
        } finally {
            setIsProLoading(false);
        }
    }, []);

    const refreshFleet = useCallback(async () => {
        if (!isFleetEntitled) return;
        setIsFleetLoading(true);
        setFleetError(null);
        try {
            setFleetStatus(await invoke<FleetRuntimeStatus>("fleet_status"));
        } catch (error) {
            setFleetStatus(null);
            setFleetError(String(error));
        } finally {
            setIsFleetLoading(false);
        }
    }, [isFleetEntitled]);

    useEffect(() => {
        if (isFleetEntitled) void refreshFleet();
    }, [isFleetEntitled, refreshFleet]);

    return {
        proStatus,
        isProLoading,
        testProConnection,
        fleetStatus,
        fleetError,
        isFleetLoading,
        isFleetEntitled,
        isLicenseLoading,
        refreshFleet,
    };
}
