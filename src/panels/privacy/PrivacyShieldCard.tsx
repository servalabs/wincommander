// src/panels/privacy/PrivacyShieldCard.tsx
// Self-contained Privacy Gaze Shield card — no props needed.
import { Button, Slider, Switch, Tooltip, Icon } from "@/components/ui/bp";
import { Segmented } from "@/components/ui/segmented";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useState, useEffect, useCallback, useRef } from "react";
import { showSuccess, showError } from "../../utils/toast";
import AIRuntimeInstaller from "./AIRuntimeInstaller";
import useBackend, { executeBackendCommand } from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import useEntitlements from "../../hooks/useEntitlements";
import useVisibility from "../../hooks/useVisibility";
import { useShieldQuotaQuery, useShieldQuotaTicker, useInvalidateShieldQuota } from "../../hooks/useShieldQuota";
import PrivacyShieldIntro from "./PrivacyShieldIntro";
import SectionCard from "../../components/shared/SectionCard";

// Module-level cache — survives panel unmount/remount
let _shieldRunningCache: boolean | null = null;
// Module-level flag — set once we've hydrated local toggle state from
// stored settings, so subsequent appSettings polls don't stomp user
// edits before they're persisted. Without this, the appSettings poll
// (every few seconds in useAppState) would re-apply the saved values
// over the live UI, flipping toggles back to false the moment the
// user clicks them but before the debounced save fires.
let _toggleHydratedFromSettings = false;

// Dedicated (i) icon next to the label rather than wrapping the whole label
// in a hover target — matches the app-wide info-icon idiom (see
// VpnKillSwitchSection's .vpn-ks-info-icon) instead of a bespoke cursor-help span.
function ShieldOption({ label, tooltip, checked, onChange, disabled }: {
    label: string; tooltip: string; checked: boolean;
    onChange: (v: boolean) => void; disabled: boolean;
}) {
    return (
        <div className="flex items-center justify-between gap-2 py-1">
            <span className="flex items-center gap-1.5 text-[13px] text-[var(--shield-text-primary)]">
                {label}
                <Tooltip content={tooltip} position="right">
                    <Icon icon="info-sign" size={11} className="physical-shield-info-icon" />
                </Tooltip>
            </span>
            <Switch checked={checked} onChange={(e) => onChange(e.currentTarget.checked)} disabled={disabled} aria-label={label} />
        </div>
    );
}

function ShieldSlider({ label, value, min, max, step, val, onChange, disabled }: {
    label: string; value: string; min: number; max: number;
    step: number; val: number; onChange: (v: number) => void; disabled: boolean;
}) {
    return (
        <div className="flex flex-col gap-1">
            <div className="flex justify-between text-[11px] text-[var(--shield-text-subtle)]">
                <span>{label}</span>
                <span className="font-mono tabular-nums text-[var(--color-accent)]">{value}</span>
            </div>
            <Slider ariaLabel={label} min={min} max={max} stepSize={step} labelRenderer={false} value={val} onChange={(v) => onChange(v)} disabled={disabled} />
        </div>
    );
}

interface PrivacyShieldCardProps {
    /** Optional content rendered inside the same "Physical Privacy"
     *  SectionCard, below the camera-based shield UI. Monitoring grids
     *  drops the Decoy monitor here so honeypot files share the
     *  "Physical Privacy" grouping. */
    extraSlot?: React.ReactNode;
}

