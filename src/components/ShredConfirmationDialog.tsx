import { Button, Dialog, Icon } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "react";
import { useBackend } from "../hooks/useBackend";
import { useTheme } from "../context/ThemeContext";
import { runOperation } from "../context/OperationContext";
import './ShredConfirmationDialog.css';

interface ShredTarget {
    path: string;
    name: string;
    isDir: boolean;
}

interface ShredConfirmationDialogProps {
    isOpen: boolean;
    paths: string[];
    onClose: () => void;
}

export default function ShredConfirmationDialog({ isOpen, paths, onClose }: ShredConfirmationDialogProps) {
    const { invoke7Erase, invokeSSDTrim } = useBackend();
    const { theme } = useTheme();
    const [targets, setTargets] = useState<ShredTarget[]>([]);
    const [resolving, setResolving] = useState(false);

    // Resolve file-vs-directory for each path whenever the dialog opens
    // with new paths. The cancelled flag prevents a stale resolve from
    // overwriting a newer batch's results — without it, two rapid
    // updates to `paths` (e.g., user shift-selecting more files in
    // Explorer's "Shred with WinCommander") could race and the older
    // batch could land last and clobber the newer one.
    useEffect(() => {
        if (!isOpen || paths.length === 0) { setTargets([]); return; }
        let cancelled = false;
        setResolving(true);
        Promise.all(
            paths.map(async (p) => {
                const isDir = await invoke<boolean>("is_path_dir", { path: p }).catch(() => false);
                return { path: p, name: p.split(/[\\/]/).pop() ?? p, isDir };
            })
        ).then(resolved => {
            if (!cancelled) setTargets(resolved);
        }).finally(() => {
            if (!cancelled) setResolving(false);
        });
        return () => { cancelled = true; };
    }, [isOpen, paths]);

    // Sequence counter for the in-dialog Add files / Add folder
    // pickers. Each picker click bumps the seq; only the latest click's
    // result is allowed to mutate state. Stops a slow OS picker from
    // leaking its old selection over a newer one when the user clicks
    // Add quickly twice (or cancels-then-reopens before the first
    // dialog finishes returning).
    const addSeqRef = useRef(0);

    const addFiles = async () => {
        const seq = ++addSeqRef.current;
        const selected = await openFilePicker({ multiple: true }).catch(() => null);
        if (seq !== addSeqRef.current) return;
        if (!selected) return;
        const newPaths = Array.isArray(selected) ? selected : [selected];
        const resolved = newPaths.map(p => ({
            path: p,
            name: p.split(/[\\/]/).pop() ?? p,
            isDir: false,
        }));
        setTargets(prev => {
            const existing = new Set(prev.map(t => t.path));
            return [...prev, ...resolved.filter(t => !existing.has(t.path))];
        });
    };

    const addFolder = async () => {
        const seq = ++addSeqRef.current;
        const selected = await openFilePicker({ directory: true }).catch(() => null);
        if (seq !== addSeqRef.current) return;
        if (!selected || Array.isArray(selected)) return;
        setTargets(prev => {
            if (prev.some(t => t.path === selected)) return prev;
            return [...prev, { path: selected, name: selected.split(/[\\/]/).pop() ?? selected, isDir: true }];
        });
    };

    const remove = (path: string) => setTargets(prev => prev.filter(t => t.path !== path));

    const handleShred = () => {
        if (targets.length === 0) return;
        onClose();
        const steps = targets.map(t => ({
            label: `${t.isDir ? 'Delete folder' : 'Securely delete'}: ${t.name}`,
            fn: async () => {
                const result = await invoke7Erase(t.path, t.isDir ? 'Directory' : 'File');
                if (!result.success) throw new Error(result.error ?? "Deletion failed");
            },
        }));
        steps.push({
            label: "Scheduling SSD TRIM",
            fn: async () => { await invokeSSDTrim(); },
        });
        runOperation("SECURE DELETE", steps, { doneTitle: "DELETION COMPLETE", accent: "red" });
    };

    const hasFolders = targets.some(t => t.isDir);
    const confirmLabel = targets.length > 1 ? `DELETE ${targets.length} ITEMS` : targets[0]?.isDir ? "DELETE FOLDER" : "DELETE FILE";

    return (
        <Dialog
            isOpen={isOpen}
            onClose={onClose}
            className={`shred-dialog${theme === "light" ? " light" : ""}`}
            canOutsideClickClose
            canEscapeKeyClose
        >
            {/* Header */}
            <div className="shred-header">
                <div className="shred-header-icon">
                    <Icon icon="flame" size={15} color="var(--color-danger)" />
                </div>
                <div className="shred-header-text">
                    <div className="shred-header-title">SECURE DELETE</div>
                    <div className="shred-header-sub">Multi-pass overwrite — irreversible</div>
                </div>
                <Button icon="cross" minimal small onClick={onClose} className="shred-close-btn" />
            </div>

            {/* Body */}
            <div className="shred-body">
                <div className="shred-warning-box">
                    <Icon icon="warning-sign" size={13} />
                    <span>
                        Permanently destroys {targets.length || paths.length} item{(targets.length || paths.length) !== 1 ? 's' : ''}.
                        {hasFolders && " Folders are destroyed recursively."}
                    </span>
                </div>

                <div className="shred-targets-header">
                    <span className="shred-path-label">TARGETS ({targets.length})</span>
                    <div className="shred-add-row">
                        <Button minimal small icon="document" onClick={addFiles} className="shred-add-btn">
                            Add files
                        </Button>
                        <Button minimal small icon="folder-close" onClick={addFolder} className="shred-add-btn">
                            Add folder
                        </Button>
                    </div>
                </div>

                <div className="shred-target-list">
                    {resolving ? (
                        <div className="shred-resolving">Resolving paths…</div>
                    ) : targets.length === 0 ? (
                        <div className="shred-resolving">No targets selected.</div>
                    ) : (
                        targets.map(t => (
                            <div key={t.path} className="shred-target-row">
                                <Icon
                                    icon={t.isDir ? "folder-close" : "document"}
                                    size={11}
                                    color={t.isDir ? "var(--color-warning)" : "var(--color-text-muted)"}
                                />
                                <div className="shred-target-info">
                                    <span className="shred-target-name">{t.name}</span>
                                    <span className="shred-target-path">{t.path}</span>
                                </div>
                                <button className="shred-target-remove" onClick={() => remove(t.path)} title="Remove from list">
                                    ×
                                </button>
                            </div>
                        ))
                    )}
                </div>
            </div>

            {/* Footer */}
            <div className="shred-footer">
                <Button text="CANCEL" minimal className="shred-cancel-btn" onClick={onClose} />
                <Button
                    text={confirmLabel}
                    icon="flame"
                    className="shred-confirm-btn"
                    onClick={handleShred}
                    disabled={targets.length === 0}
                />
            </div>
        </Dialog>
    );
}
