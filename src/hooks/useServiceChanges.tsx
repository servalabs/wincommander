import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Dialog, DialogBody, DialogFooter } from "../components/ui/bp";
import { executeBackendCommand } from "./useBackend";
import { runServiceChange, serviceStepLabel, type ServiceChangePlan, type ServiceChangeRequest } from "../lib/serviceChanges";

export function useServiceChanges() {
    const [preview, setPreview] = useState<ServiceChangePlan | null>(null);
    const [busy, setBusy] = useState(false);
    const active = useRef(false);
    const mounted = useRef(true);
    const resolve = useRef<((accepted: boolean) => void) | null>(null);
    const settle = useCallback((accepted: boolean) => {
        const callback = resolve.current;
        resolve.current = null;
        setPreview(null);
        callback?.(accepted);
    }, []);
    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; resolve.current?.(false); resolve.current = null; };
    }, []);
    const changeServices = useCallback(async (request: ServiceChangeRequest) => {
        if (active.current) return { success: false as const, error: "Finish the current service preview first." };
        active.current = true;
        setBusy(true);
        try {
            return await runServiceChange(request, executeBackendCommand, plan => {
                if (!mounted.current) return Promise.resolve(false);
                return new Promise<boolean>(accept => { resolve.current = accept; setPreview(plan); });
            });
        } finally {
            active.current = false;
            if (mounted.current) setBusy(false);
        }
    }, []);
    const dialog = (
        <Dialog isOpen={preview !== null} onClose={() => settle(false)} title="Review service changes" style={{ width: 620 }}>
            <DialogBody>
                <p>WinCommander will save the current service settings before making these changes.</p>
                {preview?.warnings.map(warning => <p key={warning} className="text-[var(--warning)]">{warning}</p>)}
                {preview?.action === "Profile" && <p className="my-3">
                    {preview.steps.filter(step => step.action === "SetStartMode").length} startup settings will change.
                    {preview.steps.some(step => step.action === "Stop") && <span className="block mt-2">Will stop: {preview.steps.filter(step => step.action === "Stop").map(step => step.displayName || step.name).join(", ")}.</span>}
                </p>}
                <details open={preview?.action !== "Profile"}>
                    <summary className="cursor-pointer my-3">View all {preview?.steps.length ?? 0} changes</summary>
                    <ol className="list-decimal pl-5 space-y-2 my-4">
                    {preview?.steps.map((step, index) => (
                        <li key={`${index}:${step.name}`}>
                            <span>{serviceStepLabel(step)}</span>
                            <span className="block text-xs text-[var(--text-dim)]">{step.reason}</span>
                        </li>
                    ))}
                    </ol>
                </details>
                <p className="text-xs text-[var(--text-dim)]">If services change before you apply this preview, WinCommander will ask you to review it again.</p>
            </DialogBody>
            <DialogFooter>
                <Button onClick={() => settle(false)}>Cancel</Button>
                <Button intent="primary" onClick={() => settle(true)}>Apply these changes</Button>
            </DialogFooter>
        </Dialog>
    );
    return { changeServices, busy, dialog };
}
