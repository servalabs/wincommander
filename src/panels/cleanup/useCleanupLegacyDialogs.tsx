// The Wi-Fi profile viewer retained by System Cleanup.
// Browser activity is reviewed in Maintenance.
import { Dialog, Button } from "@/components/ui/bp";
import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState } from "react";
import { staggerDelay } from "../../components/shared/AnimatedList";
import { DURATION_S } from "../../components/shared/motion";
import useBackend, { type WlanProfile } from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";
import { type CardData, updateCacheEntry } from "./useCleanupScan";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

export function useCleanupLegacyDialogs(cardDataMap: Record<string, CardData>) {
    const { getWlanProfiles, removeWlanProfile } = useBackend();
    const requestConfirm = useAppConfirm();

    const [localLoadingMap, setLocalLoadingMap] = useState<Record<string, boolean>>({});

    const [wlanDialogOpen, setWlanDialogOpen] = useState(false);

    const [wlanProfiles, setWlanProfiles] = useState<WlanProfile[]>([]);

    const runLocalOnly = async (key: string, fn: () => Promise<unknown>) => {
        setLocalLoadingMap(prev => ({ ...prev, [key]: true }));
        try {
            if (fn) await fn();
        } finally {
            setLocalLoadingMap(prev => ({ ...prev, [key]: false }));
        }
    };

    const handleWlanProfiles = async () => {
        const cached = cardDataMap['wlanProfiles']?.raw;
        if (cached?.profiles) { setWlanProfiles(cached.profiles); setWlanDialogOpen(true); return; }
        await runLocalOnly('wlanProfiles', async () => {
            const res = await getWlanProfiles();
            if (res.success && res.data?.profiles) {
                setWlanProfiles(res.data.profiles);
                setWlanDialogOpen(true);
            } else {
                showError(res.error || "Failed to load WLAN profiles.");
            }
        });
    };

    const handleRemoveWlanProfile = async (name: string) => {
        await runLocalOnly('wlanRemove', async () => {
            const res = await removeWlanProfile(name);
            if (res.success) {
                setWlanProfiles(prev => prev.filter(p => p.name !== name));
            } else {
                showError(res.error || "Failed to remove WLAN profile.");
            }
        });
    };

    const handleEraseAllWlan = async () => {
        const accepted = await requestConfirm({
            title: "Forget all Wi-Fi networks?",
            description: "Every saved Wi-Fi profile will be removed and the current wireless connection may be lost.",
            confirmLabel: "Forget all networks",
        });
        if (!accepted) return;
        await runLocalOnly('wlanErase', async () => {
            const res = await removeWlanProfile("");
            if (res.success) {
                setWlanProfiles([]);
                updateCacheEntry('wlanProfiles', { count: 0, items: [], loading: false, clearing: false });
                showSuccess("All Wi-Fi profiles cleared.");
            } else {
                showError(res.error || "Failed to clear Wi-Fi profiles.");
            }
        });
    };

    useEffect(() => {
        const closeAll = () => { setWlanDialogOpen(false); };
        window.addEventListener("commander-dismiss-dialogs", closeAll);
        return () => window.removeEventListener("commander-dismiss-dialogs", closeAll);
    }, []);

    const openers = { handleWlanProfiles };
    return { openers, dialogs: (<>
<Dialog
                isOpen={wlanDialogOpen}
                onClose={() => setWlanDialogOpen(false)}
                title="Wi-Fi Profiles"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div className="mb-3 p-2 bg-blue-500/10 text-blue-400 text-xs rounded border border-blue-500/20">
                        Passwords are shown in cleartext. Use with caution.
                    </div>
                    <div style={{ maxHeight: 360, overflow: "auto", paddingRight: "8px" }}>
                        <table className="wc-table wc-table--striped" style={{ width: "100%" }}>
                            <thead>
                                <tr>
                                    <th>SSID</th>
                                    <th>PASSWORD</th>
                                    <th style={{ width: 40 }}></th>
                                </tr>
                            </thead>
                            <AnimatePresence initial={false}>
                            <tbody>
                                {wlanProfiles.map((p, idx) => (
                                    <motion.tr
                                        key={p.name}
                                        initial={{ opacity: 0 }}
                                        animate={{ opacity: 1 }}
                                        exit={{ opacity: 0 }}
                                        transition={{ delay: staggerDelay(idx), duration: DURATION_S.fast }}
                                    >
                                        <td className="font-semibold" style={{ color: 'var(--color-text-primary)' }}>{p.name}</td>
                                        <td className="font-mono text-xs opacity-80" style={{ color: 'var(--color-text-muted)', wordBreak: "break-all" }}>{p.password || "<NO PASS>"}</td>
                                        <td>
                                            <Button
                                                icon="trash"
                                                small
                                                minimal
                                                intent="danger"
                                                aria-label={`Forget Wi-Fi profile ${p.name}`}
                                                onClick={() => handleRemoveWlanProfile(p.name)}
                                                loading={localLoadingMap["wlanRemove"]}
                                            />
                                        </td>
                                    </motion.tr>
                                ))}
                                {wlanProfiles.length === 0 && (
                                    <tr>
                                        <td colSpan={3} className="text-center opacity-50 py-4">No saved profiles found</td>
                                    </tr>
                                )}
                            </tbody>
                            </AnimatePresence>
                        </table>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button
                        icon="cross"
                        text="CLOSE"
                        onClick={() => setWlanDialogOpen(false)}
                        minimal
                        className="modal-cancel-btn"
                    />
                    <Button
                        icon="trash"
                        text="CLEAR ALL PROFILES"
                        onClick={handleEraseAllWlan}
                        loading={localLoadingMap["wlanErase"]}
                        className="modal-primary-btn danger"
                        disabled={wlanProfiles.length === 0}
                    />
                </div>
            </Dialog>


    </>) };
}
