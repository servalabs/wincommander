import { useCallback, useEffect, useRef, useState } from "react";
import useBackend, { type PhysicalNetworkAdapter, type PhysicalNetworkAdaptersResult } from "../../hooks/useBackend";
import { adapterOperationMessage, type AdapterAction } from "./adapterState";
import { showError, showSuccess } from "../../utils/toast";

export function useAdapterInventory() {
    const { getPhysicalNetworkAdapters, setAdapterRandomMAC, restoreAdapterMAC } = useBackend();
    const [inventory, setInventory] = useState<PhysicalNetworkAdaptersResult | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [loading, setLoading] = useState(false);
    const [busyId, setBusyId] = useState<string | null>(null);
    const [operationMessage, setOperationMessage] = useState<string | null>(null);
    const mounted = useRef(true);
    const generation = useRef(0);
    const busy = useRef(false);
    const refreshing = useRef(false);
    const recoveryUntil = useRef(0);
    const invalidate = useCallback(() => { generation.current++; }, []);

    const refresh = useCallback(async () => {
        if (busy.current || refreshing.current) return;
        const request = ++generation.current;
        refreshing.current = true;
        setLoading(true);
        try {
            const result = await getPhysicalNetworkAdapters();
            if (!mounted.current || request !== generation.current) return;
            if (!result.success || result.data?.status !== "ok" || !Array.isArray(result.data.adapters)) {
                setError(result.error || result.data?.message || "Windows adapter inventory is unavailable.");
                return;
            }
            setInventory(result.data);
            setError(null);
        } finally {
            refreshing.current = false;
            if (mounted.current) setLoading(false);
        }
    }, [getPhysicalNetworkAdapters]);

    useEffect(() => {
        mounted.current = true;
        let timer: ReturnType<typeof setTimeout>;
        let stopped = false;
        const tick = async () => {
            if (document.visibilityState !== "hidden") await refresh();
            if (!stopped) timer = setTimeout(tick, Date.now() < recoveryUntil.current ? 2000 : 15000);
        };
        void tick();
        const focus = () => { void refresh(); };
        window.addEventListener("focus", focus);
        return () => { stopped = true; mounted.current = false; invalidate(); clearTimeout(timer); window.removeEventListener("focus", focus); };
    }, [refresh, invalidate]);

    const change = useCallback(async (adapter: PhysicalNetworkAdapter, action: AdapterAction) => {
        if (busy.current) return;
        busy.current = true;
        generation.current++;
        setBusyId(adapter.id);
        setOperationMessage("Applying settings and checking the actual address. The network link may reconnect.");
        try {
            const response = action === "randomize"
                ? await setAdapterRandomMAC(adapter.id, "static-random")
                : await restoreAdapterMAC(adapter.id, action === "recover");
            if (!mounted.current) return;
            const outcome = response.success && response.data
                ? adapterOperationMessage(response.data, action)
                : { verified: false, message: response.error || "The operation result is unavailable. Refresh to check the actual state." };
            setOperationMessage(outcome.message);
            if (outcome.verified) showSuccess(outcome.message); else showError(outcome.message);
            recoveryUntil.current = Date.now() + 30000;
        } finally {
            busy.current = false;
            if (mounted.current) { setBusyId(null); await refresh(); }
        }
    }, [setAdapterRandomMAC, restoreAdapterMAC, refresh]);

    return { inventory, error, loading, busyId, operationMessage, refresh, change };
}
