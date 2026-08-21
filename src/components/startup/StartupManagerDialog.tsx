/**
 * STARTUP MANAGER DIALOG
 * Note: Currently hidden from UI as it is considered buggy (items not re-enabling correctly).
 */
import { useCallback, useEffect, useState } from "react";
import { Dialog, DialogBody, DialogFooter, Button, Intent, Icon, NonIdealState, Spinner } from "@/components/ui/bp";
import { motion } from "framer-motion";
import useBackend, { StartupItem } from "../../hooks/useBackend";

export default function StartupManagerDialog({ isOpen, onClose }: { isOpen: boolean, onClose: () => void }) {
    const {
        getStartupItems,
        enableStartupItem,
        disableStartupItem,
        optimizeStartup
    } = useBackend();

    const [items, setItems] = useState<StartupItem[]>([]);
    const [loading, setLoading] = useState(false);
    const [stats, setStats] = useState({ totalRam: 0, potentialSavings: 0 });
    const [optimizing, setOptimizing] = useState(false);

    const fetchItems = useCallback(async () => {
        setLoading(true);
        try {
            const res = await getStartupItems();
            if (res.success && res.data) {
                const list = (res.data.items || res.data) as StartupItem[] | undefined;

                if (Array.isArray(list)) {
                    // Sort: Enabled first, then by Name
                    const sortedList = list.sort((a, b) => {
                        if (a.IsEnabled !== b.IsEnabled) return a.IsEnabled ? -1 : 1;
                        return a.Name.localeCompare(b.Name);
                    });
                    setItems(sortedList);

                    // Calculate stats based on Running items that are bloat
                    const ram = sortedList.reduce((acc, item) => acc + (item.Status === "Running" ? item.RamUsageMB : 0), 0);
                    const savings = sortedList
                        .filter(i => i.Recommendation === "Disable" && i.Status === "Running")
                        .reduce((acc, item) => acc + item.RamUsageMB, 0);

                    setStats({ totalRam: ram, potentialSavings: savings });
                }
            }
        } catch (e) {
            console.error(e);
        } finally {
            setLoading(false);
        }
    }, [getStartupItems]);

    useEffect(() => {
        if (isOpen) fetchItems();
    }, [fetchItems, isOpen]);

    const handleToggle = async (item: StartupItem, enable: boolean) => {
        try {
            if (enable) {
                await enableStartupItem(item.Name);
            } else {
                await disableStartupItem(item.Name, item.Location);
            }
            // Small delay to allow backend to process
            setTimeout(fetchItems, 500);
        } catch (e) {
            console.error(e);
        }
    };

    const handleOptimize = async () => {
        setOptimizing(true);
        try {
            await optimizeStartup(false);
            await fetchItems();
        } catch (e) {
            console.error(e);
        } finally {
            setOptimizing(false);
        }
    };

    return (
        <Dialog
            isOpen={isOpen}
            onClose={onClose}
            title="Startup Manager"
            icon="rocket-slant"
            className="pb-0"
            style={{
                width: '900px',
                height: '80vh',
                maxWidth: '95vw',
                backgroundColor: 'var(--color-bg-primary)',
                color: 'var(--color-text-primary)'
            }}
        >
            <DialogBody>
                {/* Hero / Visualization Section */}
                <div className="grid grid-cols-2 gap-8 mb-6">
                    <div className="p-4 rounded-md flex flex-col items-center justify-center text-center" style={{ background: "color-mix(in srgb, var(--color-info) 10%, transparent)", border: "1px solid var(--color-info-dim)" }}>
                        <Icon icon="dashboard" size={30} className="mb-2" style={{ color: "var(--color-info)" }} />
                        <div className="text-2xl font-bold" style={{ color: "var(--color-info)" }}>
                            {Math.round(stats.totalRam)} MB
                        </div>
                        <div className="text-xs uppercase tracking-widest opacity-70" style={{ color: "var(--color-text-primary)" }}>Current Ram Load</div>
                    </div>

                    <div className="p-4 rounded-md flex flex-col items-center justify-center text-center relative overflow-hidden" style={{ background: "color-mix(in srgb, var(--color-success) 10%, transparent)", border: "1px solid var(--color-success-dim)" }}>
                        <motion.div
                            className="absolute inset-0"
                            style={{ background: "color-mix(in srgb, var(--color-success) 5%, transparent)" }}
                            animate={{ opacity: [0.5, 1, 0.5] }}
                            transition={{ duration: 2, repeat: Infinity }}
                        />
                        <Icon icon="clean" size={30} className="mb-2" style={{ color: "var(--color-success)" }} />
                        <div className="text-2xl font-bold" style={{ color: "var(--color-success)" }}>
                            {Math.round(stats.potentialSavings)} MB
                        </div>
                        <div className="text-xs uppercase tracking-widest opacity-70" style={{ color: "var(--color-text-primary)" }}>Potential Savings</div>
                    </div>
                </div>

                {/* Optimization Button */}
                <div className="flex justify-between items-center mb-4 p-4 rounded-md" style={{ background: "var(--color-bg-tertiary)", border: "1px solid var(--color-border)" }}>
                    <div>
                        <h4 className="text-lg font-bold flex items-center gap-2" style={{ color: "var(--color-text-primary)" }}>
                            One-Click Optimization
                        </h4>
                        <p className="text-sm" style={{ color: "var(--color-text-muted)" }}>
                            Safety Check: {items.filter(i => i.Recommendation === "Disable" && i.IsEnabled).length} items marked as unnecessary bloat.
                        </p>
                    </div>
                    <Button
                        intent={Intent.SUCCESS}
                        large
                        icon="rocket-slant"
                        text="Optimize Startup"
                        // Removed hardcoded color to fix white-on-white issue in Light Mode
                        onClick={handleOptimize}
                        loading={optimizing}
                        disabled={items.filter(i => i.Recommendation === "Disable" && i.IsEnabled).length === 0}
                    />
                </div>

                {/* List */}
                <div className="overflow-y-auto h-[calc(80vh-320px)] border rounded-md" style={{ borderColor: "var(--color-border)", backgroundColor: "var(--color-bg-secondary)" }}>
                    {loading ? (
                        <div className="flex items-center justify-center h-full">
                            <Spinner />
                        </div>
                    ) : items.length === 0 ? (
                        <NonIdealState icon="clean" title="No Startup Items Found" description="Your system is clean!" />
                    ) : (
                        <table className="w-full text-left border-collapse">
                            <thead className="sticky top-0 z-10 shadow-lg" style={{ backgroundColor: "var(--color-bg-tertiary)" }}>
                                <tr>
                                    <th className="p-3 text-xs uppercase tracking-wider font-semibold border-b" style={{ color: "var(--color-text-secondary)", borderColor: "var(--color-border)" }}>App Name</th>
                                    <th className="p-3 text-xs uppercase tracking-wider font-semibold border-b" style={{ color: "var(--color-text-secondary)", borderColor: "var(--color-border)" }}>State</th>
                                    <th className="p-3 text-xs uppercase tracking-wider font-semibold border-b" style={{ color: "var(--color-text-secondary)", borderColor: "var(--color-border)" }}>Impact</th>
                                    <th className="p-3 text-xs uppercase tracking-wider font-semibold border-b text-right" style={{ color: "var(--color-text-secondary)", borderColor: "var(--color-border)" }}>Action</th>
                                </tr>
                            </thead>
                            <tbody>
                                {items.map((item, idx) => {
                                    const isBloat = item.Recommendation === "Disable";
                                    const isSafe = item.Recommendation === "Keep";
                                    const isEnabled = item.IsEnabled;

                                    return (
                                        <tr key={idx} className="border-b transition-colors hover:bg-[var(--color-bg-card)]" style={{ borderColor: "var(--color-border)" }}>
                                            <td className="p-3">
                                                <div className="font-bold flex items-center gap-2" style={{ color: "var(--color-text-primary)", opacity: isEnabled ? 1 : 0.6 }}>
                                                    {item.Name.replace(".lnk", "").replace(".disabled", "")}
                                                    {isBloat && <span className="px-1.5 py-0.5 rounded text-[10px] font-mono" style={{ background: "color-mix(in srgb, var(--color-danger) 10%, transparent)", color: "var(--color-danger)", border: "1px solid var(--color-danger-dim)" }}>BLOAT</span>}
                                                    {isSafe && <span className="px-1.5 py-0.5 rounded text-[10px] font-mono" style={{ background: "color-mix(in srgb, var(--color-info) 10%, transparent)", color: "var(--color-info)", border: "1px solid var(--color-info-dim)" }}>SYSTEM</span>}
                                                </div>
                                                <div className="text-xs truncate max-w-[300px]" title={item.Command} style={{ color: "var(--color-text-muted)" }}>
                                                    {item.Description !== "Unknown Application" ? item.Description : item.Command}
                                                </div>
                                            </td>
                                            <td className="p-3">
                                                {isEnabled ? (
                                                    <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium" style={{ backgroundColor: "color-mix(in srgb, var(--color-success) 10%, transparent)", color: "var(--color-success)" }}>
                                                        Enabled
                                                    </span>
                                                ) : (
                                                    <span className="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium" style={{ backgroundColor: "rgba(107, 114, 128, 0.1)", color: "var(--color-text-muted)" }}>
                                                        Disabled
                                                    </span>
                                                )}
                                            </td>
                                            <td className="p-3 font-mono text-sm">
                                                {item.Status === "Running" ? (
                                                    <span style={{ color: "var(--color-warning)" }}>{Math.round(item.RamUsageMB)} MB</span>
                                                ) : (
                                                    <span style={{ color: "var(--color-text-muted)", opacity: 0.5 }}>-</span>
                                                )}
                                            </td>
                                            <td className="p-3 text-right">
                                                {isSafe ? (
                                                    <div className="text-xs italic mr-2 flex items-center justify-end gap-1" style={{ color: "var(--color-text-muted)" }}>
                                                        <Icon icon="lock" size={12} /> Protected
                                                    </div>
                                                ) : (
                                                    item.IsEnabled ? (
                                                        <Button
                                                            small
                                                            intent={Intent.DANGER}
                                                            text="Disable"
                                                            icon="cross"
                                                            onClick={() => handleToggle(item, false)}
                                                        />
                                                    ) : (
                                                        <Button
                                                            small
                                                            intent={Intent.NONE}
                                                            text="Enable"
                                                            icon="undo"
                                                            onClick={() => handleToggle(item, true)}
                                                        />
                                                    )
                                                )}
                                            </td>
                                        </tr>
                                    );
                                })}
                            </tbody>
                        </table>
                    )}
                </div>
            </DialogBody>
            <DialogFooter>
                <Button onClick={onClose} text="Close" />
            </DialogFooter>
        </Dialog>
    );
}