export default function PrivacyShieldCard({ extraSlot }: PrivacyShieldCardProps = {}) {
    const { refreshPrivacy, appSettings, patchAppSettings } = useAppState();
    const { startPrivacyShield, getAIDependenciesStatus } = useBackend();
    const { hasPaid } = useEntitlements();
    const { data: quota } = useShieldQuotaQuery();
    const invalidateShieldQuota = useInvalidateShieldQuota();
    // Policy ownership and session ownership are intentionally distinct. A
    // Fleet policy supplies the locked defaults, but only a session that Fleet
    // actually started is forbidden to the local user to stop.
    const fleetPolicyManaged = appSettings?.app?.fleet?.enabled === true
        && appSettings?.ideal?.privacy?.privacyShield?.fleetManaged === true;
    const fleetShieldSessionLocked = appSettings?.app?.fleet?.enabled === true
        && appSettings?.app?.fleet?.privacyShieldSessionOwned === true;
    const fleetShieldMonitoring = fleetPolicyManaged
        && appSettings?.ideal?.privacy?.privacyShield?.fleetMonitoringEnabled === true;

    const { density } = useVisibility();
    const isAdvanced = density === 'expert';
    const [localLoading, setLocalLoading] = useState(false);
    const [showShieldIntro, setShowShieldIntro] = useState(false);
    // Single disclosure for every non-essential control (was split across a
    // header row, two always-visible mini-grids, and this chevron — now one
    // toggle covers Auto start, Camera Seen, Record Proof, Detection Mode,
    // and the processing-parameter sliders).
    const [showAdvanced, setShowAdvanced] = useState(false);

    const openShieldPaywall = useCallback(() => {
        window.dispatchEvent(new CustomEvent("license-gate-open", {
            detail: { tab: "buy", featureLabel: "Privacy Shield (unlimited time)" },
        }));
    }, []);

    const [privacyShieldRunning, _setShieldRunning] = useState<boolean | null>(_shieldRunningCache);
    const setPrivacyShieldRunning = (val: boolean | null) => { _shieldRunningCache = val; _setShieldRunning(val); };
    const [cameraAvailable, setCameraAvailable] = useState<boolean | null>(null);
    const [cameraMessage, setCameraMessage] = useState<string | null>(null);
    // Live look-away state surfaced by the backend shield reader. Only
    // meaningful while the shield is running.
    const [lookingAway, setLookingAway] = useState(false);

    // Session-anchored countdown.
    //
    // The Rust server only consumes quota in 60s ticks via consume_shield_minutes.
    // Two problems if we relied on that for display + stop:
    //   1. The chip would jump in whole-minute steps (frozen 59/60s).
    //   2. A user who stops at 50s would have nothing recorded on the
    //      server — the chip would refetch as "full quota", looking like
    //      a reset. (Worse: they could game it by restart-stop cycling.)
    //   3. The "0:00 reached but shield still running" gap until the
    //      next 60s server tick is jarring.
    //
    // Fix: anchor the timer to wall-clock from when the user clicks
    // Activate. Compute remaining as `initialSec - (now - sessionStart)`.
    // On any stop (manual or local-exhaust), record the actual elapsed
    // minutes via consume_shield_minutes so the server reflects reality.
    // When the local timer hits 0 while running, trigger the exhausted
    // path directly instead of waiting for the server tick.
    const sessionStartRef = useRef<{ start: number; initialSec: number } | null>(null);
    const localExhaustFiredRef = useRef(false);
    const [liveSecondsLeft, setLiveSecondsLeft] = useState<number | null>(null);

    const formatCountdown = (totalSeconds: number) => {
        const m = Math.floor(totalSeconds / 60);
        const s = totalSeconds % 60;
        return `${m}:${s.toString().padStart(2, "0")}`;
    };

    // Record the partial session on the server. Called from both manual
    // stop and local exhaustion so a user can't game the 60s tick window
    // by restart-stop cycling within sub-minute windows.
    const consumeElapsedSession = useCallback(async () => {
        const session = sessionStartRef.current;
        sessionStartRef.current = null;
        if (!session) return;
        const elapsedMin = (Date.now() - session.start) / 1000 / 60;
        if (elapsedMin <= 0.01) return;
        try { await invoke("consume_shield_minutes", { minutes: elapsedMin }); } catch {}
    }, []);

    const [privacyConfig, setPrivacyConfig] = useState({
        blurOnLookAway: true, blurOnMultipleFaces: true, blurOnCamera: true,
        captureOnDevice: false, captureOnMultiFace: false,
        modelLevel: 'medium', confidence: 0.5, overlayOpacity: 200,
        wakeDelayMs: 150, deviceWakeMultiplier: 5, multiFaceWakeMultiplier: 5,
        bufferFrames: 2, captureSpeed: 1,
    });
    const [aiRuntimeInstalled, setAiRuntimeInstalled] = useState<boolean | null>(null);

    const [autostart, setAutostart] = useState(false);

    // Hydrate local UI state from stored settings exactly ONCE per mount.
    // After that, user edits drive the UI and we persist outward via a
    // debounced save below — we never let later appSettings polls
    // overwrite a value the user just toggled.
    useEffect(() => {
        if (!appSettings) return;
        if (_toggleHydratedFromSettings) return;
        const ps = appSettings.ideal.privacy?.privacyShield;
        if (ps) {
            setPrivacyConfig(prev => ({
                ...prev,
                confidence: ps.confidenceThreshold ?? prev.confidence,
                overlayOpacity: ps.blurOpacity ?? prev.overlayOpacity,
                // wake_delay_seconds is *misnamed* in the Rust schema --
                // its type is u32 (no fractional seconds possible) so we
                // actually store milliseconds in it. Reading it back as
                // ms is the round-trip-stable behaviour. The legacy
                // path that multiplied by 1000 produced fractional
                // wake_delay_seconds writes (150 / 1000 = 0.15) which
                // the u32 serde guard rejected, blasting the dev log
                // with "invalid type: floating point `0.15`, expected
                // u32" once per render.
                wakeDelayMs: ps.wakeDelaySeconds ?? prev.wakeDelayMs,
                modelLevel: ps.modelSize ?? prev.modelLevel,
                bufferFrames: ps.detectionBufferFrames ?? prev.bufferFrames,
                blurOnLookAway: ps.gazeDetectionEnabled ?? prev.blurOnLookAway,
                blurOnMultipleFaces: ps.antiPeepingEnabled ?? prev.blurOnMultipleFaces,
                blurOnCamera: ps.cameraHunterEnabled ?? prev.blurOnCamera,
            }));
            setAutostart(ps.autostart ?? false);
        }
        _toggleHydratedFromSettings = true;
    }, [appSettings]);

    // A signed Fleet epoch can update this card while it is mounted. Normal
    // local edits stay protected by the one-time hydration guard above, but a
    // Fleet-owned shield must immediately reflect the new policy and must not
    // write stale local state back over it.
    useEffect(() => {
        if (!fleetPolicyManaged || !appSettings) return;
        const ps = appSettings.ideal.privacy?.privacyShield;
        if (!ps) return;
        setPrivacyConfig(prev => ({
            ...prev,
            blurOnLookAway: ps.gazeDetectionEnabled ?? prev.blurOnLookAway,
            blurOnMultipleFaces: ps.antiPeepingEnabled ?? prev.blurOnMultipleFaces,
            blurOnCamera: ps.cameraHunterEnabled ?? prev.blurOnCamera,
            confidence: ps.confidenceThreshold ?? prev.confidence,
            overlayOpacity: ps.blurOpacity ?? prev.overlayOpacity,
        }));
        setAutostart(ps.autostart ?? false);
    }, [fleetPolicyManaged, appSettings]);

    // Persist toggle + slider changes to settings as the user edits them
    // (debounced). Without this, toggles flipped ON in the UI never make
    // it to storage; the next appSettings poll then re-applies the stale
    // stored values over the UI. We skip writes while the shield is
    // running (controls are disabled anyway) and during the very first
    // render before hydration completes.
    useEffect(() => {
        if (!_toggleHydratedFromSettings) return;
        if (privacyShieldRunning === true || fleetPolicyManaged) return;
        const t = setTimeout(() => {
            patchAppSettings({ ideal: { privacy: { privacyShield: {
                gazeDetectionEnabled: privacyConfig.blurOnLookAway,
                antiPeepingEnabled: privacyConfig.blurOnMultipleFaces,
                cameraHunterEnabled: privacyConfig.blurOnCamera,
                confidenceThreshold: privacyConfig.confidence,
                blurOpacity: privacyConfig.overlayOpacity,
                wakeDelaySeconds: Math.round(privacyConfig.wakeDelayMs),
                modelSize: privacyConfig.modelLevel as any,
                detectionBufferFrames: privacyConfig.bufferFrames,
                autostart,
            } } } }).catch(() => {});
        }, 400);
        return () => clearTimeout(t);
    }, [
        privacyConfig.blurOnLookAway, privacyConfig.blurOnMultipleFaces, privacyConfig.blurOnCamera,
        privacyConfig.confidence, privacyConfig.overlayOpacity, privacyConfig.wakeDelayMs,
        privacyConfig.modelLevel, privacyConfig.bufferFrames,
        autostart, privacyShieldRunning, fleetPolicyManaged, patchAppSettings,
    ]);

    // ── Status polling, ref-based stable callback ────────────────────
    // We cannot use `useCallback([getPrivacyShieldStatus])` because the
    // useBackend hook returns a fresh function reference on every
    // render — that would make the polling effect tear down + rebuild
    // the setInterval every render and the immediate "tick()" call on
    // mount would race with the next render. Result: button gets stuck
    // on "Activate Shield" even when the Python process is running.
    //
    // Stable fix: poll via the module-level executeBackendCommand and
    // keep a single setInterval alive across the component lifetime.
    // The check function is held in a ref so handleToggle can call it
    // imperatively too.
    const checkStatusRef = useRef<() => Promise<void>>(async () => {});
    checkStatusRef.current = async () => {
        try {
            const shield = await executeBackendCommand<{ running: boolean; cameraAvailable?: boolean; cameraMessage?: string }>("Get-PrivacyShieldStatus");
            if (shield.success && shield.data && typeof shield.data.running === "boolean") {
                setPrivacyShieldRunning(shield.data.running);
                if (typeof shield.data.cameraAvailable === "boolean") {
                    setCameraAvailable(shield.data.cameraAvailable);
                    setCameraMessage(shield.data.cameraMessage ?? null);
                    if (!shield.data.cameraAvailable) setAiRuntimeInstalled(null);
                }
                await invoke("update_tray_shield_label", { running: shield.data.running }).catch(() => {});
            }
            // Malformed / undefined response → keep the last known state.
            // (Previous code's `else { setRunning(false) }` was the cause
            // of "button stuck on Activate" — a single bad reply demoted
            // a real running shield to false.)
        } catch { /* transient failure — keep last known state */ }
    };
    const checkStatus = useCallback(() => checkStatusRef.current(), []);

    useEffect(() => {
        // Status polling is ALWAYS on, regardless of the
        // `shieldModuleEnabled` flag. Earlier this effect short-circuited
        // when the module was "disabled" in settings, which made sense
        // for hiding *configuration UI*, but it also stopped reading
        // whether the Python process was actually running. So if a user
        // opened the panel without first toggling the module on, the
        // button stayed permanently on "Activate Shield" even with a
        // live shield in the background -- and clicking it spawned a
        // SECOND Python process. The status read is cheap (one WMI
        // query every 3 s) and giving the UI accurate state always
        // beats gating it on a setting the user may not have flipped.
        let cancelled = false;
        const tick = () => { if (!cancelled) void checkStatusRef.current(); };
        // Fire 3 quick checks in the first ~2 seconds so the button
        // updates immediately if the shield is already running when
        // the panel mounts — instead of waiting up to a full poll
        // interval (the original 15 s lag the user complained about).
        tick();
        const fast1 = setTimeout(tick, 600);
        const fast2 = setTimeout(tick, 1800);
        // 3s poll (was 15s). Get-PrivacyShieldStatus is a cheap WMI
        // query; 3s feels instant when toggling.
        const interval = setInterval(tick, 3000);
        return () => {
            cancelled = true;
            clearInterval(interval);
            clearTimeout(fast1);
            clearTimeout(fast2);
        };
        // Empty deps — set up the polling exactly once per mount. We
        // intentionally do NOT depend on `shieldModuleEnabled` any more,
        // see the comment above.
    }, []);

    useEffect(() => {
        if (cameraAvailable !== true) {
            if (cameraAvailable === false) setAiRuntimeInstalled(null);
            return;
        }
        const checkAI = async () => {
            try {
                const res = await getAIDependenciesStatus();
                setAiRuntimeInstalled(res.success && res.data ? res.data.installed : false);
            } catch { setAiRuntimeInstalled(false); }
        };
        checkAI();
    }, [cameraAvailable]); // eslint-disable-line react-hooks/exhaustive-deps

    // Surface the backend reader's look-away/look-back transitions. The
    // reader emits `lookingAway:false` on shield stop, so the badge clears
    // itself; guard against a stale true if running flips off independently.
    useEffect(() => {
        const unlisten = listen<{ lookingAway: boolean }>("privacy-shield-look-state", (ev) => {
            setLookingAway(ev.payload.lookingAway === true);
        });
        return () => { void unlisten.then((fn) => fn()); };
    }, []);

    useEffect(() => {
        if (privacyShieldRunning !== true) setLookingAway(false);
    }, [privacyShieldRunning]);

    const handleShieldExhausted = useCallback(async () => {
        try { await executeBackendCommand<{ success: boolean }>("Stop-PrivacyShield", {}); } catch { }
        await consumeElapsedSession();
        setPrivacyShieldRunning(false);
        await invoke("update_tray_shield_label", { running: false }).catch(() => {});
        openShieldPaywall();
        invalidateShieldQuota();
    }, [consumeElapsedSession, invalidateShieldQuota, openShieldPaywall]);

    // Fleet-run sessions are charged/stopped by BackgroundPollers so the
    // monitoring service remains correct even when this card is unmounted.
    useShieldQuotaTicker(privacyShieldRunning === true && !fleetShieldSessionLocked, handleShieldExhausted);
    const quotaMinutesRemaining = quota?.minutes_remaining;
    const quotaIsUnlimited = quota?.is_unlimited;

    // Session anchor + smooth countdown. When running flips to true,
    // capture the start timestamp and the remaining-seconds budget from
    // the current quota. Tick the displayed value down from wall-clock.
    // When it hits 0 (free tier only), trigger the exhausted handler so
    // the shield actually stops at the displayed deadline, not 60s later
    // on the next server tick. When idle, just mirror the server value.
    useEffect(() => {
        if (privacyShieldRunning !== true) {
            if (quotaMinutesRemaining === undefined || quotaIsUnlimited) { setLiveSecondsLeft(null); return; }
            setLiveSecondsLeft(Math.max(0, Math.round(quotaMinutesRemaining * 60)));
            return;
        }
        if (sessionStartRef.current === null) {
            if (quotaMinutesRemaining === undefined || quotaIsUnlimited) return;
            sessionStartRef.current = {
                start: Date.now(),
                initialSec: Math.max(0, Math.round(quotaMinutesRemaining * 60)),
            };
            localExhaustFiredRef.current = false;
        }
        const tick = () => {
            const session = sessionStartRef.current;
            if (!session) return;
            const elapsedSec = (Date.now() - session.start) / 1000;
            const remaining = Math.max(0, session.initialSec - elapsedSec);
            setLiveSecondsLeft(Math.ceil(remaining));
            if (remaining <= 0 && !localExhaustFiredRef.current && !quotaIsUnlimited) {
                localExhaustFiredRef.current = true;
                handleShieldExhausted();
            }
        };
        tick();
        const id = setInterval(tick, 500);
        return () => clearInterval(id);
    }, [privacyShieldRunning, quotaMinutesRemaining, quotaIsUnlimited, handleShieldExhausted]);

    // Safety net: if the status poll positively confirms the shield is
    // running, force-clear localLoading. Without this, a slow / hung
    // startPrivacyShield IPC call could leave the button stuck on a
    // spinner forever even though the Python process is already alive
    // and answering the status poll. Belt-and-braces with the explicit
    // setLocalLoading(false) in handleToggle's finally block.
    useEffect(() => {
        if (privacyShieldRunning === true && localLoading) setLocalLoading(false);
    }, [privacyShieldRunning, localLoading]);

    const handleToggle = async () => {
        if (fleetShieldSessionLocked) {
            showError("Privacy Shield was started by Fleet and can only be stopped by a Fleet administrator.");
            return;
        }
        if (!privacyShieldRunning && fleetPolicyManaged) {
            showError("Privacy Shield activation is managed by Fleet. The device will start it after the Fleet policy arrives.");
            return;
        }
        const targetState = !privacyShieldRunning;
        if (targetState && !privacyConfig.blurOnLookAway && !privacyConfig.blurOnMultipleFaces && !privacyConfig.blurOnCamera) {
            showError("Enable at least one blur trigger before starting the Privacy Shield.");
            return;
        }
        if (targetState && cameraAvailable === false) {
            showError(cameraMessage || "No camera detected — Privacy Shield requires a webcam.");
            return;
        }
        if (targetState && quota && !quota.is_unlimited && quota.minutes_remaining <= 0) { openShieldPaywall(); return; }
        setLocalLoading(true);
        // Hard ceiling: if the Rust IPC call hangs (dep install stuck,
        // WMI timeout, etc.) free the button after 60 seconds so the
        // user can re-try / open the log. The status poll continues
        // running independently so the real state still surfaces.
        const loadingTimeout = setTimeout(() => {
            setLocalLoading(false);
            checkStatus();
        }, 60_000);
        try {
            if (targetState) {
                const res = await startPrivacyShield(
                    0, privacyConfig.blurOnLookAway, privacyConfig.blurOnMultipleFaces, privacyConfig.blurOnCamera,
                    privacyConfig.captureOnDevice, privacyConfig.captureOnMultiFace, privacyConfig.modelLevel,
                    privacyConfig.confidence, privacyConfig.overlayOpacity, privacyConfig.wakeDelayMs,
                    privacyConfig.deviceWakeMultiplier, privacyConfig.multiFaceWakeMultiplier,
                    privacyConfig.bufferFrames, privacyConfig.captureSpeed,
                );
                if (res.success) {
                    await showSuccess("Privacy Shield activated.");
                    setPrivacyShieldRunning(true);
                    setAiRuntimeInstalled(true);
                    await invoke("update_tray_shield_label", { running: true });
                    patchAppSettings({ ideal: { privacy: { privacyShield: {
                        gazeDetectionEnabled: privacyConfig.blurOnLookAway,
                        antiPeepingEnabled: privacyConfig.blurOnMultipleFaces,
                        cameraHunterEnabled: privacyConfig.blurOnCamera,
                        confidenceThreshold: privacyConfig.confidence,
                        blurOpacity: privacyConfig.overlayOpacity,
                        wakeDelaySeconds: Math.round(privacyConfig.wakeDelayMs),
                        modelSize: privacyConfig.modelLevel as any,
                        detectionBufferFrames: privacyConfig.bufferFrames,
                    } } } }).catch(() => {});
                } else {
                    const err = (res.error || "").toLowerCase();
                    if (err.includes("python is required") || err.includes("missing python dependency")) setAiRuntimeInstalled(false);
                    else if (err.includes('camera') || err.includes('webcam') || err.includes('cap.read') || err.includes('videocapture') || err.includes('no camera') || err.includes('no webcam')) {
                        setCameraAvailable(false);
                        setCameraMessage(res.error || "No camera detected — Privacy Shield requires a webcam.");
                        setAiRuntimeInstalled(null);
                        showError(res.error || "No camera detected — Privacy Shield requires a webcam.");
                    }
                    else showError(res.error || "Failed to start Privacy Shield.");
                }
            } else {
                const res = await executeBackendCommand<{ success: boolean; message?: string }>("Stop-PrivacyShield", {});
                await consumeElapsedSession();
                invalidateShieldQuota();
                if (res.success) { setPrivacyShieldRunning(false); await invoke("update_tray_shield_label", { running: false }); showSuccess("Privacy Shield deactivated."); }
                else {
                    // Belt-and-braces: also call the Rust-side process killer
                    // so a stuck Python that the PS Stop-Process missed still
                    // dies on the user's click instead of forcing Task Manager.
                    await invoke("kill_privacy_shield_process").catch(() => {});
                    setPrivacyShieldRunning(false);
                    await invoke("update_tray_shield_label", { running: false }).catch(() => {});
                    showSuccess("Privacy Shield force-stopped.");
                }
            }
        } catch (e) { showError(e instanceof Error ? e.message : "Operation failed"); }
        finally {
            clearTimeout(loadingTimeout);
            setLocalLoading(false);
            // Reconcile against the actual process state right away so the
            // button label updates the moment the start/stop returns,
            // instead of waiting up to a full poll interval.
            checkStatus();
            setTimeout(() => { checkStatus(); }, 800);
            setTimeout(() => { checkStatus(); }, 2200);
        }
    };

    const headerRight = (
        <Button
            text={privacyShieldRunning ? (isAdvanced ? "Stop Shield" : "Turn Off") : (isAdvanced ? "Activate Shield" : "Turn On")}
            intent={privacyShieldRunning ? "danger" : "primary"}
            onClick={handleToggle}
            disabled={fleetShieldSessionLocked || (!privacyShieldRunning && fleetPolicyManaged) || (privacyShieldRunning ? false : localLoading || cameraAvailable === false)}
            loading={privacyShieldRunning ? false : localLoading}
            className="shield-primary-btn physical-shield-header-btn"
        />
    );

    return (
        <div data-tour="privacy-shield-card">
        <SectionCard
            title={isAdvanced ? "Physical Privacy" : "Screen privacy"}
            headerRight={headerRight}
            className="physical-privacy-card"
            armed={privacyShieldRunning === true}
        >
            <div className="flex flex-col gap-5">
                <div className="physical-shield-head">
                    <div className="physical-shield-copy flex flex-col gap-2 min-w-0">
                        {/* Title row — name + info icon only. "How it works" and
                            "Auto start" used to crowd this line; the former is now
                            the (i) icon, the latter lives in the advanced disclosure
                            below (it's Pro-gated and rarely touched). */}
                        <div className="flex items-center gap-2">
                            <div className={`size-2 rounded-full flex-shrink-0 ${privacyShieldRunning ? 'bg-[var(--color-success)]' : 'bg-[var(--color-text-muted)]'}`} />
                            <span className="text-sm font-semibold text-[var(--shield-text-primary)]">
                                Privacy Gaze Shield
                            </span>
                            <Tooltip content="See how the Gaze Shield decides when to blur your screen." position="right">
                                {/* Unlike the tooltip-only info icons on each toggle
                                    below, this one is also clickable (opens the intro
                                    dialog) — cursor:pointer overrides the shared
                                    cursor:help so the affordance still reads clearly. */}
                                <button
                                    type="button"
                                    aria-label="How Privacy Gaze Shield works"
                                    className="inline-flex"
                                    data-tour="privacy-shield-how-it-works"
                                    onClick={() => setShowShieldIntro(true)}
                                >
                                    <Icon icon="info-sign" size={11} className="physical-shield-info-icon" />
                                </button>
                            </Tooltip>
                        </div>
                        {/* Single badge group — Active / Looking away / FREE tier /
                            quota / Camera unavailable used to be two visually
                            identical rows 4px apart; now one flex-wrap group. */}
                        {/* `cameraAvailable === false` alone suffices here (not
                            `&& privacyShieldRunning !== true` too) — by this point
                            in the `||` chain, the running check already failed, so
                            TS correctly flags the extra comparison as redundant. */}
                        {(privacyShieldRunning || (!hasPaid && quota && !quota.is_unlimited) || cameraAvailable === false) && (
                            <div className="flex flex-wrap items-center gap-1.5">
                                {privacyShieldRunning && (
                                    <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0">Active</span>
                                )}
                                {privacyShieldRunning && lookingAway && (
                                    <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-warning)]/15 text-[var(--color-warning)] border border-[var(--color-warning)]/30 flex-shrink-0">Looking away · webcam blocked</span>
                                )}
                                {!hasPaid && quota && !quota.is_unlimited && (
                                    <>
                                        <button type="button" aria-label="Upgrade Privacy Shield for unlimited time" className="text-[10px] px-2 py-0.5 rounded flex-shrink-0 cursor-pointer"
                                            title={`Free tier — ${quota.hard_cap_minutes} min per day. Upgrade for unlimited use.`}
                                            onClick={openShieldPaywall}
                                            style={{ background: 'var(--color-accent-dim, rgba(0,160,255,0.12))', color: 'var(--color-accent)', border: '1px solid var(--color-accent)', fontFamily: 'var(--font-mono)', letterSpacing: '0.5px' }}>
                                            FREE
                                        </button>
                                        {quota.minutes_remaining > 0 ? (
                                            <span className="text-[10px] px-2 py-0.5 rounded flex-shrink-0 tabular-nums"
                                                title={privacyShieldRunning
                                                    ? "Counts down every second while the shield is running."
                                                    : `Daily cap is ${quota.hard_cap_minutes} min. Resets at midnight.`}
                                                style={{
                                                    background: 'var(--shield-inner-bg)',
                                                    color: liveSecondsLeft !== null && liveSecondsLeft <= 60 ? 'var(--color-warning)' : 'var(--shield-text-subtle)',
                                                    border: `1px solid ${liveSecondsLeft !== null && liveSecondsLeft <= 60 ? 'var(--color-warning)' : 'var(--shield-inner-border)'}`,
                                                    fontFamily: 'var(--font-mono)',
                                                }}>
                                                {privacyShieldRunning && liveSecondsLeft !== null
                                                    ? `${formatCountdown(liveSecondsLeft)} left`
                                                    : `${Math.ceil(quota.minutes_remaining)} / ${quota.hard_cap_minutes} min today`}
                                            </span>
                                        ) : (
                                            <button type="button" aria-label="Upgrade Privacy Shield after reaching the daily limit" className="text-[10px] px-2 py-0.5 rounded flex-shrink-0 cursor-pointer" title="Free-tier daily cap reached." onClick={openShieldPaywall}
                                                style={{ background: 'var(--color-warning-dim)', color: 'var(--color-warning)', border: '1px solid var(--color-warning)', fontFamily: 'var(--font-mono)', letterSpacing: '0.5px' }}>
                                                DAILY LIMIT REACHED
                                            </button>
                                        )}
                                    </>
                                )}
                                {cameraAvailable === false && privacyShieldRunning !== true && (
                                    <span className="text-[10px] px-2 py-0.5 rounded flex-shrink-0"
                                        style={{ background: 'var(--color-warning-dim)', color: 'var(--color-warning)', border: '1px solid var(--color-warning)' }}>
                                        Camera unavailable
                                    </span>
                                )}
                            </div>
                        )}
                        {/* Description + trust line flow by content length now —
                            was a rigid 2-col grid forcing both onto equal-width
                            columns regardless of text length. */}
                        <div className="physical-shield-summary-row">
                            <p className="text-xs text-[var(--shield-text-subtle)] text-pretty">
                                Blurs screen when unauthorized presence or threats are detected.
                            </p>
                            <p className="text-[10px] text-[var(--color-text-muted)] flex items-center gap-1.5">
                                <Icon icon="lock" size={10} style={{ color: 'var(--color-success)' }} />
                                100% on-device - no camera frames or data sent to the cloud.
                            </p>
                        </div>
                        {fleetPolicyManaged && (
                            <p className="text-[10px] text-[var(--color-accent)] mt-1">
                                Fleet policy managed · {fleetShieldSessionLocked
                                    ? "this session was started by Fleet and can only be stopped there."
                                    : fleetShieldMonitoring
                                        ? "the locked Fleet defaults will be used when Fleet starts the shield."
                                        : "monitoring is off by Fleet policy."}
                            </p>
                        )}
                        {cameraAvailable === false && privacyShieldRunning !== true && (
                            <p className="text-[10px] text-[var(--color-warning)] mt-1 max-w-[320px]">
                                {cameraMessage || "No usable webcam was detected on this PC, so Privacy Shield cannot monitor the screen."}
                            </p>
                        )}
                    </div>
                </div>

                {cameraAvailable === true && aiRuntimeInstalled === false && (
                    <AIRuntimeInstaller onInstalled={() => { setAiRuntimeInstalled(true); refreshPrivacy(); }} />
                )}

                {cameraAvailable !== false && (
                <div className={`rounded-md border border-[var(--shield-inner-border)] bg-[var(--shield-inner-bg)] p-4 ${fleetPolicyManaged ? "pointer-events-none opacity-70" : ""}`}>
                    {/* Blur conditions are intentionally kept together: these
                        are the three inputs that directly decide whether the
                        screen is obscured. */}
                    <div className="flex flex-col gap-3">
                        <span className="text-[10px] font-medium text-[var(--shield-text-muted)]">{isAdvanced ? "Blur triggers" : "Activation Triggers"}</span>
                        <div className="grid grid-cols-[repeat(auto-fit,minmax(min(100%,160px),1fr))] gap-3">
                            <div><ShieldOption label={isAdvanced ? "Look away" : "Look Away"} tooltip="Blurs when eyes are not detected on screen." checked={privacyConfig.blurOnLookAway} onChange={(v) => setPrivacyConfig(p => ({ ...p, blurOnLookAway: v }))} disabled={privacyShieldRunning === true || fleetPolicyManaged} /></div>
                            <div><ShieldOption label="Multiple faces" tooltip="Blurs when more than one person is detected." checked={privacyConfig.blurOnMultipleFaces} onChange={(v) => setPrivacyConfig(p => ({ ...p, blurOnMultipleFaces: v }))} disabled={privacyShieldRunning === true || fleetPolicyManaged} /></div>
                            <div><ShieldOption label={isAdvanced ? "Phone / camera" : "Camera Seen"} tooltip="Experimental: blurs when a phone or camera is pointed at the screen." checked={privacyConfig.blurOnCamera} onChange={(v) => setPrivacyConfig(p => ({ ...p, blurOnCamera: v }))} disabled={privacyShieldRunning === true || fleetPolicyManaged} /></div>
                        </div>
                    </div>

                    <div className="mt-4 pt-4 border-t border-[var(--shield-inner-border)]">
                        {/* Launch behaviour is deliberately not a model
                            parameter, so it gets its own direct row without
                            an invented group title. */}
                        <ShieldOption
                            label={isAdvanced ? "Auto start" : "Auto start on launch"}
                            tooltip={hasPaid
                                ? "Automatically activate Privacy Shield a few seconds after the app launches. Skipped if no camera or the AI runtime isn't installed."
                                : "Pro feature - auto-activate Privacy Shield after launch."}
                            checked={autostart}
                            onChange={(v) => {
                                if (v && !hasPaid) { openShieldPaywall(); return; }
                                setAutostart(v);
                            }}
                            disabled={fleetPolicyManaged}
                        />

                        <button type="button" className="flex w-full items-center justify-between cursor-pointer hover:opacity-80 transition-opacity" onClick={() => setShowAdvanced(!showAdvanced)} aria-expanded={showAdvanced} aria-controls="privacy-shield-processing-parameters">
                            <span className="text-[10px] font-medium text-[var(--shield-text-muted)] block">{isAdvanced ? "Processing parameters" : "Advanced Settings"}</span>
                            <Icon icon={showAdvanced ? "chevron-up" : "chevron-down"} size={12} color="var(--shield-text-muted)" />
                        </button>
                        {showAdvanced && (
                            <div id="privacy-shield-processing-parameters" className="mt-4 flex flex-col gap-4">
                                <div className="flex flex-col gap-3 pt-3 border-t border-[var(--shield-inner-border)]">
                                    <span className="text-[10px] font-medium text-[var(--shield-text-muted)]">{isAdvanced ? "Capture on incident" : "Record Proof"}</span>
                                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                        <ShieldOption label={isAdvanced ? "Phone Detected" : "Save Camera Proof"} tooltip="Saves screenshot and webcam when phone/camera is detected." checked={privacyConfig.captureOnDevice} onChange={(v) => setPrivacyConfig(p => ({ ...p, captureOnDevice: v }))} disabled={privacyShieldRunning === true} />
                                        <ShieldOption label={isAdvanced ? "Multiple faces" : "Save Person Proof"} tooltip="Saves screenshot and webcam when multiple faces detected." checked={privacyConfig.captureOnMultiFace} onChange={(v) => setPrivacyConfig(p => ({ ...p, captureOnMultiFace: v }))} disabled={privacyShieldRunning === true} />
                                    </div>
                                </div>

                                <div className="pt-3 border-t border-[var(--shield-inner-border)]">
                                    <span className="text-[10px] font-medium text-[var(--shield-text-muted)] block mb-2">{isAdvanced ? "Model" : "Detection Mode"}</span>
                                    {/* Segmented has no disabled prop — pointer-events-none
                                        blocks interaction visually, the onValueChange guard
                                        below blocks it programmatically (matches the old
                                        hand-rolled buttons' disabled + guarded onClick). */}
                                    <div className={privacyShieldRunning === true ? 'opacity-50 pointer-events-none' : ''}>
                                        <Segmented
                                            value={privacyConfig.modelLevel}
                                            onValueChange={(v) => { if (!privacyShieldRunning) setPrivacyConfig(p => ({ ...p, modelLevel: v })); }}
                                            options={[
                                                { value: 'nano', label: 'nano' },
                                                { value: 'small', label: 'small' },
                                                { value: 'medium', label: 'medium' },
                                                { value: 'large', label: 'large' },
                                            ]}
                                            size="sm"
                                        />
                                    </div>
                                </div>

                                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-3 border-t border-[var(--shield-inner-border)]">
                                    <ShieldSlider label={isAdvanced ? "Confidence" : "Sensitivity"} value={`${(privacyConfig.confidence * 100).toFixed(0)}%`} min={0.1} max={0.9} step={0.05} val={privacyConfig.confidence} onChange={(v) => setPrivacyConfig(p => ({ ...p, confidence: v }))} disabled={privacyShieldRunning === true} />
                                    <ShieldSlider label={isAdvanced ? "Overlay opacity" : "Blur Strength"} value={`${((privacyConfig.overlayOpacity / 255) * 100).toFixed(0)}%`} min={50} max={255} step={5} val={privacyConfig.overlayOpacity} onChange={(v) => setPrivacyConfig(p => ({ ...p, overlayOpacity: v }))} disabled={privacyShieldRunning === true} />
                                    <ShieldSlider label={isAdvanced ? "Wake delay" : "Wake Delay"} value={`${privacyConfig.wakeDelayMs}ms`} min={50} max={1500} step={50} val={privacyConfig.wakeDelayMs} onChange={(v) => setPrivacyConfig(p => ({ ...p, wakeDelayMs: v }))} disabled={privacyShieldRunning === true} />
                                    <ShieldSlider label={isAdvanced ? "Device multiplier" : "Camera Strength"} value={`${privacyConfig.deviceWakeMultiplier}x`} min={1} max={20} step={1} val={privacyConfig.deviceWakeMultiplier} onChange={(v) => setPrivacyConfig(p => ({ ...p, deviceWakeMultiplier: v }))} disabled={privacyShieldRunning === true || !privacyConfig.blurOnCamera} />
                                    <ShieldSlider label={isAdvanced ? "Multi-face multiplier" : "Person Strength"} value={`${privacyConfig.multiFaceWakeMultiplier}x`} min={1} max={20} step={1} val={privacyConfig.multiFaceWakeMultiplier} onChange={(v) => setPrivacyConfig(p => ({ ...p, multiFaceWakeMultiplier: v }))} disabled={privacyShieldRunning === true || !privacyConfig.blurOnMultipleFaces} />
                                    <ShieldSlider label={isAdvanced ? "Detection buffer (frames)" : "Blur Stability"} value={`${privacyConfig.bufferFrames}`} min={1} max={8} step={1} val={privacyConfig.bufferFrames} onChange={(v) => setPrivacyConfig(p => ({ ...p, bufferFrames: v }))} disabled={privacyShieldRunning === true} />
                                    <ShieldSlider label={isAdvanced ? "Capture speed (playback)" : "Playback Speed"} value={`${privacyConfig.captureSpeed}x`} min={1} max={4} step={1} val={privacyConfig.captureSpeed} onChange={(v) => setPrivacyConfig(p => ({ ...p, captureSpeed: v }))} disabled={privacyShieldRunning === true} />
                                </div>
                            </div>
                        )}
                    </div>
                </div>
                )}
            </div>
            <PrivacyShieldIntro isOpen={showShieldIntro} onClose={() => setShowShieldIntro(false)} />
            {extraSlot && (
                <div style={{ marginTop: 12 }}>
                    {extraSlot}
                </div>
            )}
        </SectionCard>
        </div>
    );
}
