import { createCountdownSaver } from "./countdownSaver";
import { isImeComposing } from "@/lib/keyboard";
// src/panels/secret/index.tsx
//
// Secret Settings — rebuilt 2026-06-13, retabbed 2026-07-27.
// 4 tabs: Disguise & branding (incl. licensing) · Borrowed mode & visibility ·
// Lockdown & self-destruct · Diagnostics.
// Each tab composes one or two of the section components below. Business
// logic stays in callbacks here; the VisibilityTable for the panel-visibility
// grid lives in VisibilityTable.tsx.

import { useCallback, useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
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
import { showError, showSuccess, showWarning } from "../../utils/toast";
import { armBackend, reportArmFailure } from "../../hooks/armBackend";
import { syncCalculatorShortcuts } from "../privacy/calculatorShortcuts";
import { shortcutWarningsAfterReconciliation, syncSharedShortcutClaim } from "../../hooks/sharedShortcutClaims";
import VisibilityTable from "./VisibilityTable";
import { DEFAULT_COVER_NAME, resolveCoverName, runStealthSteps, STEALTH_LABELS, stealthPending, stealthSetupError, stealthStatus, stealthSummary, type StealthPartId, type StealthState } from "./stealthMode";
import AppLicensingSection from "./AppLicensingSection";
import BrandingLicensingSection from "./BrandingLicensingSection";
import ManagedPolicyBanner from "../../components/shared/ManagedPolicyBanner";
import { useSecretSessionState } from "./secretSessionState";
import RuntimeStatusSection from "./RuntimeStatusSection";
import SupportConsole from "./SupportConsole";
import SilentStartSetting from "./SilentStartSetting";
import ProcessElevationStatus from "./ProcessElevationStatus";
import FleetConnectView from "../fleet/FleetConnectView";
import { useFleetManagementAccess } from "../fleet/useFleetManagementAccess";
import { InfoPopover } from "../network/InfoTip";
import { DEFAULT_BORROWED_PANELS } from "../../lib/visibilityDefaults";
import "./index.css";
import "../privacy/index.css";
import "../fleet/index.css";

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
function DgzTile({ icon, title, desc, checked, warn, loading, disabled, onChange, control, children }: {
    icon: IconName;
    title: string;
    desc: string;
    checked: boolean;
    warn?: boolean;
    loading?: boolean;
    disabled?: boolean;
    onChange: (v: boolean) => void;
    control?: ReactNode;
    children?: ReactNode;
}) {
    return (
        <div className={`dgz-tile ${checked ? (warn ? "warn" : "on") : ""}`}>
            <div className="dgz-tile-row">
                <span className="dgz-tile-ico"><Icon icon={icon} size={15} /></span>
                <div className="dgz-tile-body">
                    <div className="dgz-tile-title">{title}</div>
                    {disabled && <div className="dgz-tile-desc">{desc}</div>}
                </div>
                <InfoPopover iconOnly title={title}><p>{desc}</p></InfoPopover>
                {control ?? (
                    <Switch
                        checked={checked}
                        disabled={loading || disabled}
                        onCheckedChange={onChange}
                        aria-label={title}
                        title={title}
                    />
                )}
            </div>
            {children}
        </div>
    );
}

/** One Stealth mode protection card: icon tile, state badge and switch, name, description, optional setup field. */
function StealthCard({ icon, title, desc, checked, disabled, unavailable, note, onChange, children }: {
    icon: IconName;
    title: string;
    desc: string;
    checked: boolean;
    disabled?: boolean;
    /** Dims the card: the protection cannot be used right now (the note says why). */
    unavailable?: boolean;
    note?: string;
    onChange: (v: boolean) => void;
    children?: ReactNode;
}) {
    const descId = useId();
    return (
        <div className={`dgz-stealth-card ${checked ? "on" : ""} ${unavailable ? "dim" : ""}`}>
            <div className="dgz-stealth-card-head">
                <span className="dgz-stealth-card-ico" aria-hidden="true"><Icon icon={icon} size={16} /></span>
                <div className="dgz-stealth-card-ctl">
                    <span className={`dgz-stealth-pill ${checked ? "on" : "off"}`} aria-hidden="true">
                        <span className="dgz-dot" />{checked ? "On" : "Off"}
                    </span>
                    <Switch
                        size="sm"
                        checked={checked}
                        disabled={disabled}
                        onCheckedChange={onChange}
                        aria-label={title}
                        aria-describedby={descId}
                    />
                </div>
            </div>
            <div className="dgz-stealth-card-body">
                <div className="dgz-stealth-card-title">{title}</div>
                <div className="dgz-stealth-card-desc" id={descId}>{desc}</div>
            </div>
            {note && <div className="dgz-stealth-note">{note}</div>}
            {children}
        </div>
    );
}

const AUTOSTART_POLICY_PATHS = ["autostart.enabled", "app.autostart.enabled"];
const INITIAL_AUTOSTART_STATUS_RETRIES = 3;
const INITIAL_AUTOSTART_STATUS_RETRY_DELAY_MS = 350;

function isAutostartPolicyLocked(lockedPaths: readonly string[]): boolean {
    return lockedPaths.some((rawPath) => {
        const path = rawPath.trim().toLowerCase();
        return path.length > 0 && AUTOSTART_POLICY_PATHS.some((target) => target.startsWith(path));
    });
}

function nativeErrorMessage(error: unknown, fallback: string): string {
    if (typeof error === "string" && error.trim()) return error.trim();
    if (error instanceof Error && error.message.trim()) return error.message.trim();
    return fallback;
}

type AutostartErrorPhase = "status" | "change" | "confirmation";

interface AutostartErrorState {
    phase: AutostartErrorPhase;
    message: string;
}

function AutoStartTile() {
    const { appSettings } = useAppState();
    const autostartPolicyLocked = isAutostartPolicyLocked(appSettings?.policy?.lockedPaths ?? []);
    const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);
    const [autostartLoading, setAutostartLoading] = useState(true);
    const [autostartBusy, setAutostartBusy] = useState(false);
    const [autostartError, setAutostartError] = useState<AutostartErrorState | null>(null);
    const [autostartControlVersion, setAutostartControlVersion] = useState(0);

    const readAutostartStatus = useCallback(async (retryInitialFalse = false) => {
        setAutostartLoading(true);
        setAutostartError(null);
        try {
            let enabled = await invoke<boolean>("is_autostart_enabled");
            for (let retry = 0; retryInitialFalse && !enabled && retry < INITIAL_AUTOSTART_STATUS_RETRIES; retry += 1) {
                await new Promise<void>((resolve) => window.setTimeout(resolve, INITIAL_AUTOSTART_STATUS_RETRY_DELAY_MS));
                enabled = await invoke<boolean>("is_autostart_enabled");
            }
            setAutostartEnabled(enabled);
        } catch (error) {
            setAutostartEnabled(null);
            setAutostartError({
                phase: "status",
                message: nativeErrorMessage(error, "Windows could not report the Auto Start state."),
            });
        } finally {
            setAutostartLoading(false);
        }
    }, []);

    useEffect(() => {
        void readAutostartStatus(true);
    }, [readAutostartStatus]);

    const handleToggleAutostart = useCallback(async (next: boolean) => {
        if (autostartEnabled === null || autostartBusy || autostartPolicyLocked) return;

        setAutostartBusy(true);
        setAutostartError(null);
        setAutostartControlVersion((version) => version + 1);
        let nativeOperationFinished = false;
        try {
            await invoke(next ? "enable_autostart_task" : "remove_autostart_task");
            nativeOperationFinished = true;
            const confirmed = await invoke<boolean>("is_autostart_enabled");
            if (confirmed !== next) {
                throw new Error(next
                    ? "Windows did not confirm that the sign-in task was created."
                    : "Windows did not confirm that all WinCommander startup entries were removed.");
            }
            setAutostartEnabled(confirmed);
            showSuccess(next
                ? "Auto Start on — WinCommander will use the supported Windows sign-in task."
                : "Auto Start off — all WinCommander startup entries were removed.");
        } catch (error) {
            // A failed operation can have changed Windows before its last step failed.
            setAutostartEnabled(null);
            try {
                setAutostartEnabled(await invoke<boolean>("is_autostart_enabled"));
            } catch {
                // Keep the control unknown until Windows can confirm its state.
            }
            const message = nativeErrorMessage(error, "Windows could not save the Auto Start choice.");
            const phase: AutostartErrorPhase = nativeOperationFinished ? "confirmation" : "change";
            const heading = phase === "confirmation"
                ? "Auto Start may have changed, but Windows did not confirm it"
                : "Auto Start change could not be completed";
            setAutostartError({ phase, message });
            showError(`${heading}: ${message}`);
        } finally {
            setAutostartBusy(false);
            setAutostartControlVersion((version) => version + 1);
        }
    }, [autostartBusy, autostartEnabled, autostartPolicyLocked]);

    const description = autostartLoading
        ? "Checking Windows startup…"
        : autostartBusy
            ? "Saving your Auto Start choice…"
            : autostartEnabled === true
                ? "On — Windows has automatic startup entries for WinCommander. Turn it off to remove all WinCommander startup entries."
                : autostartEnabled === false
                    ? "Off — WinCommander won't start at sign-in. Turn it on to create the supported Windows sign-in task."
                    : "Windows startup could not be checked. Try again before changing Auto Start.";

    const control = autostartLoading || autostartBusy ? (
        <span className="dgz-autostart-status" role="status" aria-live="polite">
            {autostartBusy ? "Saving…" : "Checking…"}
        </span>
    ) : autostartEnabled === null ? (
        <button className="dgz-autostart-retry" type="button" onClick={() => void readAutostartStatus(false)}>
            Try again
        </button>
    ) : (
        <Switch
            // The shared switch animates an internal thumb immediately. Remount it
            // for each request so a failed native operation cannot leave it looking changed.
            key={`autostart-${autostartEnabled}-${autostartControlVersion}`}
            checked={autostartEnabled}
            disabled={autostartPolicyLocked}
            onCheckedChange={handleToggleAutostart}
            aria-label="Start WinCommander at sign-in"
            title={autostartPolicyLocked ? "Set by your administrator." : "Start WinCommander at sign-in"}
        />
    );

    const errorHeading = autostartError?.phase === "status"
        ? "Auto Start status unavailable"
        : autostartError?.phase === "confirmation"
            ? "Auto Start may have changed, but Windows did not confirm it"
            : "Auto Start change could not be completed";

    return (
        <>
        <DgzTile
            icon="time"
            title="Auto start WinCommander"
            desc={description}
            checked={autostartEnabled === true}
            onChange={handleToggleAutostart}
            control={control}
        >
            {autostartError && (
                <p className="dgz-autostart-error" role="alert">
                    {errorHeading}: {autostartError.message}
                </p>
            )}
            {autostartPolicyLocked && <p className="dgz-autostart-note">Set by your administrator.</p>}
        </DgzTile>
        <SilentStartSetting autostartEnabled={autostartEnabled === true} />
        </>
    );
}

