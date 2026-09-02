// src/panels/secret/index.tsx
//
// Secret Settings — rebuilt 2026-06-13, retabbed 2026-07-27.
// 4 tabs: Disguise & branding (incl. licensing) · Borrowed mode & visibility ·
// Lockdown & self-destruct · Diagnostics.
// Each tab composes one or two of the section components below. Business
// logic stays in callbacks here; the VisibilityTable for the panel-visibility
// grid lives in VisibilityTable.tsx.

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import PanelHeader from "../../components/shared/PanelHeader";
import SectionCard from "../../components/shared/SectionCard";
import TierGate from "../../components/shared/TierGate";
import UniversalCallout from "../../components/shared/UniversalCallout";
import { useAppState } from "../../context/AppContext";
import { reportSettingsWriteFailure } from "../../lib/settingsWriteRecovery";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../../lib/machineScopeElevation";
import useBackend from "../../hooks/useBackend";
import useVisibility from "../../hooks/useVisibility";
import useEntitlements from "../../hooks/useEntitlements";
import LogViewer from "../privacy/LogViewer";
import StartupPinConfig from "../privacy/StartupPinConfig";
import LockdownConfigSection from "../privacy/LockdownConfigSection";
import LockdownWordsSection from "../privacy/LockdownWordsSection";
import CheckInTimerSection from "../privacy/CheckInTimerSection";
import PanicHotkeyTrigger from "../privacy/PanicHotkeyTrigger";
import FileWatchTriggerSection from "../privacy/FileWatchTriggerSection";
import CryptoEraseSection from "../vault/CryptoEraseSection";
import { getPersona } from "../../types/settings";
import { Icon, type IconName } from "@/components/ui/bp";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { showError, showSuccess } from "../../utils/toast";
import VisibilityTable from "./VisibilityTable";
import AppLicensingSection from "./AppLicensingSection";
import BrandingLicensingSection from "./BrandingLicensingSection";
import ManagedPolicyBanner from "../../components/shared/ManagedPolicyBanner";
import { useSecretSessionState } from "./secretSessionState";
import RuntimeStatusSection from "./RuntimeStatusSection";
import { DEFAULT_BORROWED_PANELS } from "../../lib/visibilityDefaults";
import "./index.css";
import "../privacy/index.css";

// Empty by default: the cover-name field starts blank (no pre-filled
// Microsoft-impersonation suggestion). When the user enables cover mode
// without typing one, the backend falls back to the real app name.
const DEFAULT_COVER_NAME = "";

const BACKEND_ALL_APP_IDS = [
    "meshVpn", "productivityEngine",
    "instantSearch", "systemCleaner", "unigetui", "ramDiskEngine",
];
const BACKEND_EXE_NAMES = [
    "veracrypt.exe", "tailscale.exe", "tailscale-ipn.exe",
    "activitywatch-app.exe", "aw-qt.exe", "everything.exe",
    "bleachbit.exe", "cryptomator.exe", "unigetui.exe", "imdisk.exe", "imdisksvc.exe",
    "exiftool.exe",
];

// ── 1. LOCK & DISGUISE ──────────────────────────────────────────────────────
// Everything about how the app conceals itself and who gets in: the calculator
// PIN lock, the cover name it shows to onlookers, and hiding the app entirely.

/** A single concealment toggle rendered as an icon tile (new console look). */
function DgzTile({ icon, title, desc, checked, warn, loading, disabled, onChange, children }: {
    icon: IconName;
    title: string;
    desc: string;
    checked: boolean;
    warn?: boolean;
    loading?: boolean;
    disabled?: boolean;
    onChange: (v: boolean) => void;
    children?: ReactNode;
}) {
    return (
        <div className={`dgz-tile ${checked ? (warn ? "warn" : "on") : ""}`}>
            <div className="dgz-tile-row">
                <span className="dgz-tile-ico"><Icon icon={icon} size={15} /></span>
                <div className="dgz-tile-body">
                    <div className="dgz-tile-title">{title}</div>
                    <div className="dgz-tile-desc">{desc}</div>
                </div>
                <Switch
                    checked={checked}
                    disabled={loading || disabled}
                    onCheckedChange={onChange}
                    aria-label={title}
                    title={title}
                />
            </div>
            {children}
        </div>
    );
}

