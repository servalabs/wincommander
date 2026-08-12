import { useState, useCallback, useEffect, useRef } from "react";
import { useAppState } from "../../context/AppContext";
import useBackend from "../../hooks/useBackend";
import SectionCard from "../../components/shared/SectionCard";
import VersionManagementCard from "../../components/settings/VersionManagementCard";
import ImportExportSettingsCard from "../../components/settings/ImportExportSettingsCard";
import UniversalCallout from "../../components/shared/UniversalCallout";
import UniversalToggle from "../../components/shared/UniversalToggle";
import TierGate from "../../components/shared/TierGate";
import WCSwitch from "../../components/shared/WCSwitch";
import { Icon } from "@/components/ui/bp";
import PanelHeader from "../../components/shared/PanelHeader";
import ActivationPanel from "./components/ActivationPanel";
import './index.css';

// WHY: read localStorage + html class here (not from AppShell/context) because
// this toggle is the sole writer and we want no extra store. AppShell already
// applies the class on load, so the initial read is always accurate.
function readMotionDisabled(): boolean {
    return (
        localStorage.getItem("wc-motion") === "0" ||
        document.documentElement.classList.contains("wc-no-motion")
    );
}

/** Debounces an async apply action by `delayMs` (default 5 000 ms). Mirrors
 *  the Network panel's useDebounceApply so the Censorship Protection toggle
 *  keeps its original 5s auto-apply behavior after moving here. */
function useDebounceApply(delayMs = 5000) {
    const [status, setStatus] = useState<'idle' | 'pending' | 'applying' | 'applied' | 'failed'>('idle');
    const [secsLeft, setSecsLeft] = useState(0);
    const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const tickRef = useRef<ReturnType<typeof setInterval> | null>(null);
    const resetRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const fnRef = useRef<(() => Promise<void>) | null>(null);

    const clearTimers = useCallback(() => {
        if (timerRef.current) { clearTimeout(timerRef.current); timerRef.current = null; }
        if (tickRef.current) { clearInterval(tickRef.current); tickRef.current = null; }
        if (resetRef.current) { clearTimeout(resetRef.current); resetRef.current = null; }
    }, []);

    const schedule = useCallback((fn: () => Promise<void>) => {
        fnRef.current = fn;
        clearTimers();
        setSecsLeft(Math.ceil(delayMs / 1000));
        setStatus('pending');

        tickRef.current = setInterval(() => {
            setSecsLeft(s => Math.max(0, s - 1));
        }, 1000);

        timerRef.current = setTimeout(async () => {
            clearTimers();
            setStatus('applying');
            try {
                if (fnRef.current) await fnRef.current();
                setStatus('applied');
            } catch {
                setStatus('failed');
            } finally {
                resetRef.current = setTimeout(() => setStatus('idle'), 2000);
            }
        }, delayMs);
    }, [delayMs, clearTimers]);

    useEffect(() => () => clearTimers(), [clearTimers]);

    return { schedule, status, secsLeft };
}

