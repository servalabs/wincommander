import { useState } from "react";
import { Icon } from "@/components/ui/bp";
import { useAppState } from "../context/AppContext";
import useBackend from "../hooks/useBackend";
import { runOperation } from "../context/OperationContext";
import { useTaskStatus } from "../context/TaskStatusContext";
import { claimFreeAppUpdates, clearAppUpdatesQueued, isAppUpdateQueued } from "../lib/appUpdateQueue";
import { releasePackageOperation, tryAcquirePackageOperation } from "../lib/packageOperationLock";

// Action button for "Update All Apps" — designed to live in the dashboard's
// fix-all-actions slot at the bottom of the radar. The pending-update count is
// rendered inside the radar itself (see RadarScanAnimation `pendingAppUpdates`),
// so the button stays clean: just a label + "Update All Apps".
export default function AppsUpdateButton({ compact = false }: { compact?: boolean }) {
    const { appInventory, refreshSettings, runAppInventoryScan } = useAppState();
    const { testWingetInstalled, installWinget, getAppInventory, upgradeApp } = useBackend();
    const { tasks: activeTasks } = useTaskStatus();
    const pendingAppUpdates = appInventory?.pendingUpdates?.length ?? 0;
    const [updating, setUpdating] = useState(false);
    // KT: Disabled while Fix Everything runs — it already covers app-update work,
    // so allowing this button would create a duplicate task + duplicate notification.
    const blockedByFixEverything = activeTasks.some(
        (t) => t.status === "running" && t.label === "Fix Everything"
    );

    if (pendingAppUpdates <= 0) return null;

    const handleUpdateAllApps = async () => {
        if (!tryAcquirePackageOperation()) return;
        setUpdating(true);
        // Claim the currently-known pending ids synchronously (before any await)
        // so the dashboard's Fix All / Needs Attention drops them immediately.
        const mine = claimFreeAppUpdates((appInventory?.pendingUpdates ?? []).map((u) => u.id || ""));
        try {
            const winget = await testWingetInstalled();
            if (!winget.success || winget.data?.status !== "installed") {
                const installRes = await installWinget();
                if (!installRes.success) {
                    throw new Error(installRes.error || "Package manager is not available.");
                }
            }

            const inventoryRes = await getAppInventory();
            if (!inventoryRes.success || !inventoryRes.data) {
                throw new Error(inventoryRes.error || "Failed to read app inventory.");
            }

            const queue = new Map<string, string>();
            (inventoryRes.data.pendingUpdates ?? []).forEach((item) => {
                const id = (item.id || "").trim();
                if (!id || queue.has(id)) return;
                // Skip ids already claimed by ANOTHER in-flight upgrade so the
                // same app isn't upgraded (or listed) twice.
                if (isAppUpdateQueued(id) && !mine.includes(id)) return;
                queue.set(id, (item.name || "").trim() || id);
            });
            // Claim any ids that appeared only in the fresh scan.
            mine.push(...claimFreeAppUpdates(Array.from(queue.keys())));
            const entries = Array.from(queue.entries());

            if (entries.length === 0) {
                await refreshSettings();
                return;
            }

            const steps = entries.map(([appId, name], index) => ({
                label: `[${index + 1}/${entries.length}] Upgrading ${name}`,
                fn: async () => {
                    const res = await upgradeApp(appId);
                    if (!res.success) {
                        throw new Error(res.error || `Upgrade failed for ${name}.`);
                    }
                },
            }));

            await runOperation(
                `Update ${entries.length} App${entries.length === 1 ? "" : "s"}`,
                steps,
                { mode: 'sequential', failFast: false, accent: 'blue' }
            );
            await runAppInventoryScan(true);
            await refreshSettings();
        } catch (err) {
            console.error("Update All Apps failed:", err);
        } finally {
            setUpdating(false);
            clearAppUpdatesQueued(mine);
            releasePackageOperation();
        }
    };

    if (compact) {
        return (
            <button
                type="button"
                className="na-footer-btn"
                onClick={handleUpdateAllApps}
                disabled={updating || blockedByFixEverything}
                title={blockedByFixEverything ? "Fix Everything is already updating apps" : "Update all pending app updates"}
            >
                <Icon icon="automatic-updates" size={14} />
                {updating ? "Updating..." : `Update ${pendingAppUpdates} App${pendingAppUpdates === 1 ? "" : "s"}`}
            </button>
        );
    }

    return (
        <button
            type="button"
            className="fix-all-btn"
            onClick={handleUpdateAllApps}
            disabled={updating || blockedByFixEverything}
        >
            <span className="fix-all-icon fix-all-icon--svg">
                <Icon icon="automatic-updates" size={22} />
            </span>
            <div className="fix-all-body">
                <div className="fix-all-title">{updating ? "Updating..." : "Update All Apps"}</div>
                <div className="fix-all-sub">apply pending app updates</div>
            </div>
        </button>
    );
}