function LockDisguiseSection() {
    const { appSettings, patchAppSettings, refreshSettings, refreshHardening, systemInfo } = useAppState();
    const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
    const { setWinCommanderVisibility, setWinCommanderCalculatorShortcuts, restartExplorer } = useBackend();

    // Reconcile the Hide toggles with the ACTUAL machine state once on mount.
    // After startup the settings flags can lag reality — the app + backend
    // apps stay hidden (hide-flag file present / runtime-visibility manifest
    // applied) but ideal.identity.hideWinCommander/hideBackendAppsList read off,
    // so the toggles showed off. Query the truth and re-sync the flags.
    const hideReconciledRef = useRef(false);
    useEffect(() => {
        if (!appSettings || hideReconciledRef.current) return;
        hideReconciledRef.current = true;
        (async () => {
            try {
                const actualWcHidden = await invoke<boolean>("wincommander_hidden_status");
                if (actualWcHidden !== (appSettings.ideal?.identity?.hideWinCommander === true)) {
                    patchAppSettings({ ideal: { identity: { hideWinCommander: actualWcHidden } } }).catch(reportSettingsWriteFailure);
                }
                const rv = await invoke<{ state?: { entries?: Array<{ key: string; applied: boolean }> } }>("runtime_visibility_state");
                const hiddenKeys = new Set((rv?.state?.entries ?? []).filter(e => e.applied).map(e => e.key.toLowerCase()));
                const actualBackendHidden = BACKEND_EXE_NAMES.some(exe => hiddenKeys.has(exe.toLowerCase()));
                const flagBackendHidden = (appSettings.ideal?.identity?.hideBackendAppsList?.length ?? 0) > 0;
                if (actualBackendHidden !== flagBackendHidden) {
                    patchAppSettings({ ideal: { identity: { hideBackendAppsList: actualBackendHidden ? BACKEND_ALL_APP_IDS : [] } } }).catch(reportSettingsWriteFailure);
                }
            } catch { /* best-effort reconcile */ }
        })();
    }, [appSettings, patchAppSettings]);

    // ── Calculator lock (armed when a Real PIN exists) ──
    const startupPin = appSettings?.ideal?.privacy?.startupPin;
    const hasRealPin = !!startupPin?.realHash;
    const lockArmed = hasRealPin && startupPin?.enabled !== false;
    const lockOnCloseChecked = lockArmed && (appSettings?.app?.lockPanelOnClose ?? true);
    const isHideModeActive = !!appSettings?.ideal?.identity?.hideWinCommander;
    const handleToggleLock = useCallback(async (next: boolean) => {
        if (next && !hasRealPin) {
            showError("Type a Real PIN below and press Set to turn the calculator lock on.");
            return;
        }
        try {
            await invoke("set_startup_pin_enabled", { enabled: next });
            await refreshSettings();
            // Hide mode already manages shortcuts — skip if it's active so we
            // don't conflict with its state file. Only manage shortcuts when
            // hide mode is off.
            if (!isHideModeActive) {
                setWinCommanderCalculatorShortcuts(next).catch(() => {});
            }
            invoke("update_autostart_task_identity", { covered: next || isHideModeActive }).catch(() => {});
            if (next) {
                await invoke("enter_calculator_mode").catch(() => {});
            }
            showSuccess(next
                ? "Calculator lock on — calculator mode is active."
                : "Calculator lock off — saved PINs were kept.");
        } catch (e) {
            showError(`Failed to update calculator lock: ${e}`);
        }
    }, [hasRealPin, refreshSettings, isHideModeActive, setWinCommanderCalculatorShortcuts]);

    // ── Triggers: lock/unlock keywords + peek hotkey ──
    const [lockKw, setLockKw] = useState("");
    const [unlockKw, setUnlockKw] = useState("");
    const kwSeeded = useRef(false);
    const [recordingHotkey, setRecordingHotkey] = useState(false);
    const hotkeyInputRef = useRef<HTMLInputElement>(null);
    const hotkeyValue = appSettings?.ideal?.identity?.hideWinCommanderHotkey ?? "Ctrl+Shift+G";

    useEffect(() => {
        if (!appSettings?.app || kwSeeded.current) return;
        kwSeeded.current = true;
        setLockKw(appSettings.app.lockKeyword ?? "");
        setUnlockKw(appSettings.app.unlockKeyword ?? "");
    }, [appSettings?.app]);

    useEffect(() => {
        if (recordingHotkey) {
            const id = requestAnimationFrame(() => hotkeyInputRef.current?.focus());
            return () => cancelAnimationFrame(id);
        }
    }, [recordingHotkey]);

    const handleHotkeyKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
        e.preventDefault();
        e.stopPropagation();
        if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
        const parts: string[] = [];
        if (e.ctrlKey) parts.push("Ctrl");
        if (e.shiftKey) parts.push("Shift");
        if (e.altKey) parts.push("Alt");
        parts.push(e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key);
        const combo = parts.join("+");
        setRecordingHotkey(false);
        patchAppSettings({ ideal: { identity: { hideWinCommanderHotkey: combo } } }).catch(reportSettingsWriteFailure);
        invoke("update_hide_hotkey", { hotkey: combo }).catch(() => {});
    }, [patchAppSettings]);

    useEffect(() => {
        if (appSettings?.ideal?.identity?.hideWinCommander) {
            invoke("update_hide_hotkey", { hotkey: hotkeyValue }).catch(() => {});
        }
    }, [hotkeyValue, appSettings?.ideal?.identity?.hideWinCommander]);

    const saveKeywords = useCallback(async () => {
        try {
            await patchAppSettings({
                app: {
                    lockKeyword: lockKw.trim() || undefined,
                    unlockKeyword: unlockKw.trim() || undefined,
                },
            });
            showSuccess("Keywords saved.");
        } catch {
            showError("Failed to save keywords.");
        }
    }, [lockKw, unlockKw, patchAppSettings]);

    // ── Cover identity ──
    // `coverEnabled` is DERIVED from settings (single source of truth). Turning
    // the switch on ARMS the editor (shows + focuses the name field) but does
    // NOT commit the default silently — the user types a name and presses
    // Apply/Enter to actually engage cover. `coverArming` holds that pending UI.
    const coverEnabled = appSettings?.app?.decoyMode?.enabled === true;
    const [coverArming, setCoverArming] = useState(false);
    const [coverName, setCoverName] = useState(DEFAULT_COVER_NAME);
    const [coverBusy, setCoverBusy] = useState(false);
    const coverSeeded = useRef(false);
    const coverNameRef = useRef<HTMLInputElement>(null);
    const coverOn = coverEnabled || coverArming;

    useEffect(() => {
        if (!appSettings?.app?.decoyMode || coverSeeded.current) return;
        coverSeeded.current = true;
        setCoverName(appSettings.app.decoyMode.displayName?.trim() || DEFAULT_COVER_NAME);
    }, [appSettings?.app?.decoyMode]);

    useEffect(() => {
        if (coverArming) {
            const id = requestAnimationFrame(() => coverNameRef.current?.select());
            return () => cancelAnimationFrame(id);
        }
    }, [coverArming]);

    const applyCover = useCallback(async (enabled: boolean, name: string) => {
        setCoverBusy(true);
        try {
            const clean = name.trim() || DEFAULT_COVER_NAME;
            await invoke("decoy_mode_set", { enable: enabled, displayName: clean });
            await refreshSettings();
            showSuccess(enabled ? `Cover identity on — app appears as "${clean}".` : "Cover identity off.");
        } catch (e) {
            showError(`Cover identity failed: ${e}`);
        } finally {
            setCoverBusy(false);
        }
    }, [refreshSettings]);

    const handleCoverToggle = useCallback((next: boolean) => {
        if (next) {
            // Don't commit a default name silently — reveal the field and wait.
            setCoverArming(true);
        } else {
            setCoverArming(false);
            if (coverEnabled) applyCover(false, coverName);
        }
    }, [coverEnabled, coverName, applyCover]);

    const commitCover = useCallback(async () => {
        await applyCover(true, coverName);
        setCoverArming(false);
    }, [applyCover, coverName]);

    // ── Hide the app ──
    const [wcBusy, setWcBusy] = useState(false);
    const [backendBusy, setBackendBusy] = useState(false);

    const handleToggleWc = useCallback(async (hidden: boolean) => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
        setWcBusy(true);
        try {
            const result = await setWinCommanderVisibility(hidden);
            if (result.success && result.data) {
                await restartExplorer().catch(() => {});
                await invoke("apply_wincommander_hide_mode", { hidden }).catch(() => {});
                await invoke("update_autostart_task_identity", { covered: hidden || lockArmed }).catch(() => {});
                patchAppSettings({ ideal: { identity: { hideWinCommander: hidden } } }).catch(reportSettingsWriteFailure);
                await refreshHardening();
                showSuccess(hidden ? "WinCommander hidden — window and tray removed." : "WinCommander visible — window and tray restored.");
            } else if (!result.success) {
                showError(result.error || "Failed to update WinCommander visibility.");
            }
        } finally {
            setWcBusy(false);
        }
    }, [setWinCommanderVisibility, restartExplorer, lockArmed, patchAppSettings, refreshHardening, needsElevation]);

    const savedHideList: string[] = appSettings?.ideal?.identity?.hideBackendAppsList ?? [];
    const allBackendHidden = BACKEND_ALL_APP_IDS.every(id => savedHideList.includes(id));

    const handleToggleBackend = useCallback(async () => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
        setBackendBusy(true);
        try {
            if (!allBackendHidden) {
                const r = await invoke<{ keys: string[]; reports: unknown[] }>("hide_runtime_list", { keys: BACKEND_EXE_NAMES });
                showSuccess(`Backend apps hidden. ${r.keys.length} app(s) processed.`);
            } else {
                await invoke("restore_all_runtimes");
                showSuccess("Backend apps restored.");
            }
            await patchAppSettings({
                ideal: { identity: { hideBackendAppsList: allBackendHidden ? [] : BACKEND_ALL_APP_IDS } },
            });
            await refreshHardening();
        } catch (e) {
            showError(`Backend visibility failed: ${e}`);
        } finally {
            setBackendBusy(false);
        }
    }, [allBackendHidden, patchAppSettings, refreshHardening, needsElevation]);

    const wcHidden = appSettings?.ideal?.identity?.hideWinCommander === true;

    return (
        <div className="dgz secret-grid__wide">
            {/* Console header — title + live posture at a glance */}
            <div className="dgz-head">
                <div className="dgz-head-title">
                    <Icon icon="lock" size={15} />
                    Change App Looks
                </div>
                <div className="dgz-chips">
                    <span className={`dgz-chip ${lockArmed ? "on" : ""}`}>
                        <span className="dgz-dot" />{lockArmed ? "Locked" : "Unlocked"}
                    </span>
                    <span className={`dgz-chip ${coverEnabled ? "on" : ""}`}>
                        <span className="dgz-dot" />{coverEnabled ? coverName : "No cover"}
                    </span>
                    <span className={`dgz-chip ${wcHidden ? "warn" : ""}`}>
                        <span className="dgz-dot" />{wcHidden ? "App hidden" : "App visible"}
                    </span>
                </div>
            </div>
            {needsElevation && <p role="alert" className="text-xs text-[var(--warn)]">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}

            {/* Calculator lock + triggers, side by side */}
            <div className="dgz-split">
                <div className="dgz-block dgz-block--col">
                    <div className="dgz-block-head">
                        <span className="dgz-block-title">Calculator lock</span>
                        <div className="sec-header-switch">
                            <span>{lockArmed ? "On" : "Off"}</span>
                            <Switch checked={lockArmed} onCheckedChange={handleToggleLock} aria-label="Calculator lock" />
                        </div>
                    </div>
                    <p className="dgz-hint">
                        Opens as a working calculator. Type a PIN, press <kbd className="sec-kbd">=</kbd>.{" "}
                        <strong>Real</strong> → full app · <strong>Decoy</strong> → Borrowed · <strong>Destroy</strong> → wipe + real calc.
                    </p>
                    <StartupPinConfig />
                </div>

                <div className="dgz-block dgz-block--col">
                    <div className="dgz-block-head">
                        <span className="dgz-block-title">Triggers</span>
                    </div>
                    <div className="dgz-field-label">Lock / unlock keywords</div>
                    <p className="dgz-hint">Type in the sidebar search to enter or leave Borrowed Mode silently.</p>
                    <div className="dgz-tile-extra">
                        <input className="sec-input" placeholder="lock" aria-label="Lock keyword" value={lockKw} onChange={e => setLockKw(e.target.value)} />
                        <input className="sec-input" placeholder="unlock" aria-label="Unlock keyword" value={unlockKw} onChange={e => setUnlockKw(e.target.value)} />
                        <button className="sec-btn" onClick={saveKeywords}>Save</button>
                    </div>
                    <div className="sec-divider" />
                    <div className="dgz-field-label">Peek hotkey</div>
                    <div className="sec-hotkey-row">
                        <Icon icon="eye-open" size={13} />
                        <span className="sec-hotkey-label">Show WinCommander while hidden</span>
                        {recordingHotkey ? (
                            <input
                                ref={hotkeyInputRef}
                                className="sec-hotkey-input"
                                placeholder="Press keys…"
                                value=""
                                onChange={() => {}}
                                onKeyDown={handleHotkeyKeyDown}
                                onBlur={() => setRecordingHotkey(false)}
                            />
                        ) : (
                            <button className="sec-hotkey-badge" onClick={() => setRecordingHotkey(true)}>
                                {hotkeyValue}
                            </button>
                        )}
                    </div>

                    {/* Disguise & hide — tiles, stacked below the peek hotkey in this column */}
                    <div className="dgz-tiles">
                        <DgzTile
                            icon="lock"
                            title="Lock panel on close"
                            desc={lockArmed
                                ? "Closing shows the calculator only. Turn off to hide to the tray (Borrowed Mode) on close instead."
                                : "Set a Real PIN to enable calculator-on-close; closing currently hides to the tray."}
                            checked={lockOnCloseChecked}
                            loading={!lockArmed}
                            onChange={(v) => patchAppSettings({ app: { lockPanelOnClose: v } }).catch(reportSettingsWriteFailure)}
                        />
                        <TierGate tier="paid" featureLabel="Cover Identity">
                            <DgzTile
                                icon="people"
                                title="Cover identity"
                                desc={
                                    coverEnabled ? `Appears as "${coverName}"`
                                    : coverArming ? "Enter a name, then press Apply"
                                    : "Disguise the app name in taskbar, Task Manager & tray"
                                }
                                checked={coverOn}
                                warn
                                loading={coverBusy}
                                onChange={handleCoverToggle}
                            >
                                {coverOn && (
                                    <div className="dgz-tile-extra">
                                        <input
                                            ref={coverNameRef}
                                            className="sec-input"
                                            placeholder="Choose a cover name…"
                                            value={coverName}
                                            disabled={coverBusy}
                                            onChange={e => setCoverName(e.target.value)}
                                            onKeyDown={e => { if (e.key === "Enter") commitCover(); }}
                                        />
                                        <button className="sec-btn" disabled={coverBusy} onClick={commitCover}>
                                            Apply
                                        </button>
                                    </div>
                                )}
                            </DgzTile>
                        </TierGate>

                        <DgzTile
                            icon="eye-off"
                            title="Hide WinCommander"
                            desc="Remove from taskbar, tray, Start & installed apps. Peek via the hotkey below."
                            checked={wcHidden}
                            warn
                            loading={wcBusy}
                            onChange={handleToggleWc}
                            disabled={wcBusy || needsElevation}
                        />
                        <DgzTile
                            icon="application"
                            title="Hide backend apps"
                            desc="Encryption/ RAMDISK Engine, Mesh VPN & other bundled tools."
                            checked={allBackendHidden}
                            loading={backendBusy}
                            onChange={handleToggleBackend}
                            disabled={backendBusy || needsElevation}
                        />
                    </div>
                </div>
            </div>

            {/* Row 2 — OS Personalization/Whitelabeling and App Licensing, side by side */}
            <div className="dgz-licensing-row">
                <BrandingLicensingSection />
                <AppLicensingSection />
            </div>
        </div>
    );
}

