import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import useBackend from "@/hooks/useBackend";
import { recordMaintenanceOperation } from "@/hooks/useMaintenanceHistory";
import { showError, showSuccess } from "@/utils/toast";
import { independentDiskSelection, type DiskNode, type DiskScanMeta, type LargeDiskItem, type LargeDiskResult } from "@/lib/diskAnalyzer";

interface AnalyzerSession {
    scanning: boolean;
    scanPath: string;
    meta: DiskScanMeta | null;
    rows: DiskNode[];
    largeItems: LargeDiskItem[];
    largeResult: LargeDiskResult | null;
    currentPath: string;
    history: string[];
    historyIndex: number;
    stale: boolean;
    deleting: boolean;
    loadingPath: boolean;
    largeLoading: boolean;
    minSize: number;
}

let session: AnalyzerSession = {
    scanning: false, scanPath: "C:\\", meta: null, rows: [], largeItems: [], largeResult: null,
    currentPath: "", history: [], historyIndex: -1, stale: false,
    deleting: false, loadingPath: false, largeLoading: false, minSize: 100 * 1024 * 1024,
};
let scanTask: Promise<void> | null = null;
let navigationRequest = 0;
let largeRequest = 0;
const listeners = new Set<() => void>();
const getSession = () => session;
function subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; }
function update(patch: Partial<AnalyzerSession>) { session = { ...session, ...patch }; listeners.forEach(listener => listener()); }