export default function IdentityPanel() {
    const { appSettings, patchAppSettings } = useAppState();
    const { enableDnsCensorshipProtection, disableDnsCensorshipProtection } = useBackend();

    // ── Appearance: disable all animations ──────────────────────────────────
    // WHY local state: the toggle writes directly to localStorage + html class
    // (same pair AppShell reads on boot). No new store needed — the SSOT is
    // localStorage["wc-motion"] ∪ html.wc-no-motion, consumed by
    // useMotionPreference() and the CSS reduced-motion block.
    const [motionDisabled, setMotionDisabled] = useState<boolean>(readMotionDisabled);

    const handleMotionToggle = useCallback((checked: boolean) => {
        // checked = true means "Disable all animations" is ON → motion off.
        if (checked) {
            localStorage.setItem("wc-motion", "0");
            document.documentElement.classList.add("wc-no-motion");
            document.documentElement.classList.remove("wc-motion-enabled");
        } else {
            localStorage.setItem("wc-motion", "1");
            document.documentElement.classList.remove("wc-no-motion");
            document.documentElement.classList.add("wc-motion-enabled");
        }
        setMotionDisabled(checked);
    }, []);

    // ── Censorship Protection (moved from the Network DNS card) ─────────────
    // DNS state itself (on/off, provider) lives in AppContext's shared
    // appSettings.ideal.network.dns — read it here rather than duplicating a
    // second independent toggle. Stays disabled unless Encrypted DNS is on;
    // Network's DNS-off handler clears censorshipProtection server-side too.
    const dnsOn = Boolean(appSettings?.ideal?.network?.dns?.provider);
    const persistedCensorship = Boolean(appSettings?.ideal?.network?.dns?.censorshipProtection);
    const [censorshipBusy, setCensorshipBusy] = useState(false);
    const [censorshipError, setCensorshipError] = useState<string | null>(null);
    const [optimisticCensorship, setOptimisticCensorship] = useState<boolean | null>(null);
    const effectiveCensorship = optimisticCensorship !== null ? optimisticCensorship : persistedCensorship;
    const censorshipDebounce = useDebounceApply(5000);
    const { schedule: scheduleCensorship } = censorshipDebounce;

    // KT: mirrors dnsOn so the debounced callback (scheduled up to 5s earlier)
    // reads the LIVE encrypted-DNS state at fire time, not the value captured
    // when the timer was scheduled. Without this, turning Encrypted DNS off
    // within the 5s window still lets a pending "enable censorship" apply
    // fire and create port-53 block rules with no encrypted resolver active.
    const dnsOnRef = useRef(dnsOn);
    useEffect(() => { dnsOnRef.current = dnsOn; }, [dnsOn]);

    // Reset the optimistic override once the real setting catches up, so a
    // stale optimistic value can't outlive the debounced apply.
    useEffect(() => { setOptimisticCensorship(null); }, [persistedCensorship]);

    const handleCensorshipToggle = useCallback((checked: boolean) => {
        if (censorshipBusy || !dnsOn) return;
        setCensorshipError(null);
        setOptimisticCensorship(checked);
        scheduleCensorship(async () => {
            // Re-check live DNS state: it may have changed since this was
            // scheduled. Bail out entirely on enable — never create the
            // port-53 block with no encrypted resolver active.
            if (checked && !dnsOnRef.current) {
                setOptimisticCensorship(null);
                return;
            }
            setCensorshipBusy(true);
            try {
                const response = checked
                    ? await enableDnsCensorshipProtection()
                    : await disableDnsCensorshipProtection();
                if (!response.success) throw new Error(response.error || "Could not apply censorship protection.");
                await patchAppSettings({ ideal: { network: { dns: { censorshipProtection: checked } } } });
            } catch (error) {
                setOptimisticCensorship(null);
                setCensorshipError(error instanceof Error ? error.message : "Could not apply censorship protection.");
                throw error;
            } finally {
                setCensorshipBusy(false);
            }
        });
    }, [censorshipBusy, dnsOn, enableDnsCensorshipProtection, disableDnsCensorshipProtection,
        patchAppSettings, scheduleCensorship]);

    return (
        <div className="panel-container">
            <PanelHeader
                panelId="system-identity"
                title="Settings"
                description="App preferences, logging, updates, and appearance."
            />
            <div className="identity-cols">

            {/* ── Settings Cards ── */}
            <div className="identity-col-left">
                {/* App/panel visibility, Borrowed-PC mode, and Calculator Mode
                    moved to the dedicated Secret Settings panel (2026-06-12). */}
                <SectionCard title="Logging" icon="document">
                    <UniversalToggle
                        label="Enable Logging"
                        description="Write daily log files to %LOCALAPPDATA%\WinCommander\logs\. Logs rotate per day and files older than 7 days are deleted automatically."
                        checked={appSettings?.app?.loggingEnabled !== false}
                        onChange={() => {
                            const current = appSettings?.app?.loggingEnabled !== false;
                            patchAppSettings({ app: { loggingEnabled: !current } });
                        }}
                        severity="none"
                    />
                </SectionCard>

                <SectionCard title="Updates" icon="cloud-download">
                    <div className="flex flex-col gap-3">
                        {appSettings?.app?.disableUpdates === true && (
                            <UniversalCallout
                                message="Update checks are off. WinCommander will not check for, download, or notify you of new versions until you turn this back on."
                                intent="warning"
                            />
                        )}
                        <UniversalToggle
                            label="Disable update checks"
                            description="Stop the background updater entirely — no startup scan, no banner, no restart prompt."
                            checked={appSettings?.app?.disableUpdates === true}
                            onChange={() => {
                                const current = appSettings?.app?.disableUpdates === true;
                                patchAppSettings({ app: { disableUpdates: !current } });
                            }}
                            severity="none"
                        />
                        <UniversalToggle
                            label="Auto-heal drift"
                            description="When the system drifts away from your desired settings (after an update, reboot, or OS change), automatically re-apply them in the background. Irreversible and action-type settings are never auto-healed."
                            checked={appSettings?.app?.autoHeal === true}
                            onChange={() => {
                                const current = appSettings?.app?.autoHeal === true;
                                patchAppSettings({ app: { autoHeal: !current } });
                            }}
                            severity="none"
                            icon="refresh"
                        />
                    </div>
                </SectionCard>

                <SectionCard title="System Activation" icon="key">
                    <TierGate tier="paid" featureLabel="System Activation">
                        <ActivationPanel />
                    </TierGate>
                </SectionCard>

            </div>{/* /identity-col-left */}
            <div className="identity-col-right">
                <SectionCard title="Appearance" icon="style">
                    <div className="section-subtitle mb-3">
                        <Icon icon="eye-open" size={14} />
                        ACCESSIBILITY
                    </div>
                    <UniversalToggle
                        label="Disable all animations"
                        description="Turns off all motion across the app — transitions, entrance effects, and animated counters become instant."
                        checked={motionDisabled}
                        onChange={handleMotionToggle}
                        severity="none"
                        icon="walk"
                    />
                </SectionCard>

                <SectionCard title="Censorship" icon="shield" className="identity-censorship-card">
                    <div
                        className="dns-censorship-row"
                        title={dnsOn ? "Blocks plaintext DNS (port 53) to defeat ISP hijacking" : "Turn Encrypted DNS on in Network Control to use censorship protection"}
                    >
                        <Icon icon="shield" size={12} className={`dns-censorship-row__icon${effectiveCensorship ? ' dns-censorship-row__icon--on' : ''}`} />
                        <div className="dns-censorship-row__text">
                            <div className="dns-censorship-row__title">Censorship protection</div>
                            <div className="dns-censorship-row__desc">
                                {dnsOn
                                    ? 'Blocks plaintext DNS (port 53) so network providers cannot hijack lookups.'
                                    : 'Requires Encrypted DNS in Network Control before it can block plaintext DNS.'}
                            </div>
                        </div>
                        {censorshipDebounce.status !== 'idle' && (
                            <span className={`auto-apply-pill auto-apply-pill--${censorshipDebounce.status}`}>
                                {censorshipDebounce.status === 'pending' && `Applying in ${censorshipDebounce.secsLeft}s…`}
                                {censorshipDebounce.status === 'applying' && 'Applying…'}
                                {censorshipDebounce.status === 'applied' && 'Applied'}
                                {censorshipDebounce.status === 'failed' && 'Failed'}
                            </span>
                        )}
                        <WCSwitch
                            checked={effectiveCensorship}
                            onChange={handleCensorshipToggle}
                            disabled={!dnsOn || censorshipBusy}
                            size="sm"
                            label="Censorship protection"
                        />
                    </div>
                    {censorshipError && (
                        <div className="identity-censorship-error" role="alert">{censorshipError}</div>
                    )}
                </SectionCard>

                <VersionManagementCard />

                <ImportExportSettingsCard />
            </div>{/* /identity-col-right */}
            </div>{/* /identity-cols */}

        </div>
    );
}