// ── 3. BORROWED MODE ──────────────────────────────────────────────────────────

function BorrowedModeSection() {
    const { appSettings, patchAppSettings, refreshSettings } = useAppState();
    const { hasPaid } = useEntitlements();
    const sandboxActive = (appSettings?.app?.lockedPanelIds?.length ?? 0) > 0;
    const [busy, setBusy] = useState(false);

    const handleToggleBorrowed = useCallback(async (next: boolean) => {
        setBusy(true);
        try {
            // Turning ON must seed a non-empty locked list — borrowed-active is
            // derived from lockedPanelIds.length > 0. An empty list left the
            // switch looking "on" for one frame then effectively off.
            const configured = (appSettings?.app?.lockedPanelIds ?? []) as string[];
            const ids = next
                ? (configured.length > 0 ? configured : [...DEFAULT_BORROWED_PANELS])
                : [];
            await patchAppSettings({ app: { lockedPanelIds: ids } });
            window.dispatchEvent(new CustomEvent(next ? "hidden-panels-lock" : "hidden-panels-unlock"));
            await refreshSettings();
            showSuccess(next ? "Borrowed mode on — sensitive panels hidden." : "Borrowed mode off — panels restored.");
        } catch (e) {
            showError(`Borrowed mode toggle failed: ${e}`);
        } finally {
            setBusy(false);
        }
    }, [appSettings?.app?.lockedPanelIds, patchAppSettings, refreshSettings]);

    return (
        <SectionCard
            title="Borrowed Mode"
            icon="eye-off"
            className="secret-grid__wide"
            headerRight={
                <Switch
                    checked={sandboxActive}
                    disabled={busy || !hasPaid}
                    onCheckedChange={handleToggleBorrowed}
                    aria-label="Enter or exit Borrowed Mode"
                />
            }
        >
            <TierGate tier="paid" featureLabel="Borrowed Mode">
                {sandboxActive && (
                    <UniversalCallout
                        message="Borrowed mode is active — selected panels are hidden. Flip the switch above or type the unlock keyword in the sidebar search to restore."
                        intent="warning"
                        className="mb-3"
                    />
                )}
                <div className="sec-sub">Panel &amp; feature visibility</div>
                <p className="sec-hint">
                    For each item, choose when to hide it.{" "}
                    <strong>No</strong> — always shown.{" "}
                    <strong>When borrowed</strong> — hidden only while Borrowed Mode is active.{" "}
                    <strong>Always</strong> — removed from the sidebar everywhere.
                </p>
                <VisibilityTable />
            </TierGate>
        </SectionCard>
    );
}

