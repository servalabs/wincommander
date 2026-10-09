import { useState } from "react";
import { Button, Classes, Dialog, Icon } from "@/components/ui/bp";
import { useAppState } from "../../context/AppContext";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";
import type { PhysicalNetworkAdapter } from "../../hooks/useBackend";
import { adapterMacDescription, formatAdapterMac, type AdapterAction } from "./adapterState";
import { useAdapterInventory } from "./useAdapterInventory";

export default function AdapterControls() {
    const { systemInfo } = useAppState();
    const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
    const { inventory, error, loading, busyId, operationMessage, refresh, change } = useAdapterInventory();
    const [pending, setPending] = useState<{ adapter: PhysicalNetworkAdapter; action: AdapterAction } | null>(null);
    const adapters = inventory?.adapters ?? [];
    const disabled = needsElevation || !!busyId || !!error || !inventory;

    return (
        <section className="merged-segment merged-segment--adapters">
            <header className="merged-segment__header">
                <Icon icon="globe-network" size={11} />
                <span className="merged-segment__title">Network Adapters</span>
                <span className="merged-segment__count">{error ? "Unavailable" : inventory ? `${adapters.filter(a => a.status === "Up").length} up` : "—"}</span>
                <Button small minimal icon="refresh" disabled={loading || !!busyId} onClick={() => void refresh()}>Refresh</Button>
            </header>
            <p className="merged-segment__blurb">Change the address visible to your local network. Driver support varies; this does not prevent all tracking.</p>
            <div className="merged-segment__body">
                {needsElevation && <p role="alert" className="text-xs">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}
                {error && <p role="alert" className="text-xs">{error} {inventory && "The list below is the last successful snapshot."}</p>}
                {loading && !inventory && <p role="status" className="text-xs">Reading Windows adapters…</p>}
                {inventory?.checkedAt && <p className="text-xs text-[var(--color-text-muted)]">Last checked: {new Date(inventory.checkedAt).toLocaleTimeString()}</p>}
                {!error && inventory && adapters.length === 0 && <p className="text-xs">No physical network adapters are present.</p>}
                {!error && adapters.length > 0 && adapters.every(a => a.status !== "Up") && <p className="text-xs">Adapters are present; none currently has an active link.</p>}
                <div className="adapter-list">
                    {adapters.map(adapter => (
                        <div key={adapter.id} className={`adapter-row ${adapter.status === "Up" ? "adapter-row--active" : "adapter-row--inactive"}`} style={{ flexWrap: "wrap" }}>
                            <Icon icon={adapter.kind === "wifi" ? "cell-tower" : "globe-network"} size={16} />
                            <div className="adapter-row__identity">
                                <div className="flex items-center gap-2"><span className="font-mono text-xs font-bold">{adapter.name}</span><span className="adapter-status-chip">{adapter.status}</span></div>
                                <div className="text-xs">Current: {formatAdapterMac(adapter.currentMac)}</div>
                                <div className="text-xs">Permanent: {formatAdapterMac(adapter.factoryMac)}</div>
                                {adapter.configuredMac && <div className="text-xs">Configured: {formatAdapterMac(adapter.configuredMac)}</div>}
                                <div className="text-xs text-[var(--color-text-muted)]">{adapterMacDescription(adapter)}</div>
                                {adapter.configuredMode === "rotate-on-launch" && <div className="text-xs">Legacy rotation setting detected. Automatic rotation is unavailable; use a one-time change or restore.</div>}
                            </div>
                            <div className="flex flex-wrap gap-1">
                                <Button small disabled={disabled || !!adapter.recoveryPending || !!adapter.configurationError} onClick={() => setPending({ adapter, action: "randomize" })}>Randomize once</Button>
                                <Button small disabled={disabled || !!adapter.recoveryPending || !!adapter.configurationError} onClick={() => setPending({ adapter, action: "factory" })}>Restore factory</Button>
                                {adapter.recoveryPending && <Button small intent="warning" disabled={disabled} onClick={() => setPending({ adapter, action: "recover" })}>Undo interrupted change</Button>}
                            </div>
                            {busyId === adapter.id && <p role="status" className="text-xs">Applying and verifying…</p>}
                        </div>
                    ))}
                </div>
                {operationMessage && <p role="status" className="text-xs mt-2">{operationMessage}</p>}
            </div>
            <Dialog isOpen={!!pending} onClose={() => setPending(null)} title="Change adapter address" style={{ width: 460 }}>
                <div className={Classes.DIALOG_BODY}>
                    <p>{pending?.adapter.name} may disconnect while Windows restarts it. Continue only while physically at this PC with a way to restore its connection.</p>
                    <p className="mt-2">Known active remote-management connections are blocked. Other remote-access tools may not be detectable.</p>
                </div>
                <div className={Classes.DIALOG_FOOTER}>
                    <Button onClick={() => setPending(null)}>Cancel</Button>
                    <Button intent="primary" disabled={disabled} onClick={() => { const request = pending; setPending(null); if (request) void change(request.adapter, request.action); }}>I am at this PC — continue</Button>
                </div>
            </Dialog>
        </section>
    );
}
