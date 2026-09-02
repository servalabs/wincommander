// ══════════════════════════════════════════════════════════════════════════
// ManageAppsDialog — add, edit, remove, and reorder Server Apps
// ══════════════════════════════════════════════════════════════════════════
import { useState, useCallback } from 'react';
import {
    Classes, Dialog, DialogBody, DialogFooter,
    Button, FormGroup, InputGroup, HTMLSelect, TextArea, Tag, Icon,
} from '@/components/ui/bp';
import type { IconName } from '@/components/ui/bp';
import type { ServerAppConfig } from '../../types/settings';
import { useTheme } from '../../context/ThemeContext';
import { useAppState } from '../../context/AppContext';
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from '../../lib/machineScopeElevation';

// ── Icon options available in this picker ─────────────────────────────────
const ICON_OPTIONS: { value: string; label: string }[] = [
    { value: 'applications', label: 'Applications' },
    { value: 'cloud', label: 'Cloud' },
    { value: 'code', label: 'Code' },
    { value: 'console', label: 'Console' },
    { value: 'dashboard', label: 'Dashboard' },
    { value: 'data-connection', label: 'Data' },
    { value: 'database', label: 'Database' },
    { value: 'document', label: 'Document' },
    { value: 'exchange', label: 'Exchange' },
    { value: 'filter', label: 'Filter' },
    { value: 'folder-open', label: 'Folder' },
    { value: 'globe-network', label: 'Globe' },
    { value: 'home', label: 'Home' },
    { value: 'inbox', label: 'Inbox' },
    { value: 'key', label: 'Key' },
    { value: 'lock', label: 'Lock' },
    { value: 'map-marker', label: 'Location' },
    { value: 'media', label: 'Media' },
    { value: 'phone', label: 'Phone' },
    { value: 'refresh', label: 'Refresh' },
    { value: 'search', label: 'Search' },
    { value: 'server', label: 'Server' },
    { value: 'settings', label: 'Settings' },
    { value: 'share', label: 'Share' },
    { value: 'shield', label: 'Shield' },
    { value: 'timeline-area-chart', label: 'Stats' },
    { value: 'upload', label: 'Upload' },
    { value: 'video', label: 'Video' },
];

// ── Local row state — extends config with edit-expanded flag ──────────────
// KT: _stableKey is a per-row identity that NEVER changes (even when name/id
// changes). Used as the React `key` prop to prevent unmount/remount on every
// keystroke, which would kill input focus.
interface AppRow extends ServerAppConfig {
    _cssOpen: boolean;
    _stableKey: string;
}

let _rowCounter = 0;
function toRows(apps: ServerAppConfig[]): AppRow[] {
    return apps.map(a => ({ ...a, _cssOpen: false, _stableKey: `row-${_rowCounter++}` }));
}

function toApps(rows: AppRow[]): ServerAppConfig[] {
    return rows.map(({ _cssOpen: _c, _stableKey: _s, ...rest }) => rest);
}

function slug(name: string): string {
    return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'app';
}

function uniqueId(base: string, existing: string[]): string {
    let id = slug(base);
    let n = 1;
    while (existing.includes(id)) { id = `${slug(base)}-${n++}`; }
    return id;
}

interface Props {
    isOpen: boolean;
    onClose: () => void;
    apps: ServerAppConfig[];
    onSave: (apps: ServerAppConfig[]) => Promise<void>;
}