// ── LOCKDOWN TIMER ROW ────────────────────────────────────────────────────────

function LockdownTimerRow() {
    const { appSettings, patchAppSettings } = useAppState();
    const persisted = appSettings?.app?.lockdownTimerSec ?? 4;
    const [timerSec, setTimerSec] = useState(persisted);
    // Re-sync local value if the persisted value changes elsewhere.
    useEffect(() => { setTimerSec(persisted); }, [persisted]);
    // Debounce the disk write: range onChange fires per pixel; without this each
    // pixel was a full patch_settings_cmd IPC + disk write (a UI-freeze cause).
    const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
        const v = Math.max(3, Math.min(30, parseInt(e.target.value, 10) || 4));
        setTimerSec(v); // instant visual feedback, no IPC
        if (debounceRef.current) clearTimeout(debounceRef.current);
        debounceRef.current = setTimeout(() => {
            patchAppSettings({ app: { lockdownTimerSec: v } } as any).catch(reportSettingsWriteFailure);
        }, 250);
    }, [patchAppSettings]);
    useEffect(() => () => { if (debounceRef.current) clearTimeout(debounceRef.current); }, []);

    return (
        <div className="sec-timer-row">
            <div className="sec-timer-label">
                <Icon icon="time" size={13} />
                <span>Sidebar lockdown countdown</span>
            </div>
            <div className="sec-timer-control">
                <input
                    type="range"
                    min={3}
                    max={30}
                    step={1}
                    value={timerSec}
                    onChange={handleChange}
                    className="sec-timer-slider"
                    aria-label="Lockdown countdown seconds"
                />
                <span className="sec-timer-value">{timerSec}s</span>
            </div>
            <p className="dgz-hint">Seconds before the sidebar Lockdown button fires (3–30). Default: 4.</p>
        </div>
    );
}