function LockDisguiseSection() {
    const { appSettings, patchAppSettings, refreshSettings, refreshHardening, systemInfo } = useAppState();
    const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
    const { setWinCommanderVisibility, setWinCommanderCalculatorShortcuts, restartExplorer } = useBackend();
    const { hasPaid } = useEntitlements();

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
    const [lockBusy, setLockBusy] = useState(false);
    // hideActive is passed in: a Stealth run changes Hide mode after this render read the settings.
    const applyLock = useCallback(async (next: boolean, hideActive: boolean): Promise<boolean> => {
        setLockBusy(true);
        try {
            if (next && hideActive) await invoke("require_calculator_peek_hotkey");
            await invoke("set_startup_pin_enabled", { enabled: next });
            await refreshSettings();
            const shared = await syncSharedShortcutClaim("calculator", next);
            const warning = await syncCalculatorShortcuts(setWinCommanderCalculatorShortcuts, next, shared.managed);
            const warnings = shortcutWarningsAfterReconciliation(warning ? [warning] : [], shared.warning);
            if (warnings.length || shared.warning) showWarning(`Calculator Lock changed, but shortcuts need attention: ${[...warnings, shared.warning].filter(Boolean).join(" ")}`);
            void armBackend("update_autostart_task_identity", { covered: next || hideActive }, "Autostart task identity was not updated");
            if (next) {
                try {
                    await invoke("enter_calculator_mode");
                } catch (enterError) {
                    // The lock did not actually arm: roll the persisted setting back
                    // so the toggle never reads ON while no lock is in effect.
                    reportArmFailure("enter_calculator_mode", "Calculator lock is NOT active", enterError);
                    try {
                        await invoke("set_startup_pin_enabled", { enabled: false });
                    } catch (rollbackError) {
                        reportArmFailure("set_startup_pin_enabled", "Calculator lock is saved as on but not active (rollback failed)", rollbackError);
                    }
                    await refreshSettings();
                    return false;
                }
            }
            if (!warnings.length && !shared.warning) showSuccess(next
                ? "Calculator lock on — calculator mode is active."
                : "Calculator lock off — saved PINs were kept.");
            return true;
        } catch (e) {
            showError(`Failed to update calculator lock: ${e}`);
            return false;
        } finally {
            setLockBusy(false);
        }
    }, [refreshSettings, setWinCommanderCalculatorShortcuts]);

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
        void (async () => {
            try {
                await invoke("update_hide_hotkey", { hotkey: combo });
                try {
                    await patchAppSettings({ ideal: { identity: { hideWinCommanderHotkey: combo } } });
                } catch (error) {
                    await invoke("update_hide_hotkey", { hotkey: hotkeyValue });
                    reportSettingsWriteFailure(error);
                }
            } catch (error) {
                showWarning(`Hide shortcut recovery needs attention: ${String(error)}`);
            }
        })();
    }, [patchAppSettings, hotkeyValue]);

    useEffect(() => {
        if (appSettings?.ideal?.identity?.hideWinCommander) {
            void armBackend("update_hide_hotkey", { hotkey: hotkeyValue }, "Hide shortcut is NOT active (could not bind shortcut)");
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
    // coverEnabled is derived from settings (single source of truth).
    const coverEnabled = appSettings?.app?.decoyMode?.enabled === true;
    const [coverName, setCoverName] = useState("");
    const [coverBusy, setCoverBusy] = useState(false);
    const coverSeeded = useRef(false);

    useEffect(() => {
        if (!appSettings?.app?.decoyMode || coverSeeded.current) return;
        coverSeeded.current = true;
        setCoverName(appSettings.app.decoyMode.displayName?.trim() ?? "");
    }, [appSettings?.app?.decoyMode]);

    const applyCover = useCallback(async (enabled: boolean, name: string): Promise<boolean> => {
        setCoverBusy(true);
        try {
            const clean = resolveCoverName(name);
            await invoke("decoy_mode_set", { enable: enabled, displayName: clean });
            await refreshSettings();
            showSuccess(enabled ? `Cover identity on — app appears as "${clean}".` : "Cover identity off.");
            return true;
        } catch (e) {
            showError(`Cover identity failed: ${e}`);
            return false;
        } finally {
            setCoverBusy(false);
        }
    }, [refreshSettings]);

    // ── Hide the app ──
    const [wcBusy, setWcBusy] = useState(false);
    const [backendBusy, setBackendBusy] = useState(false);

    const applyWcHidden = useCallback(async (hidden: boolean): Promise<boolean> => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return false; }
        setWcBusy(true);
        let calculatorNeedsReapply = false;
        try {
            if (hidden && lockArmed) {
                try { await invoke("require_calculator_peek_hotkey"); }
                catch (error) { showError(String(error)); return false; }
            }
            if (hidden && lockArmed) {
                // Transfer the original shortcut snapshots to Hide mode before
                // it hides them. Each feature can then be disabled independently.
                calculatorNeedsReapply = true;
                const warning = await syncCalculatorShortcuts(setWinCommanderCalculatorShortcuts, false);
                if (warning) { showWarning(`Could not prepare Calculator Lock shortcuts: ${warning}`); return false; }
            }
            const result = await setWinCommanderVisibility(hidden);
            if (result.success && result.data) {
                const shared = await syncSharedShortcutClaim("hide", hidden);
                if (lockArmed) {
                    const warning = await syncCalculatorShortcuts(setWinCommanderCalculatorShortcuts, true, shared.managed);
                    calculatorNeedsReapply = false;
                    if (warning) showWarning(`Calculator Lock shortcuts need attention: ${warning}`);
                } else if (!hidden) {
                    // A lock disabled while Hide mode was active kept its
                    // original account shortcuts in the Calculator snapshot.
                    const warning = await syncCalculatorShortcuts(setWinCommanderCalculatorShortcuts, false);
                    if (warning) showWarning(`Calculator shortcuts need attention: ${warning}`);
                }
                await restartExplorer().catch(() => {});
                await invoke("apply_wincommander_hide_mode", { hidden }).catch((error) => showWarning(`Hide shortcut recovery needs attention: ${String(error)}`));
                await armBackend("update_autostart_task_identity", { covered: hidden || lockArmed }, "Autostart task identity was not updated");
                patchAppSettings({ ideal: { identity: { hideWinCommander: hidden } } }).catch(reportSettingsWriteFailure);
                await refreshHardening();
                const warnings = shortcutWarningsAfterReconciliation(result.data.warnings ?? [], shared.warning);
                if (warnings.length || shared.warning) {
                    showWarning(`Visibility changed, but shortcuts or settings need attention: ${[...warnings, shared.warning].filter(Boolean).join(" ")}`);
                } else {
                    showSuccess(hidden ? "WinCommander hidden — window and tray removed." : "WinCommander visible — window and tray restored.");
                }
                return true;
            }
            if (!result.success) showError(result.error || "Failed to update WinCommander visibility.");
            return false;
        } finally {
            if (calculatorNeedsReapply) {
                const shared = await syncSharedShortcutClaim("calculator", true);
                const warning = await syncCalculatorShortcuts(setWinCommanderCalculatorShortcuts, true, shared.managed);
                const warnings = shortcutWarningsAfterReconciliation(warning ? [warning] : [], shared.warning);
                if (warnings.length || shared.warning) showWarning(`Calculator Lock shortcuts need attention: ${[...warnings, shared.warning].filter(Boolean).join(" ")}`);
            }
            setWcBusy(false);
        }
    }, [setWinCommanderVisibility, setWinCommanderCalculatorShortcuts, restartExplorer, lockArmed, patchAppSettings, refreshHardening, needsElevation]);

    const savedHideList: string[] = appSettings?.ideal?.identity?.hideBackendAppsList ?? [];
    const allBackendHidden = BACKEND_ALL_APP_IDS.every(id => savedHideList.includes(id));

    const applyBackendHidden = useCallback(async (hide: boolean): Promise<boolean> => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return false; }
        setBackendBusy(true);
        try {
            if (hide) {
                const r = await invoke<{ keys: string[]; reports: unknown[] }>("hide_runtime_list", { keys: BACKEND_EXE_NAMES });
                showSuccess(`Backend apps hidden. ${r.keys.length} app(s) processed.`);
            } else {
                await invoke("restore_all_runtimes");
                showSuccess("Backend apps restored.");
            }
            await patchAppSettings({
                ideal: { identity: { hideBackendAppsList: hide ? BACKEND_ALL_APP_IDS : [] } },
            });
            await refreshHardening();
            return true;
        } catch (e) {
            showError(`Backend visibility failed: ${e}`);
            return false;
        } finally {
            setBackendBusy(false);
        }
    }, [patchAppSettings, refreshHardening, needsElevation]);

    const wcHidden = appSettings?.ideal?.identity?.hideWinCommander === true;

    // ── Stealth mode: one switch for the four features above ──
    const [stealthBusy, setStealthBusy] = useState(false);
    const [stealthPin, setStealthPin] = useState("");
    const stealthState = useMemo<StealthState>(
        () => ({ lock: lockArmed, cover: coverEnabled, hide: wcHidden, backend: allBackendHidden }),
        [lockArmed, coverEnabled, wcHidden, allBackendHidden],
    );
    const stealth = stealthSummary(stealthState, hasPaid);
    const anyStepBusy = stealthBusy || lockBusy || coverBusy || wcBusy || backendBusy;

    const registerRealPin = useCallback(async (): Promise<boolean> => {
        const pinError = stealthSetupError({ hasRealPin: false, pin: stealthPin });
        if (pinError) { showError(pinError); return false; }
        try {
            if (wcHidden) await invoke("require_calculator_peek_hotkey");
            await invoke("register_startup_pin", { mode: "real", plaintext: stealthPin.trim() });
            setStealthPin("");
            await refreshSettings();
            return true;
        } catch (e) {
            showError(String(e));
            return false;
        }
    }, [stealthPin, wcHidden, refreshSettings]);

    const toggleLock = useCallback(async (next: boolean) => {
        setStealthBusy(true);
        try {
            if (next && !hasRealPin && !(await registerRealPin())) return;
            await applyLock(next, wcHidden);
        } finally {
            setStealthBusy(false);
        }
    }, [hasRealPin, registerRealPin, applyLock, wcHidden]);

    const toggleCover = useCallback(async (next: boolean) => {
        await applyCover(next, coverName);
    }, [coverName, applyCover]);

    const runStealth = useCallback(async (direction: "on" | "off") => {
        const on = direction === "on";
        const pending = stealthPending(stealthState, hasPaid, direction);
        if (pending.some((id) => id === "hide" || id === "backend") && needsElevation) {
            showError(MACHINE_SCOPE_ELEVATION_MESSAGE);
            return;
        }
        if (on) {
            const setupError = stealthSetupError({ hasRealPin, pin: stealthPin });
            if (setupError) { showError(setupError); return; }
            // Checked up front: the lock step runs after Hide, and a taken peek hotkey would leave the app hidden with no way back.
            if (pending.includes("lock") && (wcHidden || pending.includes("hide"))) {
                try { await invoke("require_calculator_peek_hotkey"); } catch (error) { showError(String(error)); return; }
            }
        }
        setStealthBusy(true);
        try {
            if (on && !hasRealPin && !(await registerRealPin())) return;
            let hideNow = wcHidden;
            const run = async (id: StealthPartId): Promise<boolean> => {
                if (id === "cover") return applyCover(on, coverName);
                if (id === "backend") return applyBackendHidden(on);
                if (id === "hide") {
                    const ok = await applyWcHidden(on);
                    if (ok) hideNow = on;
                    return ok;
                }
                return applyLock(on, hideNow);
            };
            const result = await runStealthSteps(pending, run);
            if (result.failed) {
                showError(`Stealth mode stopped at "${STEALTH_LABELS[result.failed]}" (${result.done.length} of ${pending.length} steps done). Fix that, then press "Turn on the rest", or switch the others on one by one.`);
            } else if (pending.length > 0) {
                showSuccess(on ? "Stealth mode on." : "Stealth mode off — saved PINs were kept.");
            }
        } finally {
            setStealthBusy(false);
        }
    }, [stealthState, hasPaid, needsElevation, hasRealPin, stealthPin, coverName, wcHidden, registerRealPin, applyCover, applyBackendHidden, applyWcHidden, applyLock]);

    const stealthPill = stealthStatus(stealth, stealthBusy);
    const adminNote = needsElevation ? "Needs administrator" : undefined;

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
                        <span className="dgz-dot" />{coverEnabled ? resolveCoverName(appSettings?.app?.decoyMode?.displayName ?? "") : "No cover"}
                    </span>
                    <span className={`dgz-chip ${wcHidden ? "warn" : ""}`}>
                        <span className="dgz-dot" />{wcHidden ? "App hidden" : "App visible"}
                    </span>
                </div>
            </div>
            {needsElevation && <p role="alert" className="text-xs text-[var(--warn)]">{MACHINE_SCOPE_ELEVATION_MESSAGE}</p>}

            {/* Stealth mode: four protections, each switchable alone or all at once */}
            <section className={`dgz-block dgz-stealth ${stealth.mode}`} aria-labelledby="stealth-title">
                <div className="dgz-stealth-hero">
                    <span className="dgz-stealth-hero-ico" aria-hidden="true"><Icon icon="shield" size={18} /></span>
                    <div className="dgz-stealth-hero-text">
                        <h3 className="dgz-stealth-title" id="stealth-title">Stealth mode</h3>
                        <p className="dgz-stealth-lead">Hide WinCommander behind a calculator and a cover name: all at once, or one protection at a time.</p>
                    </div>
                    <div className="dgz-stealth-hero-actions">
                        <span className={`dgz-stealth-pill ${stealthPill.tone}`} role="status" aria-live="polite">
                            <span className="dgz-dot" />{stealthPill.label}
                        </span>
                        {stealth.mode === "partial" && (
                            <button className="sec-btn" type="button" disabled={anyStepBusy} onClick={() => void runStealth("on")}>
                                Turn on the rest
                            </button>
                        )}
                        <Switch
                            checked={stealth.mode !== "off"}
                            disabled={anyStepBusy}
                            onCheckedChange={(next) => { void runStealth(next ? "on" : "off"); }}
                            aria-label="Turn all stealth protections on or off"
                        />
                    </div>
                </div>
                <div className="dgz-stealth-grid">
                    <StealthCard
                        icon="lock"
                        title="Calculator lock"
                        desc="Opens as a working calculator. Type your PIN and press = to open WinCommander."
                        checked={lockArmed}
                        disabled={anyStepBusy}
                        onChange={(next) => { void toggleLock(next); }}
                    >
                        {!hasRealPin && (
                            <div className="dgz-stealth-setup">
                                <label className="dgz-stealth-setup-label" htmlFor="stealth-pin">
                                    Calculator PIN<span className="dgz-stealth-req">* required</span>
                                </label>
                                <input
                                    id="stealth-pin"
                                    className="sec-input"
                                    type="password"
                                    inputMode="numeric"
                                    maxLength={12}
                                    required
                                    aria-required="true"
                                    aria-describedby="stealth-pin-hint"
                                    placeholder="e.g. 4829"
                                    value={stealthPin}
                                    disabled={anyStepBusy}
                                    onChange={e => setStealthPin(e.target.value.replace(/\D/g, "").slice(0, 12))}
                                />
                                <p className="dgz-stealth-setup-hint" id="stealth-pin-hint">At least 4 digits, not starting with 0. Saved when you switch the lock on.</p>
                            </div>
                        )}
                    </StealthCard>
                    <TierGate
                        tier="paid"
                        featureLabel="Cover Identity"
                        fallback={
                            <StealthCard
                                icon="people"
                                title="Cover identity"
                                desc="Shows a different name in the taskbar, Task Manager and tray."
                                checked={false}
                                disabled
                                unavailable
                                note="Needs WinCommander Pro"
                                onChange={() => {}}
                            >
                                <div className="dgz-stealth-setup">
                                    <button
                                        className="sec-btn"
                                        type="button"
                                        onClick={() => window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Cover Identity" } }))}
                                    >
                                        Unlock with WinCommander Pro
                                    </button>
                                </div>
                            </StealthCard>
                        }
                    >
                        <StealthCard
                            icon="people"
                            title="Cover identity"
                            desc="Shows a different name in the taskbar, Task Manager and tray."
                            checked={coverEnabled}
                            disabled={anyStepBusy}
                            onChange={(next) => { void toggleCover(next); }}
                        >
                            <div className="dgz-stealth-setup">
                                <label className="dgz-stealth-setup-label" htmlFor="stealth-cover-name">Cover name</label>
                                <div className="dgz-stealth-setup-fields">
                                    <input
                                        id="stealth-cover-name"
                                        className="sec-input"
                                        aria-describedby="stealth-cover-hint"
                                        placeholder={DEFAULT_COVER_NAME}
                                        value={coverName}
                                        disabled={anyStepBusy}
                                        onChange={e => setCoverName(e.target.value)}
                                        onKeyDown={e => { if (e.key === "Enter" && coverEnabled && !isImeComposing(e.nativeEvent)) void applyCover(true, coverName); }}
                                    />
                                    {coverEnabled && (
                                        <button className="sec-btn" type="button" disabled={anyStepBusy} onClick={() => void applyCover(true, coverName)}>
                                            Save name
                                        </button>
                                    )}
                                </div>
                                <p className="dgz-stealth-setup-hint" id="stealth-cover-hint">Left empty, WinCommander shows as {DEFAULT_COVER_NAME}.</p>
                            </div>
                        </StealthCard>
                    </TierGate>
                    <StealthCard
                        icon="eye-off"
                        title="Hide WinCommander"
                        desc="Gone from the taskbar, tray, Start and installed apps. Reopen with the peek hotkey below."
                        checked={wcHidden}
                        disabled={anyStepBusy || needsElevation}
                        unavailable={needsElevation}
                        note={adminNote}
                        onChange={(next) => { void applyWcHidden(next); }}
                    />
                    <StealthCard
                        icon="application"
                        title="Hide backend apps"
                        desc="Hides the Encryption / RAMDISK Engine, Mesh VPN and other bundled tools."
                        checked={allBackendHidden}
                        disabled={anyStepBusy || needsElevation}
                        unavailable={needsElevation}
                        note={adminNote}
                        onChange={(next) => { void applyBackendHidden(next); }}
                    />
                </div>
            </section>

            {/* Calculator PINs + triggers, side by side */}
            <div className="dgz-split">
                <div className="dgz-block dgz-block--col">
                    <div className="dgz-block-head">
                        <span className="dgz-block-title">Calculator lock</span>
                        <div className="sec-header-switch">
                            <span>{lockBusy ? "Updating…" : lockArmed ? "On" : "Off"}</span>
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
                    <p className="dgz-hint">When Hide is on, press the shortcut twice within two seconds to reopen WinCommander after it has been ended.</p>
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
                        <AutoStartTile />
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

function FleetSettingsCard() {
    const { canManage } = useFleetManagementAccess();
    const wasAuthorized = useRef(false);
    if (canManage) wasAuthorized.current = true;
    return wasAuthorized.current ? <section className="fleet-panel" aria-label="Fleet settings" hidden={!canManage}><FleetConnectView /></section> : null;
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
    const [timerInput, setTimerInput] = useState(String(persisted));
    // Re-sync local value if the persisted value changes elsewhere.
    useEffect(() => { setTimerSec(persisted); setTimerInput(String(persisted)); }, [persisted]);
    const patchRef = useRef(patchAppSettings);
    patchRef.current = patchAppSettings;
    const saverRef = useRef<ReturnType<typeof createCountdownSaver> | null>(null);
    if (!saverRef.current) {
        saverRef.current = createCountdownSaver((v) => {
            patchRef.current({ app: { lockdownTimerSec: v } } as any).catch(reportSettingsWriteFailure);
        });
    }
    const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
        const v = Math.max(3, Math.min(86400, parseInt(e.target.value, 10) || 4));
        setTimerSec(v);
        setTimerInput(String(v));
        saverRef.current?.queue(v);
    }, []);
    useEffect(() => () => saverRef.current?.flush(), []);

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
                    value={Math.min(30, timerSec)}
                    onChange={handleChange}
                    className="sec-timer-slider"
                    aria-label="Lockdown countdown seconds"
                />
                <input type="number" min={3} max={86400} step={1} value={timerInput} onChange={(e) => { const raw = e.currentTarget.value; setTimerInput(raw); const value = Number(raw); if (/^\d+$/.test(raw) && value >= 3 && value <= 86400) { setTimerSec(value); saverRef.current?.queue(value); } }} onBlur={(e) => handleChange(e)} onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }} aria-label="Custom lockdown countdown in seconds" className="sec-timer-input" /> <span>sec</span>
            </div>
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
                Clearing encryption keys wipes cached passwords from memory — it does <strong>not</strong> delete your volumes.
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

    // Deep link from PanelErrorBoundary's "Open Diagnostic Center" button (every
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
            <ProcessElevationStatus />
            <Tabs value={activeTab} onValueChange={setActiveTab} className="secret-tabs">
                <TabsList className="w-full flex-wrap justify-start">
                    <TabsTrigger value="disguise">Disguise &amp; branding</TabsTrigger>
                    <TabsTrigger value="borrowed">Borrowed mode &amp; visibility</TabsTrigger>
                    <TabsTrigger value="lockdown">Lockdown &amp; self-destruct</TabsTrigger>
                    <TabsTrigger value="diagnostics">Logs</TabsTrigger>
                </TabsList>
                <TabsContent value="disguise">
                    <div className="secret-grid">
                        <LockDisguiseSection />
                        <FleetSettingsCard />
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
                        <SectionCard title="Logs" icon="document" className="secret-grid__wide secret-diagnostics-log-card">
                            <SupportConsole />
                        </SectionCard>
                    </div>
                </TabsContent>
            </Tabs>
        </div>
    );
}
