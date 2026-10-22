// Only the two bespoke viewers still used by SystemCleanupPanel.
// Active handlers and dialog JSX are retained unchanged from the legacy hook.
import { Dialog, Button, Icon } from "@/components/ui/bp";
import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState } from "react";
import { staggerDelay } from "../../components/shared/AnimatedList";
import { DURATION_S } from "../../components/shared/motion";
import useBackend, { type WlanProfile, type BrowserFootprintBrowser } from "../../hooks/useBackend";
import { showSuccess, showError } from "../../utils/toast";
import { type CardData, updateCacheEntry } from "./useCleanupScan";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

const formatSizeKB = (sizeKB: number) => {
    if (sizeKB >= 1024 * 1024) return `${(sizeKB / 1024 / 1024).toFixed(2)} GB`;
    if (sizeKB >= 1024) return `${(sizeKB / 1024).toFixed(1)} MB`;
    return `${Number(sizeKB || 0).toFixed(sizeKB >= 100 ? 0 : 1)} KB`;
};

export function useCleanupLegacyDialogs(cardDataMap: Record<string, CardData>) {
    const { getWlanProfiles, removeWlanProfile, getBrowserFootprints, clearBrowserFootprints } = useBackend();
    const requestConfirm = useAppConfirm();

    const [localLoadingMap, setLocalLoadingMap] = useState<Record<string, boolean>>({});

    const [wlanDialogOpen, setWlanDialogOpen] = useState(false);

    const [wlanProfiles, setWlanProfiles] = useState<WlanProfile[]>([]);

    const [browserFootprintsDialogOpen, setBrowserFootprintsDialogOpen] = useState(false);

    const [browserFootprints, setBrowserFootprints] = useState<BrowserFootprintBrowser[]>([]);

    const [browserFootprintsError, setBrowserFootprintsError] = useState<string | null>(null);

    const runLocalOnly = async (key: string, fn: () => Promise<any>) => {
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

    const handleBrowserFootprints = async () => {
        const cached = cardDataMap['browserFootprints']?.raw;
        if (cached?.browsers) {
            setBrowserFootprints(Array.isArray(cached.browsers) ? cached.browsers : []);
            setBrowserFootprintsError(null);
            setBrowserFootprintsDialogOpen(true);
            return;
        }
        setBrowserFootprintsDialogOpen(true);
        setBrowserFootprints([]);
        setBrowserFootprintsError(null);
        await runLocalOnly('browserFootprints', async () => {
            const res = await getBrowserFootprints();
            if (res.success && res.data?.browsers) {
                setBrowserFootprints(Array.isArray(res.data.browsers) ? res.data.browsers : []);
            } else {
                const message = res.error || "Failed to audit browser footprints.";
                setBrowserFootprintsError(message);
                showError(message);
            }
        });
    };

    const browserFootprintCacheKey = (b: BrowserFootprintBrowser) =>
        `browserFootprintsClear:${b.browser || ""}|${b.profilePath || ""}`;

    const syncBrowserFootprintsCache = (next: BrowserFootprintBrowser[]) => {
        updateCacheEntry('browserFootprints', {
            count: next.length,
            items: next.map((b) => b.browser),
            loading: false,
            clearing: false,
            raw: { browsers: next, totalBrowsers: next.length },
        });
    };

    const handleClearBrowserFootprints = async () => {
        await runLocalOnly('browserFootprintsClear', async () => {
            const res = await clearBrowserFootprints();
            if (res.success) {
                setBrowserFootprints([]);
                syncBrowserFootprintsCache([]);
                showSuccess("Browser footprints cleared.");
            } else {
                showError(res.error || "Failed to clear browser footprints.");
            }
        });
    };

    const handleClearBrowserFootprint = async (target: BrowserFootprintBrowser) => {
        await runLocalOnly(browserFootprintCacheKey(target), async () => {
            const res = await clearBrowserFootprints(target.browser, target.profilePath);
            if (res.success) {
                const next = browserFootprints.filter(
                    (b) => b.browser !== target.browser || b.profilePath !== target.profilePath
                );
                setBrowserFootprints(next);
                syncBrowserFootprintsCache(next);
                showSuccess(`Browser cleared: ${target.browser}`);
            } else {
                showError(res.error || `Failed to clear browser ${target.browser}.`);
            }
        });
    };

    useEffect(() => {
        const closeAll = () => { setWlanDialogOpen(false); setBrowserFootprintsDialogOpen(false); };
        window.addEventListener("commander-dismiss-dialogs", closeAll);
        return () => window.removeEventListener("commander-dismiss-dialogs", closeAll);
    }, []);

    const openers = { handleWlanProfiles, handleBrowserFootprints };
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

<Dialog
                isOpen={browserFootprintsDialogOpen}
                onClose={() => setBrowserFootprintsDialogOpen(false)}
                title="Browser Footprint Audit"
                className="mount-dialog priv-dialog"
                isCloseButtonShown={true}
                canEscapeKeyClose={true}
            >
                <div className="wc-dialog-body">
                    <div style={{
                        display: "flex",
                        flexWrap: "wrap",
                        alignItems: "center",
                        justifyContent: "space-between",
                        gap: 16,
                        padding: "12px 14px",
                        borderRadius: 6,
                        border: "1px solid var(--color-border)",
                        background: "var(--color-bg-tertiary)",
                        marginBottom: 14,
                    }}>
                        <div style={{ minWidth: 0 }}>
                            <div className="text-xs font-semibold" style={{ color: "var(--color-text-primary)" }}>
                                Browser artifact inventory
                            </div>
                            <div className="text-[11px] mt-1" style={{ color: "var(--color-text-muted)", lineHeight: 1.45 }}>
                                History, cookies, caches, sessions, local storage, and login databases by detected browser profile.
                            </div>
                        </div>
                        <div className="flex items-center gap-2 flex-shrink-0">
                            <span className="font-mono text-[11px] px-2 py-1 rounded" style={{ background: "var(--color-bg-secondary)", color: "var(--color-text-secondary)", border: "1px solid var(--color-border)" }}>
                                {browserFootprints.length} browsers
                            </span>
                            <span className="font-mono text-[11px] px-2 py-1 rounded" style={{ background: "color-mix(in srgb, var(--color-danger) 12%, transparent)", color: "var(--color-danger)", border: "1px solid rgba(255,90,69,.25)" }}>
                                {formatSizeKB(browserFootprints.reduce((sum, b) => sum + (b.totalSizeKB || 0), 0))}
                            </span>
                        </div>
                    </div>
                    <div style={{ maxHeight: 500, overflow: "auto", paddingRight: "6px" }}>
                        {localLoadingMap['browserFootprints'] && (
                            <div className="flex items-center justify-center gap-2 py-8 text-xs" role="status" style={{ color: "var(--color-text-muted)" }}>
                                <Icon icon="refresh" size={14} className="animate-spin" />
                                Auditing browser profiles…
                            </div>
                        )}
                        {browserFootprintsError && !localLoadingMap['browserFootprints'] && (
                            <div className="mx-1 mb-3 flex flex-wrap items-center justify-between gap-2 rounded border px-3 py-2 text-xs" role="alert" style={{ borderColor: "var(--color-danger)", color: "var(--color-danger)", background: "var(--color-danger-dim)" }}>
                                <span>Browser audit failed: {browserFootprintsError}</span>
                                <Button small minimal icon="refresh" text="RETRY AUDIT" onClick={() => void handleBrowserFootprints()} />
                            </div>
                        )}
                        {browserFootprints.length === 0 && !localLoadingMap['browserFootprints'] && !browserFootprintsError && (
                            <p className="text-xs text-center py-4" style={{ color: "var(--color-text-muted)" }}>No browser profiles detected</p>
                        )}
                        <div className="grid grid-cols-1 gap-3 pb-1.5 lg:grid-cols-2">
                            {[...browserFootprints].sort((a, b) => b.totalSizeKB - a.totalSizeKB).map((b) => (
                                <div key={`${b.browser}:${b.profilePath}`} style={{ background: "var(--color-bg-secondary)", borderRadius: 8, padding: 14, border: "1px solid var(--color-border)" }}>
                                    <div className="flex items-start justify-between gap-3 mb-2">
                                        <div style={{ minWidth: 0 }}>
                                            <div className="text-sm font-semibold truncate" style={{ color: "var(--color-text-primary)" }}>{b.browser}</div>
                                            <div className="font-mono text-[10px] truncate mt-0.5" title={b.profilePath} style={{ color: "var(--color-text-muted)" }}>
                                                {b.profilePath}
                                            </div>
                                        </div>
                                        <span className="text-[11px] font-mono px-2 py-1 rounded flex-shrink-0" style={{ background: "color-mix(in srgb, var(--color-danger) 12%, transparent)", color: "var(--color-danger)", border: "1px solid rgba(255,90,69,.25)" }}>
                                            {formatSizeKB(b.totalSizeKB)}
                                        </span>
                                    </div>
                                    <div className="mt-2.5 grid grid-cols-1 gap-x-3 gap-y-1.5 sm:grid-cols-2">
                                        {b.artifacts.filter(a => a.sizeKB > 0).map((a) => (
                                            <div key={`${b.browser}:${b.profilePath}:${a.name}`} style={{ minWidth: 0, padding: "4px 6px", borderRadius: 4, background: "var(--color-bg-tertiary)", border: "1px solid color-mix(in srgb, var(--color-border) 70%, transparent)" }}>
                                                <div className="text-[10px] truncate" title={a.name} style={{ color: "var(--color-text-secondary)" }}>{a.name}</div>
                                                <div className="text-[11px] font-mono mt-0.5" style={{ color: "var(--color-text-muted)", whiteSpace: "nowrap" }}>
                                                    {formatSizeKB(a.sizeKB)}
                                                </div>
                                            </div>
                                        ))}
                                        {b.artifacts.every(a => a.sizeKB === 0) && (
                                            <span className="text-[10px] opacity-40">No data found</span>
                                        )}
                                    </div>
                                    <div className="flex justify-end" style={{ marginTop: 8 }}>
                                        <Button
                                            icon="trash"
                                            text="CLEAR THIS BROWSER"
                                            aria-label={`Clear ${b.browser} artifacts for ${b.profilePath}`}
                                            onClick={() => handleClearBrowserFootprint(b)}
                                            loading={localLoadingMap[browserFootprintCacheKey(b)]}
                                            className="modal-primary-btn danger"
                                        />
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>
                <div className="mount-dialog-footer">
                    <Button icon="cross" text="CLOSE" onClick={() => setBrowserFootprintsDialogOpen(false)} minimal className="modal-cancel-btn" />
                    <Button icon="trash" text="CLEAR ALL BROWSERS" onClick={handleClearBrowserFootprints} loading={localLoadingMap["browserFootprintsClear"]} disabled={browserFootprints.length === 0 || !!localLoadingMap['browserFootprints']} className="modal-primary-btn danger" />
                </div>
            </Dialog>
    </>) };
}
