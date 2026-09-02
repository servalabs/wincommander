import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../components/ui/icon";
import type { FileWatchRule, FileWatchTriggerSettings } from "../../types/settings";

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

    const syncWatcher = useCallback((nextEnabled: boolean, nextRules: FileWatchRule[]) => {
        if (disabled) return;
        if (nextEnabled && nextRules.some(r => r.enabled)) {
            invoke('start_file_watch_triggers', { rules: nextRules.filter(r => r.enabled) }).catch(() => {});
        } else {
            invoke('stop_file_watch_triggers').catch(() => {});
        }
    }, [disabled]);

    useEffect(() => {
        syncWatcher(enabled, rules);
    }, [enabled, rules, syncWatcher]);

    const toggleEnabled = () => {
        if (disabled) return;
        const next = !enabled;
        onPatch({ enabled: next });
        syncWatcher(next, rules);
    };

    const addRule = () => {
        if (disabled) return;
        if (!form.path.trim()) { setFormError('Path is required'); return; }
        if (!form.namePattern.trim()) { setFormError('Name pattern is required'); return; }
        const rule: FileWatchRule = { ...form, id: nanoid(), path: form.path.trim(), namePattern: form.namePattern.trim() };
        const next = [...rules, rule];
        onPatch({ rules: next });
        syncWatcher(enabled, next);
        setForm({ ...EMPTY_FORM });
        setShowAdd(false);
        setFormError('');
    };

    const removeRule = (id: string) => {
        if (disabled) return;
        const next = rules.filter(r => r.id !== id);
        onPatch({ rules: next });
        syncWatcher(enabled, next);
    };

    const toggleRule = (id: string) => {
        if (disabled) return;
        const next = rules.map(r => r.id === id ? { ...r, enabled: !r.enabled } : r);
        onPatch({ rules: next });
        syncWatcher(enabled, next);
    };

    const activeCount = rules.filter(r => r.enabled).length;

    return (
        <div className={bare ? "lockdown-trigger-block" : "rounded-lg border p-4"}>
            <div className="lockdown-trigger-head">
                <Icon icon="folder-open" size={14} className="lockdown-trigger-icon" />
                <span className="lockdown-trigger-title">File Watch</span>
                {enabled && activeCount > 0 && (
                    <span className="lockdown-trigger-status">{activeCount} active</span>
                )}
                <button
                    type="button"
                    role="switch"
                    aria-checked={enabled}
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
                    className="flex flex-col gap-2 p-2 rounded"
                    style={{ background: 'var(--color-bg-secondary)', border: '1px solid var(--color-border)' }}
                >
                    <input
                        type="text"
                        placeholder="Directory path (e.g. C:\Users\Admin\Desktop)"
                        value={form.path}
                        onChange={e => setForm(f => ({ ...f, path: e.target.value }))}
                        className="lockdown-trigger-input"
                    />
                    <div className="flex gap-2">
                        <input
                            type="text"
                            placeholder="Name pattern (e.g. *.pdf, secret.txt)"
                            value={form.namePattern}
                            onChange={e => setForm(f => ({ ...f, namePattern: e.target.value }))}
                            className="lockdown-trigger-input flex-1"
                        />
                        <select
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
                            className="text-[10px] font-bold px-3 py-1 rounded"
                            style={{ background: 'var(--color-accent)', color: '#fff', border: 'none', cursor: 'pointer' }}
                        >
                            Add Rule
                        </button>
                        <button
                            type="button"
                            onClick={() => { setShowAdd(false); setFormError(''); setForm({ ...EMPTY_FORM }); }}
                            className="text-[10px] px-3 py-1 rounded"
                            style={{ background: 'transparent', color: 'var(--color-text-muted)', border: '1px solid var(--color-border)', cursor: 'pointer' }}
                        >
                            Cancel
                        </button>
                    </div>
                </div>
            ) : (
                <button
                    type="button"
                    onClick={() => setShowAdd(true)}
                    className="text-[10px] font-semibold flex items-center gap-1"
                    style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--color-accent)', padding: 0 }}
                >
                    <Icon icon="plus" size={10} />
                    Add rule
                </button>
            )}
        </div>
    );
}