export function useDiskAnalyzer() {
    const state = useSyncExternalStore(subscribe, getSession);
    const backend = useBackend();
    const backendRef = useRef(backend);
    backendRef.current = backend;
    const [selected, setSelected] = useState<Set<string>>(new Set());
    const [focused, setFocused] = useState<DiskNode | null>(null);
    const [confirmItems, setConfirmItems] = useState<DiskNode[]>([]);
    const [progress, setProgress] = useState<{ filesScanned: number; foldersScanned: number; bytesScanned: number } | null>(null);
    const [cancelling, setCancelling] = useState(false);

    useEffect(() => {
        setSelected(new Set()); setFocused(null); setConfirmItems([]);
    }, [state.scanning, state.currentPath, state.stale]);

    useEffect(() => {
        if (!state.scanning) { setProgress(null); setCancelling(false); return; }
        let active = true;
        let pending = false;
        const timer = setInterval(async () => {
            if (pending) return;
            pending = true;
            try {
                const next = await backendRef.current.getDiskScanProgress();
                if (active) setProgress(next);
            } catch { /* The scan result remains authoritative if progress is unavailable. */ }
            finally { pending = false; }
        }, 1000);
        return () => { active = false; clearInterval(timer); };
    }, [state.scanning]);

    async function startScan() {
        if (scanTask || session.deleting || !session.scanPath.trim()) return;
        const path = session.scanPath;
        navigationRequest++;
        largeRequest++;
        update({ scanning: true, meta: null, rows: [], largeItems: [], largeResult: null, currentPath: "", history: [], historyIndex: -1, stale: false, loadingPath: false, largeLoading: false });
        scanTask = (async () => {
            try {
                const meta = await backendRef.current.runDiskScan(path);
                const [rows, largeResult] = await Promise.all([
                    backendRef.current.getDiskChildren(meta.scanRoot),
                    backendRef.current.getLargeDiskItems(session.minSize, 5000, false),
                ]);
                update({ meta, rows, largeItems: largeResult.items, largeResult, currentPath: meta.scanRoot, history: [meta.scanRoot], historyIndex: 0 });
                recordMaintenanceOperation({ tool: "disk-analysis", title: "Storage analyzed", outcome: meta.cancelled ? "cancelled" : meta.complete === false ? "partial" : "completed", affectedItems: meta.fileCount, detail: "Logical sizes only; no files changed." });
            } catch (error) { showError(String(error)); recordMaintenanceOperation({ tool: "disk-analysis", title: "Storage analysis failed", outcome: "failed" }); }
            finally { update({ scanning: false }); scanTask = null; }
        })();
        await scanTask;
    }

    async function navigate(path: string, historyIndex?: number) {
        if (session.scanning || session.deleting || session.stale) return;
        const request = ++navigationRequest;
        update({ loadingPath: true });
        setSelected(new Set()); setFocused(null);
        try {
            const rows = await backendRef.current.getDiskChildren(path);
            if (request !== navigationRequest) return;
            const history = historyIndex === undefined ? [...session.history.slice(0, session.historyIndex + 1), path] : session.history;
            update({ rows, currentPath: path, history, historyIndex: historyIndex ?? history.length - 1 });
        } catch (error) { if (request === navigationRequest) showError(String(error)); }
        finally { if (request === navigationRequest) update({ loadingPath: false }); }
    }

    async function changeMinSize(next: number) {
        if (session.scanning || session.deleting || session.stale) return;
        update({ minSize: next, largeItems: [], largeResult: null }); setSelected(new Set()); setFocused(null);
        if (!session.meta) return;
        const request = ++largeRequest;
        update({ largeLoading: true });
        try {
            const largeResult = await backendRef.current.getLargeDiskItems(next, 5000, false);
            if (request === largeRequest) update({ largeItems: largeResult.items, largeResult });
        } catch (error) { showError(String(error)); }
        finally { if (request === largeRequest) update({ largeLoading: false }); }
    }

    function toggleSelection(item: DiskNode) {
        if (item.isDir) return;
        setSelected(previous => {
            const next = new Set(previous);
            if (next.has(item.fullPath)) next.delete(item.fullPath); else next.add(item.fullPath);
            return next;
        });
    }

    function selectedItems(): DiskNode[] {
        const candidates = new Map([...session.rows, ...session.largeItems].map(item => [item.fullPath, item]));
        return independentDiskSelection([...selected].flatMap(path => candidates.has(path) && !candidates.get(path)!.isDir ? [candidates.get(path)!] : []));
    }

    async function deleteConfirmed() {
        if (session.deleting || session.scanning || session.stale || !confirmItems.length) return;
        navigationRequest++; largeRequest++;
        update({ deleting: true, loadingPath: false, largeLoading: false });
        const failed: string[] = [];
        let removed = 0;
        for (const item of confirmItems) {
            try { await backendRef.current.diskDeleteItem(item.fullPath); removed++; }
            catch { failed.push(item.name); }
        }
        setSelected(new Set()); setFocused(null); setConfirmItems([]);
        if (removed) {
            // Every aggregate and parent size is from the previous scan, so require a fresh scan.
            update({ stale: true, rows: [], largeItems: [] });
            showSuccess(`Deleted ${removed} item${removed === 1 ? "" : "s"}. Refreshing disk usage…`);
        }
        if (failed.length) showError(`Could not delete ${failed.length} item${failed.length === 1 ? "" : "s"}: ${failed.slice(0, 3).join(", ")}`);
        recordMaintenanceOperation({ tool: "disk-analysis", title: "Selected files deleted", outcome: failed.length ? removed ? "partial" : "failed" : "completed", affectedItems: removed, detail: failed.length ? `${failed.length} files could not be deleted.` : "Analyze again to measure current storage. Physical space recovered was not measured." });
        update({ deleting: false });
        if (removed) await startScan();
    }

    async function cancelScan() {
        setCancelling(true);
        try { await backendRef.current.cancelDiskScan(); }
        catch (error) { setCancelling(false); showError(String(error)); }
    }

    async function openInExplorer(item: DiskNode) {
        if (!session.meta || session.stale || session.scanning) return;
        try { await backendRef.current.openDiskItem(session.meta.scanId, item.entryId); } catch (error) { showError(String(error)); }
    }

    return { ...state, selected, focused, confirmItems, progress, cancelling,
        setScanPath: (scanPath: string) => update({ scanPath }), startScan, navigate, changeMinSize, toggleSelection,
        setSelected, setFocused, setConfirmItems, selectedItems, deleteConfirmed, cancelScan, openInExplorer,
        getChildren: (path: string) => backendRef.current.getDiskChildren(path) };
}

export type DiskAnalyzer = ReturnType<typeof useDiskAnalyzer>;
