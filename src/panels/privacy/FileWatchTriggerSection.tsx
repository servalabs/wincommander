import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { armBackend } from "../../hooks/armBackend";
import { Icon } from "../../components/ui/icon";
import type { FileWatchRule, FileWatchTriggerSettings } from "../../types/settings";
import "./LockdownTriggerControls.css";

interface Props {
    settings: FileWatchTriggerSettings | null | undefined;
    onPatch: (patch: Partial<FileWatchTriggerSettings>) => void;
    bare?: boolean;
    disabled?: boolean;
}

const EMPTY_FORM: Omit<FileWatchRule, 'id'> = {
    path: '',
    namePattern: '',
    event: 'created',
    enabled: true,
};

function nanoid() {
    return Math.random().toString(36).slice(2, 10);
}

export default function FileWatchTriggerSection({ settings, onPatch, bare = false, disabled = false }: Props) {
    const enabled = settings?.enabled ?? false;
    const rules = useMemo(() => settings?.rules ?? [], [settings?.rules]);
    const [showAdd, setShowAdd] = useState(false);
    const [form, setForm] = useState({ ...EMPTY_FORM });
    const [formError, setFormError] = useState('');
    const [picking, setPicking] = useState(false);
    const disabledRef = useRef(disabled);
    disabledRef.current = disabled;

    const [watchFailed, setWatchFailed] = useState(false);

    const syncWatcher = useCallback(async (nextEnabled: boolean, nextRules: FileWatchRule[]): Promise<boolean> => {
        if (disabled) return true;
        const starting = nextEnabled && nextRules.some(r => r.enabled);
        const ok = starting
            ? await armBackend('start_file_watch_triggers', { rules: nextRules.filter(r => r.enabled) }, 'File watch triggers are NOT active')
            : await armBackend('stop_file_watch_triggers', undefined, 'File watch triggers could not be stopped');
        setWatchFailed(!ok && starting);
        return ok;
    }, [disabled]);

    useEffect(() => {
        void syncWatcher(enabled, rules);
    }, [enabled, rules, syncWatcher]);

    const toggleEnabled = async () => {
        if (disabled) return;
        const next = !enabled;
        // Enabling is persisted only once the watcher actually started, so the
        // toggle never reads ON while nothing is watching.
        const ok = await syncWatcher(next, rules);
        if (next && !ok) return;
        onPatch({ enabled: next });
    };

    const addRule = () => {
        if (disabled) return;
        if (!form.path.trim()) { setFormError('Path is required'); return; }
        if (!form.namePattern.trim()) { setFormError('Name pattern is required'); return; }
        const rule: FileWatchRule = { ...form, id: nanoid(), path: form.path.trim(), namePattern: form.namePattern.trim() };
        const next = [...rules, rule];
        onPatch({ rules: next });
        void syncWatcher(enabled, next);
        setForm({ ...EMPTY_FORM });
        setShowAdd(false);
        setFormError('');
    };

    const removeRule = (id: string) => {
        if (disabled) return;
        const next = rules.filter(r => r.id !== id);
        onPatch({ rules: next });
        void syncWatcher(enabled, next);
    };

    const toggleRule = (id: string) => {
        if (disabled) return;
        const next = rules.map(r => r.id === id ? { ...r, enabled: !r.enabled } : r);
        onPatch({ rules: next });
        void syncWatcher(enabled, next);
    };

    const activeCount = rules.filter(r => r.enabled).length;

    const chooseDirectory = async () => {
        if (disabled || picking) return;
        setPicking(true);
        try {
            const picked = await openDialog({ directory: true, multiple: false, title: 'Choose a folder to watch' });
            if (!disabledRef.current && typeof picked === 'string') {
                setForm(current => ({ ...current, path: picked }));
                setFormError('');
            }
        } catch {
            setFormError('The folder picker could not be opened. You can enter the directory path instead.');
        } finally {
            setPicking(false);
        }
    };

    return (
        <div className={bare ? "lockdown-trigger-block" : "rounded-lg border p-4"}>
            <div className="lockdown-trigger-head">
                <Icon icon="folder-open" size={14} className="lockdown-trigger-icon" />
                <span className="lockdown-trigger-title">File Watch</span>
                {enabled && activeCount > 0 && !watchFailed && (
                    <span className="lockdown-trigger-status">{activeCount} active</span>
                )}
                {watchFailed && (
                    <span role="alert" className="lockdown-trigger-status" style={{ color: 'var(--color-danger)' }}>not active</span>
                )}
                <button
                    type="button"
                    role="switch"
                    aria-checked={enabled}
                    aria-label="Enable file watch"
                    disabled={disabled}
                    className={`lockdown-trigger-toggle ${enabled ? 'is-on' : ''}`}
                    onClick={toggleEnabled}
                />
            </div>
            <p className="lockdown-trigger-desc">
                Trigger lockdown when a file matching a name pattern is created or deleted at a watched path.
            </p>

            {/* Rule list */}
            {rules.length > 0 && (
                <div className="flex flex-col gap-1 mb-2">
                    {rules.map(rule => (
                        <div
                            key={rule.id}
                            className="flex items-center gap-2 px-2 py-1 rounded"
                            style={{ background: 'var(--color-bg-secondary)', border: '1px solid var(--color-border)', opacity: rule.enabled ? 1 : 0.5 }}
                        >
                            <button
                                type="button"
                                onClick={() => toggleRule(rule.id)}
                                style={{
                                    width: 20, height: 12, borderRadius: 6, border: 'none', cursor: 'pointer', flexShrink: 0,
                                    background: rule.enabled ? 'var(--color-accent)' : 'var(--color-bg-tertiary)',
                                    position: 'relative', transition: 'background 0.15s',
                                }}
                            >
                                <span style={{
                                    position: 'absolute', top: 1, left: rule.enabled ? 9 : 1, width: 10, height: 10,
                                    borderRadius: '50%', background: '#fff', transition: 'left 0.15s',
                                }} />
                            </button>
                            <span
                                className="font-mono text-[9px] px-1.5 py-0.5 rounded flex-shrink-0"
                                style={{
                                    background: rule.event === 'created' ? 'rgba(34,197,94,0.12)' : 'rgba(239,68,68,0.12)',
                                    color: rule.event === 'created' ? 'var(--color-success)' : 'var(--color-danger)',
                                }}
                            >
                                {rule.event === 'created' ? '+create' : '−delete'}
                            </span>
                            <span className="font-mono text-[10px] text-[var(--color-text-secondary)] truncate flex-1 min-w-0">
                                {rule.namePattern}
                            </span>
                            <span className="text-[9px] text-[var(--color-text-muted)] truncate min-w-0 max-w-[100px]" title={rule.path}>
                                {rule.path.split(/[\\/]/).pop() || rule.path}
                            </span>
                            <button
                                type="button"
                                onClick={() => removeRule(rule.id)}
                                style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--color-text-muted)', padding: 2, flexShrink: 0 }}
                            >
                                <Icon icon="cross" size={10} />
                            </button>
                        </div>
                    ))}
                </div>
            )}

            {/* Add rule form */}
            {showAdd ? (
                <div
                    className="lockdown-trigger-form"
                >
                    <input
                        type="text"
                        aria-label="Directory path"
                        placeholder="Directory path (e.g. C:\Users\Admin\Desktop)"
                        value={form.path}
                        onChange={e => setForm(f => ({ ...f, path: e.target.value }))}
                        className="lockdown-trigger-input"
                    />
                    <button type="button" onClick={() => void chooseDirectory()} disabled={disabled || picking}
                        className="lockdown-trigger-action" aria-label="Choose directory to watch">
                        Choose folder
                    </button>
                    <div className="flex gap-2">
                        <input
                            type="text"
                            aria-label="File name pattern"
                            placeholder="Name pattern (e.g. *.pdf, secret.txt)"
                            value={form.namePattern}
                            onChange={e => setForm(f => ({ ...f, namePattern: e.target.value }))}
                            className="lockdown-trigger-input flex-1"
                        />
                        <select
                            aria-label="File watch event"
                            value={form.event}
                            onChange={e => setForm(f => ({ ...f, event: e.target.value as 'created' | 'deleted' }))}
                            className="lockdown-trigger-select"
                        >
                            <option value="created">Created</option>
                            <option value="deleted">Deleted</option>
                        </select>
                    </div>
                    {formError && <span role="alert" className="text-[10px]" style={{ color: 'var(--color-danger)' }}>{formError}</span>}
                    <div className="flex gap-2">
                        <button
                            type="button"
                            onClick={addRule}
                            disabled={disabled}
                            className="lockdown-trigger-action lockdown-trigger-action--primary"
                        >
                            Add rule
                        </button>
                        <button
                            type="button"
                            onClick={() => { setShowAdd(false); setFormError(''); setForm({ ...EMPTY_FORM }); }}
                            disabled={picking}
                            className="lockdown-trigger-action"
                        >
                            Cancel
                        </button>
                    </div>
                </div>
            ) : (
                <div className="flex justify-end">
                <button
                    type="button"
                    onClick={() => setShowAdd(true)}
                    disabled={disabled}
                    className="lockdown-trigger-action lockdown-trigger-action--add"
                >
                    <Icon icon="plus" size={10} />
                    Add rule
                </button>
                </div>
            )}
        </div>
    );
}