// ── 5. SELF-DESTRUCT ──────────────────────────────────────────────────────────

function SelfDestructSection() {
    const { appSettings, patchAppSettings, systemInfo } = useAppState();
    const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
    const visibility = useVisibility();
    const [phraseOpen, setPhraseOpen] = useState(false);

    const panicHotkey = appSettings?.app?.panicHotkey ?? "Ctrl+Shift+Q";
    const selfDestructConfig = appSettings?.ideal?.privacy?.selfDestruct;
    const sdEnabled = selfDestructConfig?.enabled === true;
    const coercionEnabled = appSettings?.ideal?.privacy?.coercionPhrase?.enabled ?? false;
    const coercionPhrasesList = appSettings?.ideal?.privacy?.coercionPhrase?.phrases ?? [];
    // useCallback: keeps StepRow's memoized rows (LockdownConfigSection) from
    // re-rendering on every appSettings change — they only skip re-render if
    // the onToggle reference they receive is stable across renders.
    const patchSelfDestruct = useCallback((patch: any) => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
        patchAppSettings({ ideal: { privacy: { selfDestruct: patch } } } as any).catch(reportSettingsWriteFailure);
    }, [patchAppSettings, needsElevation]);
    const patchCoercion = (patch: Record<string, unknown>) =>
        patchAppSettings({ ideal: { privacy: { coercionPhrase: patch } } } as any).catch(reportSettingsWriteFailure);
    const patchFileWatch = (patch: Record<string, unknown>) =>
        patchAppSettings({ ideal: { privacy: { fileWatchTrigger: patch } } } as any).catch(reportSettingsWriteFailure);
    const savePanicHotkey = (combo: string) =>
        patchAppSettings({ app: { panicHotkey: combo } } as any).catch(reportSettingsWriteFailure);

    return (
        <SectionCard title="Lockdown" icon="warning-sign" className="secret-grid__wide">
            {needsElevation && <p role="alert" className="text-xs text-[var(--warn)]">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}
            <p className="sec-hint">
                Armed by: the Destroy PIN on the calculator, any trigger below, or the Lockdown button in the sidebar.
                Before shutting down, WinCommander restores the genuine Windows calculator so no trace of the disguise remains.
                Clearing VeraCrypt keys wipes cached passwords from memory — it does <strong>not</strong> delete your volumes.
            </p>

            {/* ── Master opt-in toggle ── */}
            <div className="dgz-tile-row sec-master-toggle">
                <div className="dgz-tile-body">
                    <div className="dgz-tile-title">Enable self-destruct</div>
                    <div className="dgz-tile-desc">
                        All trigger paths (sidebar button, hotkey, destroy PIN, dead-man's switch) are blocked until you opt in.
                    </div>
                </div>
                <Switch
                    checked={sdEnabled}
                    onCheckedChange={(v) => patchSelfDestruct({ enabled: v })}
                    disabled={needsElevation}
                    aria-label="Enable self-destruct"
                />
            </div>

            {/* ── Triggers and config — dimmed when self-destruct is off ── */}
            <div className={sdEnabled ? undefined : "sec-disabled-group"}>

            {/* ── Triggers — any one fires the lockdown ── */}
            <div className="sec-divider" />
            <div className="sec-sub">Triggers — any one fires the lockdown</div>
            <div className="lockdown-triggers-card mb-6">
                <div className="lockdown-trigger-col">
                    <PanicHotkeyTrigger hotkey={panicHotkey} onSave={savePanicHotkey} bare />
                    <FileWatchTriggerSection
                        settings={appSettings?.ideal?.privacy?.fileWatchTrigger}
                        onPatch={patchFileWatch}
                        disabled={needsElevation}
                        bare
                    />
                </div>
                <CheckInTimerSection bare />
                <LockdownWordsSection
                    isAdvanced={visibility.density === "expert"}
                    enabled={coercionEnabled}
                    phrases={coercionPhrasesList}
                    onPatch={patchCoercion}
                    expanded={phraseOpen}
                    onExpandedChange={setPhraseOpen}
                    bare
                />
            </div>

            {/* ── Countdown timer ── */}
            <div className="sec-divider" />
            <LockdownTimerRow />

            {/* ── What the lockdown deletes ── */}
            <div className="sec-divider" />
            <div className="sec-sub">What the lockdown deletes</div>
            <LockdownConfigSection searchQuery="" config={selfDestructConfig} onPatch={patchSelfDestruct} />

            </div>{/* end sec-disabled-group */}
        </SectionCard>
    );
}

