import { Icon } from "./ui/icon";
import { Spinner } from "./ui/spinner";
import { cn } from "../lib/utils";
import { useAppState } from "../context/AppContext";
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
import { runOperation } from "../context/OperationContext";
import { DESTRUCT_STEPS, isStepEnabled } from "../types/lockdownSteps";
import { DEFAULT_BORROWED_EXTRAS } from "../lib/visibilityDefaults";
import './RightSidebar.css';

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
    } = useAppState();
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
        getEncryptionPartitions,
        safePastePrepare,
        scrubMetadataPaths,
    } = useBackend();

    const visibility = useVisibility();
    const { canUse } = useEntitlements();
    const borrowedActive = useBorrowedActive();
    const borrowedHidden = appSettings?.app?.borrowedHidden ?? DEFAULT_BORROWED_EXTRAS;
    // Quick actions the user has hidden via Secret Settings ▸ Sidebar actions.
    // An action is also hidden when Borrowed Mode is active and its key is in
    // borrowedHidden (key format: "action:<key>").
    const hiddenActions = new Set([
        ...(appSettings?.app?.hiddenSidebarActions ?? []),
        ...(borrowedActive
            ? borrowedHidden
                .filter(k => k.startsWith("action:"))
                .map(k => k.slice("action:".length))
            : []),
    ]);
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
    // true when the active countdown was armed by hotkey or coercion — no audio.
    const sdSilentRef = useRef<boolean>(false);
    const [scrubDialogOpen, setScrubDialogOpen] = useState(false);
    const [scrubInitialPaths, setScrubInitialPaths] = useState<string[] | undefined>(undefined);

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
    type QmSlot = QuickMountSlot;
    // useMemo so the array identity is stable across renders — otherwise the
    // `?? []` fallback minted a fresh [] every render, churning the deps of
    // every useCallback below that closes over it.
    const quickMountSlots: QmSlot[] = useMemo(
        () => appSettings?.app?.vault?.quickMountSlots ?? [],
        [appSettings?.app?.vault?.quickMountSlots],
    );

    const nextFreeLetter = useCallback((excludeIdx?: number) => {
        const taken = new Set(
            quickMountSlots
                .filter((_, i) => i !== excludeIdx)
                .map(s => s.driveLetter.toUpperCase())
        );
        return 'VWXYZEFGHIJKLMNOPQRSTU'.split('').find(l => !taken.has(l)) ?? 'V';
    }, [quickMountSlots]);

    const handleQmOpen = useCallback(() => {
        setQmPassword('');
        setQmSelectedIdx(0);
        setQmEditing(quickMountSlots.length === 0 ? { idx: 'new', path: '', letter: nextFreeLetter(), targetType: 'file' } : null);
        setQmOpen(true);
    }, [quickMountSlots.length, nextFreeLetter]);

    const patchQmSlots = useCallback(async (slots: QmSlot[]) => {
        setQmSaving(true);
        try {
            await patchAppSettings({ app: { vault: { quickMountSlots: slots } } });
        } catch { showError('Failed to save.'); }
        finally { setQmSaving(false); }
    }, [patchAppSettings]);

    const handleQmSaveSlot = useCallback(async () => {
        if (!qmEditing) return;
        const path = qmEditing.path.trim();
        const letter = qmEditing.letter.trim().toUpperCase().slice(0, 1);
        if (!path || !letter) return;
        // Block duplicate drive letters across slots
        const clash = quickMountSlots.some((s, i) =>
            s.driveLetter.toUpperCase() === letter &&
            (qmEditing.idx === 'new' || i !== (qmEditing.idx as number))
        );
        if (clash) {
            showError(`Drive ${letter}: is already used by another vault. Pick a different letter.`);
            return;
        }
        const next = [...quickMountSlots];
        const saved: QmSlot = { filePath: path, driveLetter: letter, targetType: qmEditing.targetType };
        if (qmEditing.idx === 'new') next.push(saved);
        else next[qmEditing.idx as number] = saved;
        await patchQmSlots(next);
        setQmEditing(null);
    }, [qmEditing, quickMountSlots, patchQmSlots]);

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
        await patchQmSlots(next);
        setQmSelectedIdx(0);
    }, [quickMountSlots, patchQmSlots]);

    const handleQmMount = useCallback(async () => {
        const slot = quickMountSlots[qmSelectedIdx];
        if (!slot || !qmPassword) return;
        setQmMountingIdx(qmSelectedIdx);
        try {
            const r = await mountVolume({
              volumePath: slot.filePath,
              driveLetter: slot.driveLetter,
              password: qmPassword,
            });
            if (r?.success) {
                showSuccess(`Volume mounted as ${slot.driveLetter}:`);
                refreshVault(true);
                setQmOpen(false);
                setQmPassword('');
            } else {
                // Operational mount result → Notifications tab, not System Alerts.
                showError(r?.error || 'Mount failed — wrong password?', undefined, { kind: "notification" });
            }
        } catch (e) {
            showError(`Mount failed: ${e}`, undefined, { kind: "notification" });
        } finally {
            setQmMountingIdx(null);
        }
    }, [quickMountSlots, qmSelectedIdx, qmPassword, mountVolume, refreshVault]);

    // Listen for `scrub-requested` events from the single-instance handler.
    // Fires when Explorer's right-click "Scrub metadata with WinCommander"
    // launches the exe — paths come in via the --scrub flag and bubble up
    // to here so we can open the dialog pre-seeded.
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        listen<string[]>('scrub-requested', (e) => {
            const paths = e.payload ?? [];
            if (paths.length === 0) return;
            setScrubInitialPaths(paths);
            setScrubDialogOpen(true);
        }).then((u) => {
            unlisten = u;
        });
        return () => {
            unlisten?.();
        };
    }, []);

    // Listen for `safe-paste-requested` — Explorer's right-click "Safe Paste"
    // on a destination folder. Copy the recorded sources into that folder
    // (exact names, collisions skipped — never renamed), then scrub the fresh
    // copies in place automatically — Safe Paste is a one-shot verb, so it
    // does NOT open the scrub dialog (that's the explicit "Scrub metadata"
    // verb's job). Replace-in-place, non-paranoid defaults match a normal
    // MetadataScrubberDialog scrub.
    useEffect(() => {
        let unlisten: (() => void) | undefined;
        listen<string[]>('safe-paste-requested', async (e) => {
            const dest = (e.payload ?? [])[0];
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
                const report = await scrubMetadataPaths(res.copied, {
                    dryRun: false,
                    recursive: true,
                    replaceOriginals: true,
                    paranoid: { randomizeTimestamps: false, stripAltStreams: false },
                });
                const cleaned = report.scrubbed.length;
                const noun = `${cleaned} file${cleaned !== 1 ? "s" : ""}`;
                // Survivors: files where identifying metadata couldn't be fully
                // stripped — surfaced loudly so a leaky copy isn't read as clean.
                const residual = report.scrubbed.filter((r) => (r.residualFields?.length ?? 0) > 0).length;
                if (report.errors.length > 0) {
                    const first = report.errors[0];
                    const more = report.errors.length > 1 ? ` +${report.errors.length - 1} more` : "";
                    showError(`Safe-pasted, but scrub failed on ${report.errors.length} of ${res.copied.length}: ${first.message}${more}`, undefined, { kind: "notification" });
                } else if (residual > 0) {
                    showError(`Safe-pasted & scrubbed ${noun} — ${residual} still contain${residual === 1 ? "s" : ""} metadata that couldn't be removed`, undefined, { kind: "notification" });
                } else {
                    showSuccess(`Safe-pasted & scrubbed ${noun}`);
                }
            } catch (err) {
                const msg = err instanceof Error ? err.message : String(err);
                // require_paid surfaces here for Free users — honest upsell.
                showError(`Safe Paste: ${msg}`, undefined, { kind: "notification" });
            }
        }).then((u) => {
            unlisten = u;
        });
        return () => {
            unlisten?.();
        };
    }, [safePastePrepare, scrubMetadataPaths]);

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
        setLoadingAction('selfDestruct');

        // When the user has opted to hide the destruction sequence overlay,
        // skip the deferred/listener setup and fire the cascade silently.
        if (appSettings?.app?.hideDestructionSequence === true) {
            try {
                await invoke('full_lockdown');
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
            void patchAppSettings({ app: { vault: { ramdiskAutostart: { ...ramdiskCfg, enabled: false } } } } as any);
        }

        // Kick off the universal destruct. Don't await — the include_app
        // path exits the app and would block the overlay's completion
        // handler.
        invoke('full_lockdown').catch((err) => {
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
    ]);

    const lockdownTimerSeconds = Math.min(
        30,
        Math.max(3, appSettings?.app?.lockdownTimerSec ?? 4),
    );

    // Lockdown triggers are no longer surfaced as a chrome button/countdown.
    // Trigger paths fire the configured cascade directly and show progress in
    // the operation overlay; editing the routine lives under Cleanup.
    useEffect(() => {
        const unlisten = listen<void>('lockdown-trigger', () => {
            if (loadingAction === 'selfDestruct') return;
            // Hotkey toggle: pressing the hotkey again WHILE counting aborts.
            // Hotkey-armed countdowns are silent — no abort tone either.
            if (sdCountdownRef.current !== null) {
                if (sdIntervalRef.current) clearInterval(sdIntervalRef.current);
                sdIntervalRef.current = null;
                setSdCountdown(null);
                setSdPopup(false);
                return;
            }
            sdSilentRef.current = true; // hotkey = no audio
            setSdPopup(false);
            setSdCountdown(lockdownTimerSeconds);
        });
        return () => { unlisten.then(fn => fn()); };
    }, [loadingAction, lockdownTimerSeconds]);

    // Keep the ref in sync so the listeners above read the live countdown.
    useEffect(() => { sdCountdownRef.current = sdCountdown; }, [sdCountdown]);

    // Manual self-destruct: arm the countdown, or abort if already counting.
    const handleSelfDestructClick = () => {
        if (loadingAction === 'selfDestruct') return;
        if (sdCountdown !== null) {
            if (sdIntervalRef.current) clearInterval(sdIntervalRef.current);
            sdIntervalRef.current = null;
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
        setSdPopup(true);
        setSdCountdown(lockdownTimerSeconds);
    };

    // Drive the countdown: tick every second, fire the cascade at 0.
    // Audio is suppressed when sdSilentRef is true (hotkey / coercion triggers).
    useEffect(() => {
        if (sdCountdown === null) return;
        if (sdCountdown === 0) {
            if (!sdSilentRef.current) lockdownFire();
            void fireSelfDestruct();
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

    // Listen for instant-cascade window event (from panic phrase
    // triggers in BackgroundPollers). These bypass the 4s abort
    // countdown — a user under duress can't reach the abort button, and
    // the trigger itself is intentional (typed phrase). Reuses
    // fireSelfDestruct so the user gets the same
    // operation overlay the configured cascade produces; without it, the
    // cascade was invisible and indistinguishable from "trigger broken".
    useEffect(() => {
        const handler = () => {
            if (loadingAction === 'selfDestruct') return; // already running
            void fireSelfDestruct();
        };
        window.addEventListener('panic-cascade-instant', handler);
        return () => window.removeEventListener('panic-cascade-instant', handler);
    }, [fireSelfDestruct, loadingAction]);

    const handleAction = useCallback(async (action: string, handler: () => Promise<any>) => {
        setLoadingAction(action);
        try {
            await handler();
            if (action === "dismount") {
                await refreshVault(true);
            }
            showSuccess(ACTION_LABELS[action] || "Action completed");
        } catch (err) {
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
                {!hiddenActions.has("lockdown") && appSettings?.ideal?.privacy?.selfDestruct?.enabled === true && (
                <div className="sidebar-footer">
                    <div
                        className="action-item"
                        data-tour="right-sidebar-lockdown"
                        onClick={handleSelfDestructClick}
                        data-tip={sdCountdown !== null ? "Click again to abort" : "Lockdown — runs your configured steps (edit in Secret Settings → Lockdown)"}
                        data-tip-intent="danger"
                    >
                        <ActionBtn
                            className="action-btn"
                            icon="warning-sign"
                            intent="danger"
                            minimal
                            large
                            loading={loadingAction === "selfDestruct"}
                            ariaLabel={sdCountdown !== null ? "Abort lockdown countdown" : "Run configured lockdown"}
                        />
                        <span className="action-label">
                            {sdCountdown !== null
                                ? `ABORT (${sdCountdown})`
                                : loadingAction === "selfDestruct"
                                    ? "PURGING"
                                    : "Lockdown"}
                        </span>
                    </div>
                </div>
                )}
            </div>

            {scrubDialogOpen && (
                <Suspense fallback={null}>
                    <MetadataScrubberDialog
                        isOpen
                        onClose={() => {
                            setScrubDialogOpen(false);
                            setScrubInitialPaths(undefined);
                        }}
                        initialPaths={scrubInitialPaths}
                    />
                </Suspense>
            )}


            {/* Quick Mount overlay */}
            {qmOpen && (
                <div className="qm-overlay" role="dialog" aria-modal="true"
                    onClick={(e) => { if (e.target === e.currentTarget) { setQmOpen(false); setQmEditing(null); } }}>
                    <div className="qm-dialog">
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
                                    <input
                                        className={`qm-input qm-input--letter${
                                            qmEditing.letter && quickMountSlots.some((s, i) =>
                                                s.driveLetter.toUpperCase() === qmEditing.letter.toUpperCase() &&
                                                (qmEditing.idx === 'new' || i !== qmEditing.idx)
                                            ) ? ' qm-input--conflict' : ''
                                        }`}
                                        type="text"
                                        maxLength={1}
                                        placeholder="V"
                                        value={qmEditing.letter}
                                        onChange={(e) => setQmEditing(ed => ed && ({ ...ed, letter: e.target.value.toUpperCase().slice(0, 1) }))}
                                        onKeyDown={(e) => { if (e.key === 'Enter') handleQmSaveSlot(); }}
                                    />
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
                                        disabled={!qmEditing.path.trim() || !qmEditing.letter.trim() || qmSaving}
                                        onClick={handleQmSaveSlot}>
                                        {qmSaving ? <Spinner size={14} /> : 'Save'}
                                    </button>
                                </div>
                            </>
                        ) : (
                            /* ── Mount view — dropdown + path info + password ── */
                            <>
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
                                                    onChange={(e) => { setQmSelectedIdx(Number(e.target.value)); setQmPassword(''); }}
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
