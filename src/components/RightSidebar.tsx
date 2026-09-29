import { Icon } from "./ui/icon";
import { Spinner } from "./ui/spinner";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "./ui/dialog";
import { cn } from "../lib/utils";
import { useAppState } from "../context/AppContext";
import { reportSettingsWriteFailure } from "../lib/settingsWriteRecovery";
import { isPrivilegedWriteBlocked, MACHINE_SCOPE_ELEVATION_MESSAGE } from "../lib/machineScopeElevation";
import { getDisplayBranding } from "../lib/branding";
import useBackend, { type EncryptionPartition } from "../hooks/useBackend";
import type { QuickMountSlot } from "../types/settings";
import useVisibility from "../hooks/useVisibility";
import useEntitlements from "../hooks/useEntitlements";
import useBorrowedActive from "../hooks/useBorrowedActive";
import { lazy, Suspense, useState, useEffect, useCallback, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { showSuccess, showError } from "../utils/toast";
import { selectableDriveLetters, vaultOperationError } from "@/lib/vaultOperationFeedback";
import { runOperation } from "../context/OperationContext";
import { DESTRUCT_STEPS, isStepEnabled } from "../types/lockdownSteps";
import { DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS, DEFAULT_BORROWED_EXTRAS } from "../lib/visibilityDefaults";
import { requestDestructiveCapability } from "../hooks/destructiveAuthz";
import { invalidateDiskCleanupScheduleStatus } from "../panels/maintenance/diskCleanupScheduleState";
import { useActiveTourStepId, useLockdownChoicePendingEnabled } from "../lib/tourActive";
import useVaultAccess, { FLEET_VAULTS_CHANGED_EVENT } from "../hooks/useVaultAccess";
import { vaultMountResultLabel, type VaultAuthorizedEntry } from "../panels/fleet/vaultAccessTypes";
import './RightSidebar.css';

// The service can add a preferred mount letter without exposing a container
// path. Keep this narrow display-only extension local to the sidebar until
// the generated renderer contract includes the optional projection field.
type FleetQuickMountEntry = VaultAuthorizedEntry & { preferred_letter?: string | null };

// This large, occasional dialog carries its own legacy UI bridge; keep it out
// of the always-visible quick-action rail until the operator requests it.
const MetadataScrubberDialog = lazy(() => import("./MetadataScrubberDialog"));

// ── Lockdown countdown audio cues (Web Audio — no bundled assets) ──────────
// A short beep on each tick (3-2-1), a low "destruct" tone at zero, and a soft
// confirm tone on abort. Best-effort: silently no-ops if Web Audio is blocked.
let _lockdownAudioCtx: AudioContext | null = null;

const ACTION_LABELS: Record<string, string> = {
    dismount: "Volumes & RAM disks dismounted",
};
function lockdownTone(freq: number, durationMs: number, type: OscillatorType = "sine", gain = 0.14) {
    try {
        const Ctx = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
        if (!Ctx) return;
        _lockdownAudioCtx = _lockdownAudioCtx ?? new Ctx();
        const ctx = _lockdownAudioCtx;
        const osc = ctx.createOscillator();
        const g = ctx.createGain();
        osc.type = type;
        osc.frequency.value = freq;
        osc.connect(g);
        g.connect(ctx.destination);
        const now = ctx.currentTime;
        g.gain.setValueAtTime(gain, now);
        g.gain.exponentialRampToValueAtTime(0.0001, now + durationMs / 1000);
        osc.start(now);
        osc.stop(now + durationMs / 1000);
    } catch {
        /* audio is best-effort */
    }
}
// Escalating tones: 4 = calm warning, 3 = building, 2 = urgent, 1 = critical
const COUNTDOWN_TONES: Record<number, [number, number]> = {
    4: [400, 120],
    3: [500, 130],
    2: [630, 145],
    1: [780, 165],
};
function lockdownCountdownBeep(count: number) {
    const [freq, dur] = COUNTDOWN_TONES[count] ?? [680, 130];
    lockdownTone(freq, dur, "square", 0.1);
}
const lockdownFire = () => lockdownTone(150, 700, "sawtooth", 0.22);
const lockdownAbort = () => lockdownTone(520, 220, "sine", 0.12);

// V2 rail action button (replaces Blueprint <ActionBtn> in the right rail).
// Accepts the Blueprint-ish props the call sites pass; only the relevant
// ones are used (minimal/large are layout no-ops here).
interface ActionBtnProps {
    icon: string;
    className?: string;
    intent?: "danger" | "primary" | "success" | "warning" | "none" | string;
    minimal?: boolean;
    large?: boolean;
    loading?: boolean;
    disabled?: boolean;
    onClick?: () => void;
    ariaLabel: string;
}

function ActionBtn({ icon, className, intent, loading, disabled, onClick, ariaLabel }: ActionBtnProps) {
    return (
        <button
            type="button"
            className={cn("action-btn", intent === "danger" && "action-btn--danger", className)}
            disabled={disabled}
            onClick={onClick}
            aria-label={ariaLabel}
        >
            {loading ? <Spinner size={20} /> : <Icon icon={icon} size={20} />}
        </button>
    );
}

export default function RightSidebar() {
    const {
        refreshVault,
        appSettings,
        patchAppSettings,
        systemInfo,
    } = useAppState();
    const needsElevation = isPrivilegedWriteBlocked(true, systemInfo?.isAdmin);
    const { productName } = getDisplayBranding(appSettings);

    // The 17-task parallel orchestration was replaced by the universal
    // `full_lockdown` Rust command (see fireSelfDestruct below).
    // The hooks below remain because other sidebar surfaces (the
    // individual quick-action buttons) still use them directly.
    const {
        dismountAllVolumes,
        removeAllRamDisks,
        getAutoEraseSchedules,
        removeAutoEraseSchedule,
        mountVolume,
        verifyVaultDrive,
        getEncryptionPartitions,
        getAvailableDriveLetters,
        openEncryptionVolume,
        safePastePrepare,
    } = useBackend();

    const visibility = useVisibility();
    const { canUse } = useEntitlements();
    const borrowedActive = useBorrowedActive();
    const activeTourStepId = useActiveTourStepId();
    const pendingLockdownEnabled = useLockdownChoicePendingEnabled();
    const lockdownChoiceTourActive = activeTourStepId === "dashboard-tour-lockdown-choice";
    const lockdownEnabled = appSettings?.ideal?.privacy?.selfDestruct?.enabled === true;
    const lockdownVisibleInRail = pendingLockdownEnabled ?? lockdownEnabled;
    const borrowedHidden = appSettings?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS;
    // Quick actions the user has hidden via Secret Settings ▸ Sidebar actions.
    // An action is also hidden when Borrowed Mode is active and its key is in
    // borrowedHidden (key format: "action:<key>").
    const hiddenActions = new Set([
        ...(appSettings?.app?.hiddenSidebarActions ?? DEFAULT_ALWAYS_HIDDEN_SIDEBAR_ACTIONS),
        ...(borrowedActive
            ? borrowedHidden
                .filter(k => k.startsWith("action:"))
                .map(k => k.slice("action:".length))
            : []),
    ]);
    // The final tour step gets a disabled preview in the real footer position
    // while Lockdown is off. If the user opts in, reveal the regular control
    // for the rest of the tour even when a sidebar visibility preference had
    // hidden it; the preference resumes as soon as the tour closes.
    const showLockdownControl = lockdownVisibleInRail && (!hiddenActions.has("lockdown") || lockdownChoiceTourActive);
    const showLockdownImpression = lockdownChoiceTourActive && !lockdownVisibleInRail;
    // Do not wait for the delayed dependency/vault probes before making the
    // emergency control available. Both backend operations are idempotent and
    // report an empty/not-installed state safely, while the old probe-derived
    // gate left a usable Dismount action disabled for up to a couple of minutes.
    const dismountAvailable = true;

    const [loadingAction, setLoadingAction] = useState<string | null>(null);
    // Manual Lockdown/Self-Destruct button + 4s abort countdown — restored
    // 2026-06-09 (owner). The button was removed in the redesign; the cascade
    // only fired via hotkey/coercion events. sdCountdownRef lets the
    // event listeners see the live countdown without re-subscribing.
    const [sdCountdown, setSdCountdown] = useState<number | null>(null);
    // Whether to show the full-screen countdown POPUP. True only when the
    // lockdown was armed from the right-sidebar CLICK; hotkey-armed countdowns
    // run silently with just the on-rail label (owner request).
    const [sdPopup, setSdPopup] = useState(false);
    const sdIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const sdCountdownRef = useRef<number | null>(null);
    const sdRustOwnedRef = useRef(false);
    // true when the active countdown was armed by hotkey or coercion — no audio.
    const sdSilentRef = useRef<boolean>(false);
    const [scrubDialogOpen, setScrubDialogOpen] = useState(false);
    // ── Quick Mount ──────────────────────────────────────────────────────────
    const [qmOpen, setQmOpen] = useState(false);
    const [qmSelectedIdx, setQmSelectedIdx] = useState(0);
    const [qmPassword, setQmPassword] = useState('');
    const [qmMountingIdx, setQmMountingIdx] = useState<number | null>(null);
    // null = mount view; object = editing/adding a slot
    const [qmEditing, setQmEditing] = useState<{ idx: number | 'new'; path: string; letter: string; targetType: 'file' | 'partition' } | null>(null);
    const [qmPartitions, setQmPartitions] = useState<EncryptionPartition[]>([]);
    const [qmPartitionsLoading, setQmPartitionsLoading] = useState(false);
    const [qmSaving, setQmSaving] = useState(false);
    const [qmFeedback, setQmFeedback] = useState('');
    const [qmMountedDrive, setQmMountedDrive] = useState<string | null>(null);
    const [qmAvailableLetters, setQmAvailableLetters] = useState<string[]>([]);
    const [qmLettersLoading, setQmLettersLoading] = useState(false);
    const [dismountFailure, setDismountFailure] = useState('');
    // Fleet Vaults never use this user-settings list: the secure service owns
    // the caller-filtered projection and takes only an opaque entry id on mount.
    const [fleetVaults, setFleetVaults] = useState<FleetQuickMountEntry[]>([]);
    const [fleetVaultsLoading, setFleetVaultsLoading] = useState(false);
    const [fleetVaultEntryId, setFleetVaultEntryId] = useState('');
    const [fleetVaultPassword, setFleetVaultPassword] = useState('');
    const [fleetVaultMounting, setFleetVaultMounting] = useState(false);
    const { listAuthorizedEntries, mountEntry: mountFleetVaultEntry } = useVaultAccess<unknown, unknown>();
    type QmSlot = QuickMountSlot;
    // useMemo so the array identity is stable across renders — otherwise the
    // `?? []` fallback minted a fresh [] every render, churning the deps of
    // every useCallback below that closes over it.
    const quickMountSlots: QmSlot[] = useMemo(
        () => appSettings?.app?.vault?.quickMountSlots ?? [],
        [appSettings?.app?.vault?.quickMountSlots],
    );

    const refreshFleetVaults = useCallback(async () => {
        setFleetVaultsLoading(true);
        try {
            const entries = await listAuthorizedEntries();
            setFleetVaults(entries);
            setFleetVaultEntryId(current => entries.some(entry => entry.entry_id === current) ? current : (entries[0]?.entry_id ?? ''));
        } catch {
            // The normal Quick Mount shortcuts remain usable if the secure
            // Fleet service is briefly unavailable. Never substitute cached
            // policy/path data for this caller-filtered list.
            setFleetVaults([]);
            setFleetVaultEntryId('');
        } finally {
            setFleetVaultsLoading(false);
        }
    }, [listAuthorizedEntries]);

    useEffect(() => {
        if (qmOpen) void refreshFleetVaults();
    }, [qmOpen, refreshFleetVaults]);

    const refreshQmLetters = useCallback(async () => {
        setQmLettersLoading(true);
        try {
            const result = await getAvailableDriveLetters();
            if (!result.success || !result.data) throw new Error();
            const letters = selectableDriveLetters(result.data.letters);
            setQmAvailableLetters(letters);
            return letters;
        } catch {
            setQmAvailableLetters([]);
            setQmFeedback('Free drive letters could not be checked. Refresh the list before saving or mounting.');
            return [];
        } finally { setQmLettersLoading(false); }
    }, [getAvailableDriveLetters]);

    useEffect(() => { if (qmOpen) void refreshQmLetters(); }, [qmOpen, qmEditing?.idx, refreshQmLetters]);
    const qmLetterChoices = selectableDriveLetters(qmAvailableLetters, quickMountSlots
        .filter((_, index) => qmEditing?.idx === 'new' || index !== qmEditing?.idx)
        .map(slot => slot.driveLetter));

    const openVaultDrive = async (drive: string) => {
        try {
            const result = await openEncryptionVolume(drive);
            if (!result.success) throw new Error(result.error);
        } catch (error) { setQmFeedback(vaultOperationError(error, 'open')); }
    };

    useEffect(() => {
        const refreshAfterFleetVaultSave = () => { void refreshFleetVaults(); };
        window.addEventListener(FLEET_VAULTS_CHANGED_EVENT, refreshAfterFleetVaultSave);
        return () => window.removeEventListener(FLEET_VAULTS_CHANGED_EVENT, refreshAfterFleetVaultSave);
    }, [refreshFleetVaults]);

    const handleFleetVaultMount = useCallback(async () => {
        if (!fleetVaultEntryId || !fleetVaultPassword || fleetVaultMounting) return;
        setFleetVaultMounting(true);
        setQmFeedback('');
        setQmMountedDrive(null);
        try {
            const result = await mountFleetVaultEntry(fleetVaultEntryId, fleetVaultPassword, 'outer');
            if (result.state === 'mounted') {
                showSuccess(vaultMountResultLabel(result));
                setFleetVaultPassword('');
                setQmFeedback(vaultMountResultLabel(result));
                await refreshFleetVaults();
            } else {
                setQmFeedback(vaultMountResultLabel(result));
                showError(vaultMountResultLabel(result), undefined, { kind: 'notification' });
                if (result.reason === 'already_mounted') await refreshFleetVaults();
            }
        } catch (error) {
            const message = vaultOperationError(error);
            setQmFeedback(message);
            showError(message, undefined, { kind: 'notification' });
        } finally {
            setFleetVaultPassword('');
            setFleetVaultMounting(false);
        }
    }, [fleetVaultEntryId, fleetVaultMounting, fleetVaultPassword, mountFleetVaultEntry, refreshFleetVaults]);

    const nextFreeLetter = useCallback((excludeIdx?: number) => {
        const taken = new Set(
            quickMountSlots
                .filter((_, i) => i !== excludeIdx)
                .map(s => s.driveLetter.toUpperCase())
        );
        return qmAvailableLetters.find(l => !taken.has(l)) ?? '';
    }, [quickMountSlots, qmAvailableLetters]);

    const handleQmOpen = useCallback(() => {
        setQmPassword('');
        setFleetVaultPassword('');
        setQmFeedback('');
        setQmMountedDrive(null);
        setQmSelectedIdx(0);
        // A service-owned Fleet Vault is a mount target immediately after its
        // policy is saved.  Do not force the personal-shortcut editor merely
        // because this Windows account has no local Quick Mount slots.
        setQmEditing(null);
        setQmOpen(true);
    }, []);

    const patchQmSlots = useCallback(async (slots: QmSlot[]) => {
        setQmSaving(true);
        try {
            await patchAppSettings({ app: { vault: { quickMountSlots: slots } } });
            return true;
        } catch {
            setQmFeedback('The shortcut could not be saved. Your changes are still here; try saving again.');
            showError('The Quick Mount shortcut could not be saved.');
            return false;
        }
        finally { setQmSaving(false); }
    }, [patchAppSettings]);

    const handleQmSaveSlot = useCallback(async () => {
        if (!qmEditing) return;
        const path = qmEditing.path.trim();
        const letter = qmEditing.letter.trim().toUpperCase().slice(0, 1);
        if (!path || !letter) return;
        setQmFeedback('');
        const available = await refreshQmLetters();
        if (!available.includes(letter)) {
            setQmFeedback(`Drive ${letter}: is in use or reserved. Select a free letter from the list.`);
            return;
        }
        // Block duplicate drive letters across slots
        const clash = quickMountSlots.some((s, i) =>
            s.driveLetter.toUpperCase() === letter &&
            (qmEditing.idx === 'new' || i !== (qmEditing.idx as number))
        );
        if (clash) {
            setQmFeedback(`Drive ${letter}: is already used by another shortcut. Pick a different letter.`);
            showError(`Drive ${letter}: is already used by another vault. Pick a different letter.`);
            return;
        }
        const next = [...quickMountSlots];
        const saved: QmSlot = { filePath: path, driveLetter: letter, targetType: qmEditing.targetType };
        if (qmEditing.idx === 'new') next.push(saved);
        else next[qmEditing.idx as number] = saved;
        if (await patchQmSlots(next)) setQmEditing(null);
    }, [qmEditing, quickMountSlots, patchQmSlots, refreshQmLetters]);

    const loadQmPartitions = useCallback(async () => {
        setQmPartitionsLoading(true);
        try {
            const result = await getEncryptionPartitions();
            if (result?.success) setQmPartitions(result.data?.partitions ?? []);
            else showError(result?.error || 'Could not find encrypted partitions.');
        } catch { showError('Could not find encrypted partitions.'); }
        finally { setQmPartitionsLoading(false); }
    }, [getEncryptionPartitions]);

    const handleQmRemoveSlot = useCallback(async (idx: number) => {
        const next = quickMountSlots.filter((_, i) => i !== idx);
        if (await patchQmSlots(next)) setQmSelectedIdx(0);
    }, [quickMountSlots, patchQmSlots]);

    const handleQmMount = useCallback(async () => {
        const slot = quickMountSlots[qmSelectedIdx];
        if (!slot || !qmPassword) return;
        setQmMountingIdx(qmSelectedIdx);
        setQmFeedback('');
        setQmMountedDrive(null);
        try {
            const status = await refreshVault(true);
            const existing = status?.volumes?.find(volume => volume.accessible !== false && volume.path?.toLowerCase() === slot.filePath.toLowerCase());
            if (existing) {
                setQmFeedback(`This container is already mounted at ${existing.letter}.`);
                setQmMountedDrive(existing.letter);
                return;
            }
            const available = await refreshQmLetters();
            if (!available.includes(slot.driveLetter.toUpperCase())) throw new Error('Drive letter is in use or reserved');
            const r = await mountVolume({
              volumePath: slot.filePath,
              driveLetter: slot.driveLetter,
              volumeKind: "standard",
              volumeRole: "standard",
              password: qmPassword,
              // The shortcut is an ordinary personal mount, so it must retain
              // the same read/write capability the user expects in Explorer.
              // Hidden-volume protection is not enabled for this flow.
              readOnly: false,
              protectHidden: false,
              scope: "machine",
              hardenAcl: true,
            });
            if (r?.success && r.data?.scope === "machine") {
                await verifyVaultDrive(r.data.drive);
                const refreshed = await refreshVault(true);
                const isVisibleInThisSession = refreshed?.volumes?.some((volume) =>
                    volume.letter === r.data?.drive
                    && volume.internalDrive === r.data?.internalDrive,
                );
                if (!isVisibleInThisSession) {
                    throw new Error("The encrypted volume was not available in this signed-in Windows session.");
                }
                showSuccess(`Volume mounted as ${r.data.drive}`);
                setQmFeedback(`Volume mounted as ${r.data.drive}.`);
                setQmMountedDrive(r.data.drive);
                setQmPassword('');
            } else {
                // Operational mount result → Notifications tab, not System Alerts.
                const message = vaultOperationError(r?.error || (r?.success
                    ? 'Machine-wide mounting was not confirmed. Update both WinCommander and Pro before retrying.'
                    : 'Mount failed'));
                setQmFeedback(message);
                showError(message, undefined, { kind: "notification" });
            }
        } catch (e) {
            const message = vaultOperationError(e);
            setQmFeedback(message);
            showError(message, undefined, { kind: "notification" });
        } finally {
            setQmPassword('');
            setQmMountingIdx(null);
        }
    }, [quickMountSlots, qmSelectedIdx, qmPassword, mountVolume, refreshVault, verifyVaultDrive, refreshQmLetters]);

    // Safe Paste requests are kept in a native queue until this listener drains
    // them. Tauri events alone can be emitted before this effect subscribes on
    // a cold or hidden launch, silently dropping the scrub workflow.
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        let draining = false;
        let drainRequested = false;

        const processRequest = async (paths: string[]) => {
            const dest = paths[0];
            if (!dest) return;
            try {
                const res = await safePastePrepare(dest);
                if (res.skipped.length > 0) {
                    const names = res.skipped.slice(0, 3).map((sk) => `${sk.name} (${sk.reason})`).join(", ");
                    const more = res.skipped.length > 3 ? ` +${res.skipped.length - 3} more` : "";
                    // Operational Safe Paste result → Notifications tab.
                    showError(`Safe Paste skipped ${res.skipped.length}: ${names}${more}`, undefined, { kind: "notification" });
                }
                if (res.copied.length === 0) {
                    if (res.sourceCount === 0) showError("Nothing to Safe Paste — use Safe Copy first.", undefined, { kind: "notification" });
                    return;
                }
                const noun = `${res.copied.length} item${res.copied.length !== 1 ? "s" : ""}`;
                showSuccess(`Safe-pasted ${noun} after metadata scrub`);
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                // require_paid surfaces here for Free users — honest upsell.
                showError(`Safe Paste: ${msg}`, undefined, { kind: "notification" });
            }
        };

        const drainRequests = async () => {
            if (draining) {
                drainRequested = true;
                return;
            }
            draining = true;
            try {
                do {
                    drainRequested = false;
                    const requests = await invoke<string[][]>('take_safe_paste_requests');
                    for (const paths of requests) await processRequest(paths);
                } while (drainRequested);
            } finally {
                draining = false;
            }
        };

        listen('safe-paste-requested', () => {
            void drainRequests();
        }).then((u) => {
            unlisten = u;
            // A context-menu launch can queue before React finishes mounting.
            // Drain once after subscription so that wake-up event isn't needed
            // for cold-start reliability.
            void drainRequests();
        });
        return () => {
            unlisten?.();
        };
    }, [safePastePrepare]);

    // Resolve which steps are enabled per the user's config in
    // privacy.selfDestruct. Sparse override map; missing keys fall
    // back to the step's defaultEnabled. Same resolution as the Rust
    // orchestrator — the two sides MUST agree on which rows render.
    const sdConfig = appSettings?.ideal?.privacy?.selfDestruct;
    const sdShutdownSystem = sdConfig?.shutdownSystem ?? false;

    // Fire the universal Rust orchestrator and drive the operation
    // overlay from `lockdown-step` events. The orchestrator reads its
    // own configuration from settings — no args. Per-step Promises
    // resolve when the matching `done` event arrives; failures from
    // Rust (paid command unauthorised, Pro not installed, etc.) come
    // through with `ok: false` so the overlay shows them as errored
    // rather than silently swallowing them.
    const fireSelfDestruct = useCallback(async () => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
        setLoadingAction('selfDestruct');

        let capabilityToken: string;
        try {
            capabilityToken = await requestDestructiveCapability({ command: "full_lockdown" });
        } catch (err) {
            showError(`Lockdown cancelled: ${String(err)}`);
            setLoadingAction(null);
            return;
        }

        // When the user has opted to hide the destruction sequence overlay,
        // skip the deferred/listener setup and fire the cascade silently.
        if (appSettings?.app?.hideDestructionSequence === true) {
            try {
                await invoke('full_lockdown', { capabilityToken });
            } catch (err) {
                showError(`Lockdown failed: ${String(err)}`);
            } finally {
                setLoadingAction(null);
            }
            return;
        }

        // Build the row list from the current settings snapshot. This
        // is the same resolution the Rust side uses, so the row count
        // matches the events we'll receive. include_app is rendered
        // as a row only if enabled — the Rust side emits the
        // "Uninstall WinCommander" event before exiting the process.
        const userSteps = sdConfig?.steps;
        const enabledDefs = DESTRUCT_STEPS.filter((d) => isStepEnabled(d, userSteps));
        const includeAppEnabled = enabledDefs.some((d) => d.id === 'include_app');

        type Deferred = {
            promise: Promise<void>;
            resolve: () => void;
            reject: (err: Error) => void;
        };
        const deferreds = new Map<string, Deferred>();
        for (const def of enabledDefs) {
            let resolve: () => void = () => { };
            let reject: (err: Error) => void = () => { };
            const promise = new Promise<void>((res, rej) => {
                resolve = res;
                reject = rej;
            });
            deferreds.set(def.label, { promise, resolve, reject });
        }
        let removeSchedulesDeferred: Deferred | null = null;
        // Reserve a row for removing auto-erase schedules when enabled.
        if (enabledDefs.some(d => d.id === 'remove_schedules')) {
            let resolve: () => void = () => { };
            let reject: (err: Error) => void = () => { };
            const promise = new Promise<void>((res, rej) => { resolve = res; reject = rej; });
            removeSchedulesDeferred = { promise, resolve, reject };
            deferreds.set('Auto-clean Schedules', removeSchedulesDeferred);
        }
        // Standalone "System Shutdown" row when app removal is OFF
        // but shutdown is ON — Rust emits this label in that path.
        if (!includeAppEnabled && sdShutdownSystem) {
            let resolve: () => void = () => { };
            let reject: (err: Error) => void = () => { };
            const promise = new Promise<void>((res, rej) => {
                resolve = res;
                reject = rej;
            });
            deferreds.set('System Shutdown', { promise, resolve, reject });
        }

        const unlistenPromise = listen<{
            label: string;
            status: string;
            ok: boolean;
            error: string | null;
        }>('lockdown-step', (event) => {
            const { label, status, ok, error } = event.payload ?? ({} as any);
            if (status !== 'done') return;
            const def = deferreds.get(label);
            if (!def) return;
            if (ok) {
                def.resolve();
            } else {
                def.reject(new Error(error || 'failed'));
            }
        });

        // Translate internal Rust event-key labels to branded display labels.
        // deferreds stays keyed on the raw Rust labels so event matching works.
        const toDisplayLabel = (key: string) =>
            key === 'Uninstall WinCommander' ? `Uninstall ${productName}` : key;

        const tasks: { label: string; fn: () => Promise<{ success: boolean; data: any }> }[] =
            Array.from(deferreds.entries()).map(([key, def]) => ({
                label: toDisplayLabel(key),
                fn: async () => {
                    await def.promise; // throws on failure → step shows as error
                    return { success: true, data: undefined };
                },
            }));

        // The include_app step exits the app via a detached PowerShell
        // before the frontend gets to mark it complete. The Rust side
        // pre-emits a `done` event for the row so the overlay's
        // promise resolves before the process exits — but as a
        // belt-and-braces guard, give the overlay a 6s ceiling on
        // that final row too.
        if (includeAppEnabled) {
            const def = deferreds.get('Uninstall WinCommander');
            if (def) {
                Promise.race([
                    def.promise,
                    new Promise<void>((res) => setTimeout(res, 6000)),
                ]).then(() => def.resolve()).catch(() => { /* already settled */ });
            }
        }

        // Kick off removal of auto-erase schedules (frontend-driven row).
        if (removeSchedulesDeferred) {
            const def = removeSchedulesDeferred;
            (async () => {
                try {
                    const res = await getAutoEraseSchedules();
                    if (!res || !res.success || !res.data) {
                        // Nothing to remove or failed to list — resolve to avoid blocking
                        def.resolve();
                        return;
                    }
                    const schedules = res.data.schedules || [];
                    if (schedules.length === 0) {
                        def.resolve();
                        return;
                    }
                    const failures: string[] = [];
                    await Promise.all(schedules.map(async (s: any) => {
                        try {
                            const r = await removeAutoEraseSchedule(s.categoryId);
                            if (!r || !r.success) {
                                failures.push(`${s.categoryId}: ${r?.error || 'remove failed'}`);
                            }
                        } catch (err) {
                            failures.push(`${s.categoryId}: ${String(err)}`);
                        }
                    }));
                    invalidateDiskCleanupScheduleStatus();
                    if (failures.length > 0) {
                        def.reject(new Error(failures.join('; ')));
                    } else {
                        def.resolve();
                    }
                } catch (err) {
                    def.reject(new Error(String(err)));
                }
            })();
        }

        // If the user configured "disable auto ramdisk after lockdown", patch
        // settings before firing so the next launch won't recreate it.
        const ramdiskCfg = appSettings?.app?.vault?.ramdiskAutostart;
        if (ramdiskCfg?.skipAfterLockdown && ramdiskCfg?.enabled) {
            void patchAppSettings({ app: { vault: { ramdiskAutostart: { ...ramdiskCfg, enabled: false } } } } as any).catch(reportSettingsWriteFailure);
        }

        // Kick off the universal destruct. Don't await — the include_app
        // path exits the app and would block the overlay's completion
        // handler.
        invoke('full_lockdown', { capabilityToken }).catch((err) => {
            // Surface the actual reason so the user knows WHY nothing
            // happened. Most common rejection causes:
            //   - Pro entitlement missing (require_paid gate)
            //   - Cleanup module disabled
            //   - Pro binary not installed
            // Without this toast, the row-rejection path below makes
            // every step error in red but the user has no clue why.
            const reason = String(err);
            showError(`Lockdown failed: ${reason}`);
            const e = new Error(reason);
            deferreds.forEach((d) => d.reject(e));
        });

        runOperation(
            'PURGING SYSTEM',
            tasks,
            // mode:'parallel' — sidecar.rs now keeps a pool of Pro
            // sessions (POOL_CAPACITY=4), so up to 4 paid commands
            // execute genuinely concurrently. Phase 1 of the cascade
            // fans out via futures::join_all and the IPC layer no
            // longer serialises them onto a single pipe. The overlay
            // lighting all rows 'running' at once now reflects real
            // execution rather than the previous facade.
            //
            // System Cleaner still bypasses Pro IPC entirely
            // (run_bleachbit_clean is a Rust-native helper) so it
            // also runs in parallel with the paid pool — same as
            // before, just no longer the lone exception.
            { doneTitle: 'PURGE COMPLETE', mode: 'parallel', failFast: false, accent: 'red' }
        ).finally(async () => {
            try {
                const fn = await unlistenPromise;
                fn();
            } catch { /* listener never registered */ }
            setLoadingAction(null);
        });
    }, [
        getAutoEraseSchedules,
        removeAutoEraseSchedule,
        sdConfig,
        sdShutdownSystem,
        appSettings?.app?.hideDestructionSequence,
        appSettings?.app?.vault?.ramdiskAutostart,
        patchAppSettings,
        productName,
        needsElevation,
    ]);

    const lockdownTimerSeconds = Math.min(
        30,
        Math.max(3, appSettings?.app?.lockdownTimerSec ?? 4),
    );

    // Lockdown triggers are no longer surfaced as a chrome button/countdown.
    // Trigger paths fire the configured cascade directly and show progress in
    // the operation overlay; editing the routine lives under Cleanup.
    useEffect(() => {
        const unlisten = listen<{ countdownSeconds: number; cancelled: boolean }>('lockdown-trigger', (event) => {
            if (loadingAction === 'selfDestruct') return;
            if (event.payload?.cancelled) {
                if (sdIntervalRef.current) clearInterval(sdIntervalRef.current);
                sdIntervalRef.current = null;
                sdRustOwnedRef.current = false;
                setSdCountdown(null);
                setSdPopup(false);
                return;
            }
            // Hotkey toggle: pressing the hotkey again WHILE counting aborts.
            // Hotkey-armed countdowns are silent — no abort tone either.
            if (sdCountdownRef.current !== null) {
                if (sdIntervalRef.current) clearInterval(sdIntervalRef.current);
                sdIntervalRef.current = null;
                sdRustOwnedRef.current = false;
                setSdCountdown(null);
                setSdPopup(false);
                return;
            }
            sdSilentRef.current = true; // hotkey = no audio
            sdRustOwnedRef.current = true;
            setSdPopup(false);
            setSdCountdown(event.payload?.countdownSeconds ?? lockdownTimerSeconds);
        });
        return () => { unlisten.then(fn => fn()); };
    }, [loadingAction, lockdownTimerSeconds]);

    // Keep the ref in sync so the listeners above read the live countdown.
    useEffect(() => { sdCountdownRef.current = sdCountdown; }, [sdCountdown]);

    // Manual self-destruct: arm the countdown, or abort if already counting.
    const handleSelfDestructClick = () => {
        if (needsElevation) { showError(MACHINE_SCOPE_ELEVATION_MESSAGE); return; }
        if (loadingAction === 'selfDestruct') return;
        if (sdCountdown !== null) {
            if (sdIntervalRef.current) clearInterval(sdIntervalRef.current);
            sdIntervalRef.current = null;
            sdRustOwnedRef.current = false;
            setSdCountdown(null);
            setSdPopup(false);
            lockdownAbort();
            return;
        }
        if (!canUse("paid")) {
            window.dispatchEvent(new CustomEvent("license-gate-open", {
                detail: { tab: "buy", featureLabel: "Lockdown (full system purge)" },
            }));
            return;
        }
        // Clicking the sidebar control shows the countdown popup with audio.
        sdSilentRef.current = false;
        sdRustOwnedRef.current = false;
        setSdPopup(true);
        setSdCountdown(lockdownTimerSeconds);
    };

    // Drive the countdown: tick every second, fire the cascade at 0.
    // Audio is suppressed when sdSilentRef is true (hotkey / coercion triggers).
    useEffect(() => {
        if (sdCountdown === null) return;
        if (sdCountdown === 0) {
            if (!sdSilentRef.current) lockdownFire();
            const rustOwned = sdRustOwnedRef.current;
            sdRustOwnedRef.current = false;
            if (!rustOwned) void fireSelfDestruct();
            setSdCountdown(null);
            setSdPopup(false);
            return;
        }
        if (!sdSilentRef.current) lockdownCountdownBeep(sdCountdown);
        sdIntervalRef.current = setInterval(() => {
            setSdCountdown(prev => (prev !== null ? prev - 1 : null));
        }, 1000);
        return () => { if (sdIntervalRef.current) clearInterval(sdIntervalRef.current); };
    }, [sdCountdown, fireSelfDestruct]);

    const handleAction = useCallback(async (action: string, handler: () => Promise<any>) => {
        setLoadingAction(action);
        if (action === "dismount") setDismountFailure('');
        try {
            await handler();
            if (action === "dismount") {
                await refreshVault(true);
            }
            showSuccess(ACTION_LABELS[action] || "Action completed");
        } catch (err) {
            if (action === "dismount") setDismountFailure(vaultOperationError(err, 'dismount'));
            const msg = err instanceof Error ? err.message : String(err);
            showError(`Failed: ${msg}`);
        } finally {
            setLoadingAction(null);
        }
    }, [refreshVault]);

    const handleDismountClick = useCallback(() => {
        if (!canUse("paid")) {
            window.dispatchEvent(
                new CustomEvent("license-gate-open", {
                    detail: { tab: "buy", featureLabel: "Dismount Encrypted Volumes" },
                })
            );
            return;
        }
        // Sidebar panic action — dismount everything in one tap. The in-panel
        // VolumeActionsMenu / RamDisksSection still expose per-row eject for
        // selective dismounts.
        void handleAction("dismount", async () => {
            // Always ask both engines. Their availability probes are deliberately
            // staggered during startup, so using them as a precondition makes a
            // real mounted volume impossible to dismount until those probes finish.
            const [encrypted, ram] = await Promise.all([
                dismountAllVolumes(true),
                removeAllRamDisks(),
            ]);
            if (!encrypted?.success) {
                throw new Error(encrypted?.error || "Failed to force-dismount encrypted volumes.");
            }
            const ramData = ram?.data as { status?: string; error?: string } | undefined;
            const ramError = String(ram?.error ?? ramData?.error ?? "");
            if (!ram?.success && !/not installed|no ram disks|none found/i.test(ramError)) {
                throw new Error(ramError || "Failed to dismount RAM disks.");
            }
            if (ramData?.status === "error" && !/not installed|no ram disks|none found/i.test(String(ramData.error ?? ""))) {
                throw new Error(ramData.error || "Failed to dismount RAM disks.");
            }
        });
    }, [canUse, dismountAllVolumes, handleAction, removeAllRamDisks]);

    // The tray action intentionally routes through this same handler instead
    // of maintaining a second native dismount implementation. This keeps the
    // entitlement gate, real backend calls, toast/error state, and vault refresh
    // identical whether the user clicks the sidebar or the tray icon.
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        let disposed = false;
        void listen("tray-dismount-all-requested", () => {
            if (loadingAction !== "dismount") handleDismountClick();
        }).then((dispose) => {
            if (disposed) dispose();
            else unlisten = dispose;
        });
        return () => {
            disposed = true;
            unlisten?.();
        };
    }, [loadingAction, handleDismountClick]);


    const handleOpenShredder = () => {
        // Windows' native picker can't mix files and folders in one
        // selection, so we skip the OS picker and open the in-app shred
        // dialog directly. The dialog's own Add files / Add folder
        // buttons let the user build a mixed target list.
        window.dispatchEvent(new CustomEvent("open-shred-dialog"));
    };

    return (
        <>
            <div className="right-sidebar">

                <div className="quick-actions-list">

                    {/* PANIC DISMOUNT — single combined action: dismounts both
                        encrypted volumes AND RAM disks in one tap (owner
                        request: keep the sidebar as a single emergency control,
                        per-kind eject lives inside the Secure Storage panel
                        itself). Disables when neither engine is installed.
                        Borrow-mode visibility follows the Secret Settings table:
                        hidden by default (DEFAULT_BORROWED_EXTRAS lists it), but a
                        user who sets it to "No" can keep it reachable. The
                        hiddenActions set already encodes that borrow logic — a
                        blanket !borrowedActive gate here would override the
                        per-action setting and is the bug this replaced. */}
                    {!hiddenActions.has("dismount") && (
                    <div
                        className={`action-item ${!dismountAvailable ? 'action-item--disabled' : ''}`}
                        data-tip="Emergency Dismount Volumes + RAM Disks"
                        data-tip-intent="danger"
                    >
                        <ActionBtn
                            className="action-btn dismount-btn"
                            icon="eject"
                            intent="danger"
                            minimal
                            large
                            loading={loadingAction === "dismount"}
                            disabled={!dismountAvailable}
                            onClick={dismountAvailable ? handleDismountClick : undefined}
                            ariaLabel="Emergency dismount volumes and RAM disks"
                        />
                        <span className="action-label dismount-label">Dismount</span>
                    </div>
                    )}

                    {/* QUICK MOUNT — password-only mount shortcut for saved vaults */}
                    {!borrowedActive && !hiddenActions.has("quickMount") && (
                    <div
                        className="action-item"
                        data-tip={quickMountSlots.length > 0
                            ? `Quick Mount (${quickMountSlots.length} vault${quickMountSlots.length > 1 ? 's' : ''} configured)`
                            : "Quick Mount — add a vault shortcut"}
                    >
                        <ActionBtn
                            className="action-btn"
                            icon="unlock"
                            minimal
                            large
                            loading={qmMountingIdx !== null}
                            onClick={handleQmOpen}
                            ariaLabel="Quick mount encrypted volume"
                        />
                        <span className="action-label">Mount</span>
                    </div>
                    )}

                    {!hiddenActions.has("ai-advisor") && (
                        <div className="action-item" data-tip="AI Security Advisor">
                            <ActionBtn
                                className="action-btn"
                                icon="lightbulb"
                                minimal
                                large
                                onClick={() => window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "advisor" }))}
                                ariaLabel="Open AI Security Advisor"
                            />
                            <span className="action-label">Advisor</span>
                        </div>
                    )}

                    {!hiddenActions.has("search") && (
                        <div className="action-item" data-tip="Instant file search">
                            <ActionBtn
                                className="action-btn"
                                icon="search"
                                minimal
                                large
                                onClick={() => window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "search-files" }))}
                                ariaLabel="Open instant file search"
                            />
                            <span className="action-label">Search</span>
                        </div>
                    )}

                    {/* (Removed) Laptop erase helper — per request */}



                    {/* SECURE SHREDDER — shown unless hidden via Secret Settings.
                        Opens the shred dialog which accepts files AND folders,
                        single or multiple. Borrow-mode visibility follows the
                        Secret Settings table (hidden by default). */}
                    {!hiddenActions.has("delete") && (
                    <div className="action-item" data-tip="Secure file/folder deletion">
                        <ActionBtn
                            className="action-btn"
                            icon="trash"
                            minimal
                            large
                            loading={loadingAction === "shredder"}
                            onClick={handleOpenShredder}
                            ariaLabel="Open secure file and folder deletion"
                        />
                        <span className="action-label">Delete</span>
                    </div>
                    )}

                    {/* SHARE SAFELY — metadata scrubber.
                        Gated by the Privacy Clean surface (NOT investigator).
                        Stripping EXIF / PDF / Office metadata before sharing is
                        a Privacy Clean hygiene task, not evidence collection.
                        Borrow-mode visibility follows the Secret Settings table
                        (hidden by default). */}
                    {visibility.isVisible({ capability: ["privacy"] }) && !hiddenActions.has("scrubMeta") && (
                        <div className="action-item" data-tour="right-sidebar-scrub" data-tip="Strip EXIF / PDF / Office metadata before sharing">
                            <ActionBtn
                                className="action-btn"
                                icon="eraser"
                                minimal
                                large
                                onClick={() => setScrubDialogOpen(true)}
                                ariaLabel="Open metadata scrubber"
                            />
                            <span className="action-label">Scrub Meta</span>
                        </div>
                    )}

                </div>

                {/* LOCKDOWN — pinned to the bottom-right corner, separated from the
                    main action group (owner request). Arms a 4s abort countdown,
                    then fires the configured cascade (same fireSelfDestruct as
                    hotkey/coercion). Click again during the countdown to
                    abort. Execution is paid-gated. Borrow-mode visibility follows
                    the Secret Settings table (hidden by default). Hidden entirely
                    when self-destruct is not opted in (appSettings null = decoy
                    mode → treat as not-enabled). */}
                {(showLockdownControl || showLockdownImpression) && (
                <div className="sidebar-footer">
                    {showLockdownControl ? (
                        <div
                            className="action-item"
                            data-tour="right-sidebar-lockdown"
                            onClick={needsElevation || pendingLockdownEnabled !== null ? undefined : handleSelfDestructClick}
                            data-tip={pendingLockdownEnabled !== null ? "Saving Lockdown setting…" : sdCountdown !== null ? "Click again to abort" : "Lockdown — runs your configured steps (edit in Secret Settings → Lockdown)"}
                            data-tip-intent="danger"
                        >
                            <ActionBtn
                                className="action-btn"
                                icon="warning-sign"
                                disabled={needsElevation || pendingLockdownEnabled !== null}
                                intent="danger"
                                minimal
                                large
                                loading={loadingAction === "selfDestruct"}
                                ariaLabel={pendingLockdownEnabled !== null ? "Lockdown setting is being saved" : sdCountdown !== null ? "Abort lockdown countdown" : "Run configured lockdown"}
                            />
                            <span className="action-label">
                                {sdCountdown !== null
                                    ? `ABORT (${sdCountdown})`
                                    : loadingAction === "selfDestruct"
                                        ? "PURGING"
                                        : "Lockdown"}
                            </span>
                            {needsElevation && <span className="text-[10px] text-[var(--warn)]">Requires an administrator</span>}
                        </div>
                    ) : (
                        <div
                            className="action-item action-item--disabled lockdown-tour-impression"
                            data-tour="right-sidebar-lockdown-impression"
                            aria-disabled="true"
                        >
                            <ActionBtn
                                className="action-btn"
                                icon="warning-sign"
                                intent="danger"
                                disabled
                                ariaLabel="Lockdown preview. Turn on Lockdown with the tour switch."
                            />
                            <span className="action-label">Lockdown</span>
                        </div>
                    )}
                </div>
                )}
            </div>

            {scrubDialogOpen && (
                <Suspense fallback={null}>
                    <MetadataScrubberDialog
                        isOpen
                        onClose={() => setScrubDialogOpen(false)}
                    />
                </Suspense>
            )}

            {/* Quick Mount overlay */}
            <Dialog open={Boolean(dismountFailure)} onOpenChange={open => { if (!open) setDismountFailure(''); }}>
                <DialogContent><DialogHeader><DialogTitle>Dismount needs attention</DialogTitle></DialogHeader><p role="alert">{dismountFailure}</p><button type="button" className="qm-btn qm-btn--primary" onClick={() => setDismountFailure('')}>Close</button></DialogContent>
            </Dialog>
            {qmOpen && (
                <div className="qm-overlay" role="dialog" aria-modal="true"
                    onClick={(e) => { if (e.target === e.currentTarget) { setQmOpen(false); setQmEditing(null); } }}>
                    <div className="qm-dialog">
                        {qmFeedback && <div className="qm-operation-feedback" role="alert">{qmFeedback}</div>}
                        {qmMountedDrive && <button type="button" className="qm-btn qm-btn--primary" onClick={() => void openVaultDrive(qmMountedDrive)}>Open {qmMountedDrive} in File Explorer</button>}
                        {qmEditing ? (
                            /* ── Slot editor ── */
                            <>
                                <div className="qm-title-row">
                                    {quickMountSlots.length > 0 && (
                                        <button type="button" className="qm-back-btn"
                                            onClick={() => setQmEditing(null)}>
                                            <Icon icon="arrow-left" size={13} />
                                        </button>
                                    )}
                                    <span className="qm-title">
                                        {qmEditing.idx === 'new' ? 'Add Quick Mount' : 'Edit Quick Mount'}
                                    </span>
                                </div>
                                <div className="qm-field">
                                    <label className="qm-label">Target type</label>
                                    <div className="qm-actions">
                                        <button type="button" className={`qm-btn ${qmEditing.targetType === 'file' ? 'qm-btn--primary' : 'qm-btn--ghost'}`}
                                            onClick={() => setQmEditing(ed => ed && ({ ...ed, targetType: 'file', path: '' }))}>Container file</button>
                                        <button type="button" className={`qm-btn ${qmEditing.targetType === 'partition' ? 'qm-btn--primary' : 'qm-btn--ghost'}`}
                                            onClick={() => {
                                                setQmEditing(ed => ed && ({ ...ed, targetType: 'partition', path: '' }));
                                                void loadQmPartitions();
                                            }}>Partition / drive</button>
                                    </div>
                                </div>
                                {qmEditing.targetType === 'partition' ? (
                                    <div className="qm-field">
                                        <label className="qm-label">Encrypted partition</label>
                                        {qmPartitionsLoading ? <span className="qm-hint">Finding mountable partitions…</span> : (
                                            <select className="qm-select" value={qmEditing.path}
                                                onChange={(e) => setQmEditing(ed => ed && ({ ...ed, path: e.target.value }))}>
                                                <option value="">Select a partition</option>
                                                {qmPartitions.map((partition) => (
                                                    <option key={partition.devicePath} value={partition.devicePath}>
                                                        {partition.model} · Disk {partition.diskNumber}, Part {partition.partitionNumber} · {partition.size}{partition.driveLetter ? ` · ${partition.driveLetter}:` : ''}
                                                    </option>
                                                ))}
                                            </select>
                                        )}
                                        {!qmPartitionsLoading && qmPartitions.length === 0 && <span className="qm-hint">No mountable encrypted partitions found.</span>}
                                    </div>
                                ) : (
                                <div className="qm-field">
                                    <label className="qm-label">File Path</label>
                                    <div className="qm-path-row">
                                        <input
                                            className="qm-input"
                                            type="text"
                                            placeholder="C:\path\to\vault.hc"
                                            autoFocus
                                            value={qmEditing.path}
                                            onChange={(e) => setQmEditing(ed => ed && ({ ...ed, path: e.target.value }))}
                                            onKeyDown={(e) => { if (e.key === 'Enter') handleQmSaveSlot(); }}
                                        />
                                        <button
                                            type="button"
                                            className="qm-browse-btn"
                                            title="Browse for file"
                                            onClick={async () => {
                                                try {
                                                    const selected = await openFilePicker({
                                                        multiple: false,
                                                        filters: [{ name: 'Container File', extensions: ['hc', 'tc', '*'] }],
                                                    });
                                                    if (selected && typeof selected === 'string')
                                                        setQmEditing(ed => ed && ({ ...ed, path: selected }));
                                                } catch {}
                                            }}
                                        >
                                            <Icon icon="folder-open" size={14} />
                                        </button>
                                    </div>
                                </div>
                                )}
                                <div className="qm-field">
                                    <label className="qm-label">Drive Letter</label>
                                    <select
                                        className="qm-select"
                                        aria-label="Quick Mount free drive letter"
                                        disabled={qmLettersLoading}
                                        value={qmEditing.letter}
                                        onChange={(e) => setQmEditing(ed => ed && ({ ...ed, letter: e.target.value }))}
                                    >
                                        <option value="" disabled>{qmLettersLoading ? 'Checking free letters…' : 'Select a free drive letter'}</option>
                                        {qmEditing.letter && !qmLetterChoices.includes(qmEditing.letter) && <option value={qmEditing.letter} disabled>{qmEditing.letter}: — unavailable</option>}
                                        {qmLetterChoices.map(letter => <option key={letter} value={letter}>{letter}:</option>)}
                                    </select>
                                    <button type="button" className="qm-btn qm-btn--ghost" disabled={qmLettersLoading} onClick={() => void refreshQmLetters()}>Refresh free letters</button>
                                    {quickMountSlots.filter((_, i) => qmEditing.idx === 'new' || i !== qmEditing.idx).length > 0 && (
                                        <span className="qm-hint">
                                            Already used:{' '}
                                            {quickMountSlots
                                                .filter((_, i) => qmEditing.idx === 'new' || i !== (qmEditing.idx as number))
                                                .map(s => `${s.driveLetter}:`)
                                                .join(', ')}
                                        </span>
                                    )}
                                </div>
                                <div className="qm-actions">
                                    <button type="button" className="qm-btn qm-btn--ghost"
                                        onClick={() => quickMountSlots.length > 0 ? setQmEditing(null) : setQmOpen(false)}>
                                        Cancel
                                    </button>
                                    <button type="button" className="qm-btn qm-btn--primary"
                                        disabled={!qmEditing.path.trim() || !qmLetterChoices.includes(qmEditing.letter) || qmSaving || qmLettersLoading}
                                        onClick={handleQmSaveSlot}>
                                        {qmSaving ? <Spinner size={14} /> : 'Save'}
                                    </button>
                                </div>
                            </>
                        ) : (
                            /* ── Mount view — dropdown + path info + password ── */
                            <>
                                <div className="qm-fleet-vaults">
                                    <div className="qm-title-row">
                                        <span className="qm-title">Saved Fleet Vaults</span>
                                    </div>
                                    {fleetVaultsLoading ? (
                                        <div className="qm-empty">Loading your saved Fleet Vaults…</div>
                                    ) : fleetVaults.length === 0 ? (
                                        <div className="qm-empty">No Fleet Vault is assigned to this Windows account.</div>
                                    ) : (
                                        <>
                                            <div className="qm-field">
                                                <label className="qm-label" htmlFor="fleet-vault-mount-select">Vault</label>
                                                <select
                                                    id="fleet-vault-mount-select"
                                                    className="qm-select"
                                                    value={fleetVaultEntryId}
                                                    onChange={(event) => { setFleetVaultEntryId(event.target.value); setFleetVaultPassword(''); setQmFeedback(''); setQmMountedDrive(null); }}
                                                >
                                                    {fleetVaults.map(entry => <option key={entry.entry_id} value={entry.entry_id}>
                                                        {(entry.drive_letter ?? entry.preferred_letter) ? `${entry.drive_letter ?? entry.preferred_letter}: — ` : ''}{entry.label} — {entry.access === 'write' ? 'Edit / read-write' : 'View / read only'}
                                                    </option>)}
                                                </select>
                                                <span className="qm-hint">This list is supplied by the Vault service for this Windows account. Container paths are never exposed here.</span>
                                            </div>
                                            {fleetVaults.find(entry => entry.entry_id === fleetVaultEntryId)?.mount_state === 'mounted' && <div role="status" className="qm-hint">
                                                This Vault is already mounted.
                                                {fleetVaults.find(entry => entry.entry_id === fleetVaultEntryId)?.drive_letter && <button type="button" className="qm-btn qm-btn--primary" onClick={() => {
                                                    const drive = fleetVaults.find(entry => entry.entry_id === fleetVaultEntryId)?.drive_letter;
                                                    if (drive) void openVaultDrive(drive);
                                                }}>Open in File Explorer</button>}
                                            </div>}
                                            <div className="qm-field">
                                                <label className="qm-label" htmlFor="fleet-vault-mount-password">Password</label>
                                                <input
                                                    id="fleet-vault-mount-password"
                                                    className="qm-input"
                                                    type="password"
                                                    placeholder="Enter password"
                                                    value={fleetVaultPassword}
                                                    onChange={(event) => setFleetVaultPassword(event.target.value)}
                                                    onKeyDown={(event) => { if (event.key === 'Enter') void handleFleetVaultMount(); }}
                                                />
                                            </div>
                                            <div className="qm-actions">
                                                <button type="button" className="qm-btn qm-btn--primary"
                                                    disabled={!fleetVaultEntryId || !fleetVaultPassword || fleetVaultMounting}
                                                    onClick={() => void handleFleetVaultMount()}>
                                                    {fleetVaultMounting ? <Spinner size={14} /> : 'Mount inside'}
                                                </button>
                                            </div>
                                        </>
                                    )}
                                </div>
                                <div className="qm-title-row">
                                    <span className="qm-title">Quick Mount</span>
                                    <button type="button" className="qm-add-btn"
                                        onClick={() => setQmEditing({ idx: 'new', path: '', letter: nextFreeLetter(), targetType: 'file' })}>
                                        <Icon icon="plus" size={13} /> Add
                                    </button>
                                </div>

                                {quickMountSlots.length === 0 ? (
                                    <div className="qm-empty">
                                        No vaults configured. Click <strong>Add</strong> to set one up.
                                    </div>
                                ) : (
                                    <>
                                        <div className="qm-field">
                                            <label className="qm-label">Vault</label>
                                            <div className="qm-select-row">
                                                <select
                                                    className="qm-select"
                                                    value={qmSelectedIdx}
                                                    onChange={(e) => { setQmSelectedIdx(Number(e.target.value)); setQmPassword(''); setQmFeedback(''); setQmMountedDrive(null); }}
                                                >
                                                    {quickMountSlots.map((slot, idx) => {
                                                        const name = slot.targetType === 'partition'
                                                            ? `Partition ${slot.filePath}`
                                                            : slot.filePath.split(/[\\/]/).filter(Boolean).pop() || slot.filePath;
                                                        return (
                                                            <option key={idx} value={idx}>
                                                                {slot.driveLetter}: — {name}
                                                            </option>
                                                        );
                                                    })}
                                                </select>
                                                <button type="button" className="qm-slot-edit-btn" title="Edit"
                                                    onClick={() => {
                                                        const s = quickMountSlots[qmSelectedIdx];
                                                        if (s) {
                                                            const targetType = s.targetType ?? 'file';
                                                            setQmEditing({ idx: qmSelectedIdx, path: s.filePath, letter: s.driveLetter, targetType });
                                                            if (targetType === 'partition') void loadQmPartitions();
                                                        }
                                                    }}>
                                                    <Icon icon="edit" size={12} />
                                                </button>
                                                <button type="button" className="qm-slot-remove-btn" title="Remove"
                                                    onClick={() => handleQmRemoveSlot(qmSelectedIdx)}>
                                                    <Icon icon="cross" size={12} />
                                                </button>
                                            </div>
                                            {quickMountSlots[qmSelectedIdx] && (
                                                <span className="qm-path-display" title={quickMountSlots[qmSelectedIdx].filePath}>
                                                    {quickMountSlots[qmSelectedIdx].filePath}
                                                </span>
                                            )}
                                            <span className="qm-hint">Mounts machine-wide, read/write. Existing file permissions still apply.</span>
                                        </div>

                                        <div className="qm-field">
                                            <label className="qm-label">Password</label>
                                            <input
                                                className="qm-input"
                                                type="password"
                                                placeholder="Enter password"
                                                autoFocus
                                                value={qmPassword}
                                                onChange={(e) => setQmPassword(e.target.value)}
                                                onKeyDown={(e) => { if (e.key === 'Enter') handleQmMount(); }}
                                            />
                                        </div>

                                        <div className="qm-actions">
                                            <button type="button" className="qm-btn qm-btn--ghost"
                                                onClick={() => setQmOpen(false)}>Cancel</button>
                                            <button type="button" className="qm-btn qm-btn--primary"
                                                disabled={!qmPassword || qmMountingIdx !== null}
                                                onClick={handleQmMount}>
                                                {qmMountingIdx !== null ? <Spinner size={14} /> : 'Mount'}
                                            </button>
                                        </div>
                                    </>
                                )}
                            </>
                        )}
                    </div>
                </div>
            )}

            {/* Lockdown countdown popup — shown only when armed from the sidebar
                CLICK (the hotkey path stays popup-less). Click ABORT, the rail
                control, or press the hotkey again to cancel before it fires. */}
            {sdPopup && sdCountdown !== null && (
                <div className="lockdown-popup-overlay" role="alertdialog" aria-live="assertive">
                    <div className="lockdown-popup">
                        <div className="lockdown-popup-label">LOCKDOWN IN</div>
                        <div className="lockdown-popup-count">{sdCountdown}</div>
                        <div className="lockdown-popup-sub">
                            Running your configured erase. This cannot be undone.
                        </div>
                        <button type="button" className="lockdown-popup-abort" onClick={handleSelfDestructClick}>
                            ABORT
                        </button>
                    </div>
                </div>
            )}
        </>
    );
}