function EmergencyToolsSection() {
    const { appSettings, encryptionStatus } = useAppState();

    if (getPersona(appSettings) !== "secure") return null;

    return (
        <TierGate tier="paid" featureLabel="Crypto-Erase">
            <CryptoEraseSection veracryptVolumes={encryptionStatus?.volumes ?? []} />
        </TierGate>
    );
}

// ── Panel ──────────────────────────────────────────────────────────────────────

export default function SecretPanel() {
    const [activeTab, setActiveTab] = useSecretSessionState("secret.active-tab", "disguise");

    // Deep link from PanelErrorBoundary's "Open Error Center" button (every
    // panel's crash screen navigates here via navigate-panel/"secret" first,
    // then fires this once Secret Settings has mounted) — land on Diagnostics.
    useEffect(() => {
        const openDiagnostics = () => setActiveTab("diagnostics");
        window.addEventListener("open-secret-error-center", openDiagnostics);
        return () => window.removeEventListener("open-secret-error-center", openDiagnostics);
    }, [setActiveTab]);

    return (
        <div className="panel-container secret-panel">
            <PanelHeader
                panelId="secret"
                title="Secret Settings"
                description="Calculator PINs, concealment triggers, cover identity, borrowed mode, and self-destruct — all in one place."
            />
            {/* F9: show when AD admin has pushed policy via commander.admx */}
            <ManagedPolicyBanner />
            <Tabs value={activeTab} onValueChange={setActiveTab} className="secret-tabs">
                <TabsList className="w-full flex-wrap justify-start">
                    <TabsTrigger value="disguise">Disguise &amp; branding</TabsTrigger>
                    <TabsTrigger value="borrowed">Borrowed mode &amp; visibility</TabsTrigger>
                    <TabsTrigger value="lockdown">Lockdown &amp; self-destruct</TabsTrigger>
                    <TabsTrigger value="diagnostics">Diagnostics</TabsTrigger>
                </TabsList>
                <TabsContent value="disguise">
                    <div className="secret-grid">
                        <LockDisguiseSection />
                    </div>
                </TabsContent>
                <TabsContent value="borrowed">
                    <div className="secret-grid">
                        <BorrowedModeSection />
                    </div>
                </TabsContent>
                <TabsContent value="lockdown">
                    <div className="secret-grid">
                        <SelfDestructSection />
                        <EmergencyToolsSection />
                    </div>
                </TabsContent>
                <TabsContent value="diagnostics" className="secret-diagnostics-tab">
                    <div className="secret-grid secret-diagnostics-grid">
                        <RuntimeStatusSection />
                        <SectionCard title="Error Center" icon="document" className="secret-grid__wide secret-diagnostics-log-card">
                            <LogViewer />
                        </SectionCard>
                    </div>
                </TabsContent>
            </Tabs>
        </div>
    );
}