export default function ManageAppsDialog({ isOpen, onClose, apps, onSave }: Props) {
    const { theme } = useTheme();
    const { systemInfo } = useAppState();
    const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
    const [rows, setRows] = useState<AppRow[]>([]);
    const [saving, setSaving] = useState(false);
    const [saveError, setSaveError] = useState<string | null>(null);

    // Re-seed local state each time the dialog opens
    const handleOpened = useCallback(() => {
        setRows(toRows(apps));
        setSaveError(null);
    }, [apps]);

    // ── Field update helpers ──────────────────────────────────────────────
    const update = useCallback((idx: number, patch: Partial<AppRow>) => {
        setRows(prev => prev.map((r, i) => i === idx ? { ...r, ...patch } : r));
    }, []);

    const moveUp = useCallback((idx: number) => {
        if (idx === 0) return;
        setRows(prev => {
            const next = [...prev];
            [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
            return next;
        });
    }, []);

    const moveDown = useCallback((idx: number) => {
        setRows(prev => {
            if (idx >= prev.length - 1) return prev;
            const next = [...prev];
            [next[idx], next[idx + 1]] = [next[idx + 1], next[idx]];
            return next;
        });
    }, []);

    const addApp = useCallback(() => {
        setRows(prev => {
            const id = uniqueId('new-app', prev.map(r => r.id));
            const newRow: AppRow = { id, name: 'New App', url: 'https://servalabs.com', icon: 'applications', customCss: '', _cssOpen: false, _stableKey: `row-${_rowCounter++}` };
            return [...prev, newRow];
        });
    }, []);

    const removeApp = useCallback((idx: number) => {
        setRows(prev => prev.filter((_, i) => i !== idx));
    }, []);

    const handleSave = useCallback(async () => {
        if (needsElevation) { setSaveError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
        setSaveError(null);
        setSaving(true);
        try {
            await onSave(toApps(rows));
            onClose();
        } catch (error) {
            setSaveError(error instanceof Error ? error.message : String(error));
        } finally {
            setSaving(false);
        }
    }, [rows, onSave, onClose, needsElevation]);

    // Sync id from name when name changes (only if id still matches the auto-slug pattern)
    const handleNameChange = useCallback((idx: number, name: string) => {
        setRows(prev => {
            const row = prev[idx];
            const oldAuto = slug(prev[idx].name);
            // Only auto-update id if it still matches the previous auto-slug
            const shouldSyncId = row.id === oldAuto;
            const newId = shouldSyncId
                ? uniqueId(name, prev.map((r, i) => i !== idx ? r.id : ''))
                : row.id;
            return prev.map((r, i) => i === idx ? { ...r, name, id: newId } : r);
        });
    }, []);

    return (
        <Dialog
            isOpen={isOpen}
            onClose={() => { if (!saving) onClose(); }}
            onOpened={handleOpened}
            title="Manage Server Apps"
            className={`manage-apps-dialog ${theme === 'dark' ? Classes.DARK : ''}`}
            style={{ width: 620, maxHeight: '85vh' }}
        >
            <DialogBody>
                {needsElevation && <p role="alert" className="manage-apps-error">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}
                {rows.length === 0 && (
                    <div className="manage-apps-empty">
                        <Icon icon="applications" size={32} color="var(--color-text-secondary)" />
                        <p>No apps configured. Click "Add App" to get started.</p>
                    </div>
                )}

                {rows.map((row, idx) => (
                    // KT: key MUST be _stableKey, NOT row.id — id changes as
                    // you type the name (auto-slug sync), causing React to
                    // unmount+remount the row and killing input focus.
                    <div key={row._stableKey} className="manage-app-row">
                        {/* Order controls */}
                        <div className="manage-app-order">
                            <button
                                type="button"
                                className="manage-app-order-btn"
                                onClick={() => moveUp(idx)}
                                disabled={idx === 0}
                                title="Move up"
                                aria-label={`Move ${row.name} up`}
                            >▲</button>
                            <span className="manage-app-num">{idx + 1}</span>
                            <button
                                type="button"
                                className="manage-app-order-btn"
                                onClick={() => moveDown(idx)}
                                disabled={idx === rows.length - 1}
                                title="Move down"
                                aria-label={`Move ${row.name} down`}
                            >▼</button>
                        </div>

                        {/* Icon preview */}
                        <div className="manage-app-icon-preview">
                            <Icon icon={row.icon as IconName} size={18} />
                        </div>

                        {/* Fields */}
                        <div className="manage-app-fields">
                            <div className="manage-app-row-top">
                                <FormGroup label="Name" labelFor={`manage-app-name-${row._stableKey}`} className="manage-app-form-group">
                                    <InputGroup
                                        id={`manage-app-name-${row._stableKey}`}
                                        value={row.name}
                                        onChange={e => handleNameChange(idx, e.target.value)}
                                        placeholder="Display name"
                                        small
                                    />
                                </FormGroup>
                                <FormGroup label="Icon" labelFor={`manage-app-icon-${row._stableKey}`} className="manage-app-form-group manage-app-icon-select">
                                    <HTMLSelect
                                        id={`manage-app-icon-${row._stableKey}`}
                                        value={row.icon}
                                        onChange={e => update(idx, { icon: e.target.value })}
                                        options={ICON_OPTIONS.map(o => ({ value: o.value, label: o.label }))}
                                        minimal
                                    />
                                </FormGroup>
                            </div>
                            <FormGroup label="URL" labelFor={`manage-app-url-${row._stableKey}`} className="manage-app-form-group manage-app-url">
                                <InputGroup
                                    id={`manage-app-url-${row._stableKey}`}
                                    value={row.url}
                                    onChange={e => update(idx, { url: e.target.value })}
                                    placeholder="http://192.168.x.x:port"
                                    small
                                />
                            </FormGroup>
                            <div className="manage-app-css-toggle">
                                <button
                                    type="button"
                                    className="manage-app-css-btn"
                                    onClick={() => update(idx, { _cssOpen: !row._cssOpen })}
                                    aria-expanded={!!row._cssOpen}
                                    aria-controls={`manage-app-css-${row._stableKey}`}
                                >
                                    <Icon icon={row._cssOpen ? 'chevron-down' : 'chevron-right'} size={10} />
                                    Custom CSS
                                    {row.customCss ? <Tag minimal round style={{ marginLeft: 6, fontSize: 9 }}>active</Tag> : null}
                                </button>
                                {row._cssOpen && (
                                    <TextArea
                                        id={`manage-app-css-${row._stableKey}`}
                                        aria-label={`Custom CSS for ${row.name}`}
                                        value={row.customCss || ''}
                                        onChange={e => update(idx, { customCss: e.target.value })}
                                        placeholder="/* CSS injected into the webview */"
                                        className="manage-app-css-area"
                                        rows={4}
                                        fill
                                    />
                                )}
                            </div>
                        </div>

                        {/* Delete */}
                        <Button
                            icon="trash"
                            minimal
                            intent="danger"
                            onClick={() => removeApp(idx)}
                            title="Remove app"
                            aria-label={`Remove ${row.name}`}
                            className="manage-app-delete"
                        />
                    </div>
                ))}

                <Button
                    icon="plus"
                    text="Add App"
                    minimal
                    onClick={addApp}
                    className="manage-apps-add-btn"
                />
                {saveError && (
                    <p className="manage-apps-error" role="alert">
                        Could not save server apps: {saveError}
                    </p>
                )}
            </DialogBody>
            <DialogFooter
                actions={
                    <>
                        <Button text="Cancel" onClick={onClose} minimal disabled={saving} />
                        <Button
                            text="Save Changes"
                            intent="primary"
                            onClick={handleSave}
                            loading={saving}
                            disabled={saving || needsElevation}
                        />
                    </>
                }
            />
        </Dialog>
    );
}
