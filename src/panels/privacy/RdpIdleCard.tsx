// src/panels/privacy/RdpIdleCard.tsx
// Self-contained RDP Idle Disconnect card — no props needed.
import { Switch, HTMLSelect, Icon, Spinner, Tag, CheckboxControl } from "@/components/ui/bp";
import { useCallback, useEffect, useState } from "react";
import { useAppState } from "../../context/AppContext";
import useEntitlements from "../../hooks/useEntitlements";
import useVisibility from "../../hooks/useVisibility";
import SectionCard from "../../components/shared/SectionCard";
import ConflictToggleDialog from "../../components/shared/ConflictToggleDialog";
import { executeBackendCommand } from "../../hooks/useBackend";

function supportsRdpHost(osName?: string | null): boolean {
    if (!osName) return false;
    const n = osName.toLowerCase();
    if (!n.includes("windows")) return false;
    if (n.includes(" home")) return false;
    return n.includes("pro") || n.includes("enterprise") || n.includes("education") || n.includes("server");
}

// Values are SECONDS. Minimum is 1 minute: incoming (remote) RDP sessions
// report idle via quser at whole-minute resolution, so sub-minute timeouts
// cannot be honored reliably for them and are not offered.
const RDP_INCOMING_PRESETS = [
    { label: '1 minute',   value: 60   },
    { label: '5 minutes',  value: 300  },
    { label: '10 minutes', value: 600  },
    { label: '15 minutes', value: 900  },
    { label: '30 minutes', value: 1800 },
    { label: '60 minutes', value: 3600 },
    { label: 'Custom…',    value: -1   },
];

const RDP_TIMEOUT_PRESETS = [
    { label: '1 minute',   value: 60   },
    { label: '2 minutes',  value: 120  },
    { label: '5 minutes',  value: 300  },
    { label: '10 minutes', value: 600  },
    { label: '15 minutes', value: 900  },
    { label: '30 minutes', value: 1800 },
    { label: 'Custom…',    value: -1   },
];

const RDP_WARNING_PRESETS = [
    { label: '5 seconds',  value: 5  },
    { label: '10 seconds', value: 10 },
    { label: '15 seconds', value: 15 },
    { label: '30 seconds', value: 30 },
    { label: '1 minute',   value: 60 },
    { label: 'Custom…',    value: -1 },
];

export default function RdpIdleCard() {
    const { appSettings, patchAppSettings, refreshSettings, systemInfo } = useAppState();
    const { hasPaid } = useEntitlements();

    const { density } = useVisibility();
    const isAdvanced = density === 'expert';
    const canHostRdp = supportsRdpHost(systemInfo?.osName);

    const rdpIdleEnabled  = appSettings?.ideal?.privacy?.tracking?.rdpIdleDisconnectEnabled ?? false;
    const rdpIdleTimeout  = appSettings?.ideal?.privacy?.tracking?.rdpIdleDisconnectTimeout ?? 120;
    const rdpIdleWarningSeconds = appSettings?.ideal?.privacy?.tracking?.rdpIdleWarningSeconds ?? 5;
    const rdpClearCache   = appSettings?.ideal?.privacy?.tracking?.rdpClearCacheOnDisconnect ?? true;
    const rdpRemoveCreds  = appSettings?.ideal?.privacy?.tracking?.rdpRemoveCredsOnDisconnect ?? false;
    const rdpDismountVaults = appSettings?.ideal?.privacy?.tracking?.rdpDismountVaultsOnDisconnect ?? false;
    const [rdpDismountDraft, setRdpDismountDraft] = useState(rdpDismountVaults);

    useEffect(() => {
        setRdpDismountDraft(rdpDismountVaults);
    }, [rdpDismountVaults]);

    const lockedPaths = appSettings?.policy?.lockedPaths ?? [];
    const isLocked = (path: string) => lockedPaths.some(p => p.trim().length > 0 && path.startsWith(p));
    const rdpIdleLocked       = isLocked("ideal.privacy.tracking.rdpIdleDisconnectEnabled");
    const rdpClearCacheLocked = isLocked("ideal.privacy.tracking.rdpClearCacheOnDisconnect");
    const rdpRemoveCredsLocked= isLocked("ideal.privacy.tracking.rdpRemoveCredsOnDisconnect");
    const rdpDismountLocked   = isLocked("ideal.privacy.tracking.rdpDismountVaultsOnDisconnect");

    const [timeoutPreset, setTimeoutPreset] = useState<number>(() =>
        RDP_TIMEOUT_PRESETS.slice(0, -1).some(p => p.value === rdpIdleTimeout) ? rdpIdleTimeout : -1
    );
    const [rdpCustomTime, setRdpCustomTime] = useState<string>(String(rdpIdleTimeout));
    const [warningPreset, setWarningPreset] = useState<number>(() =>
        RDP_WARNING_PRESETS.slice(0, -1).some(p => p.value === rdpIdleWarningSeconds) ? rdpIdleWarningSeconds : -1
    );
    const [rdpCustomWarningTime, setRdpCustomWarningTime] = useState<string>(String(rdpIdleWarningSeconds));

    useEffect(() => {
        setTimeoutPreset(RDP_TIMEOUT_PRESETS.slice(0, -1).some(p => p.value === rdpIdleTimeout) ? rdpIdleTimeout : -1);
        setRdpCustomTime(String(rdpIdleTimeout));
    }, [rdpIdleTimeout]);

    useEffect(() => {
        setWarningPreset(RDP_WARNING_PRESETS.slice(0, -1).some(p => p.value === rdpIdleWarningSeconds) ? rdpIdleWarningSeconds : -1);
        setRdpCustomWarningTime(String(rdpIdleWarningSeconds));
    }, [rdpIdleWarningSeconds]);

    // Server-side incoming idle sign-out (canonical unit: SECONDS).
    // Migration: fall back to the legacy minute field × 60, then 15 min.
    const rdpIncomingEnabled = appSettings?.ideal?.tweaks?.rdp?.incomingIdleTimeoutEnabled ?? false;
    const rdpNoTimeoutsEnabled = appSettings?.ideal?.tweaks?.rdp?.noTimeouts ?? false;
    const rdpIncomingDismount = appSettings?.ideal?.tweaks?.rdp?.incomingDismountOnEmpty ?? false;
    const rdpIncomingSignOffOnDisconnect = appSettings?.ideal?.tweaks?.rdp?.incomingSignOffOnDisconnect ?? false;
    const [incomingDismountDraft, setIncomingDismountDraft] = useState(rdpIncomingDismount);
    const [incomingSignOffDraft, setIncomingSignOffDraft] = useState(rdpIncomingSignOffOnDisconnect);

    useEffect(() => { setIncomingDismountDraft(rdpIncomingDismount); }, [rdpIncomingDismount]);
    useEffect(() => { setIncomingSignOffDraft(rdpIncomingSignOffOnDisconnect); }, [rdpIncomingSignOffOnDisconnect]);
    const rdpIncomingSeconds = appSettings?.ideal?.tweaks?.rdp?.incomingIdleTimeoutSeconds
        ?? ((appSettings?.ideal?.tweaks?.rdp?.incomingIdleTimeoutMinutes ?? 15) * 60);
    const isCustomIncoming = !RDP_INCOMING_PRESETS.slice(0, -1).map(p => p.value).includes(rdpIncomingSeconds);
    const [incomingCustomMode, setIncomingCustomMode] = useState(isCustomIncoming);
    const [incomingCustomSecs, setIncomingCustomSecs] = useState<string>(String(rdpIncomingSeconds));
    const [applyingIncoming, setApplyingIncoming] = useState(false);
    const [incomingError, setIncomingError] = useState<string | null>(null);
    const [pendingIncomingConflict, setPendingIncomingConflict] = useState(false);

    const disableRdpNoTimeouts = useCallback(async () => {
        const res = await executeBackendCommand("Disable-RdpNoTimeouts");
        if (!res.success) {
            throw new Error(res.error ?? "Failed to disable RDP No Timeouts");
        }
        await refreshSettings();
    }, [refreshSettings]);

    const applyIncoming = async (enabled: boolean, seconds: number) => {
        setApplyingIncoming(true);
        setIncomingError(null);
        try {
            const res = enabled
                ? await executeBackendCommand("Enable-RdpIncomingIdleTimeout", { Seconds: String(seconds) })
                : await executeBackendCommand("Disable-RdpIncomingIdleTimeout");
            if (!res.success) { setIncomingError(res.error ?? "Command failed"); return; }
            await patchAppSettings({ ideal: { tweaks: { rdp: {
                incomingIdleTimeoutEnabled: enabled,
                incomingIdleTimeoutSeconds: enabled ? seconds : null,
            } } } } as any).catch(() => {});
            await refreshSettings().catch(() => {});
        } catch (e) {
            setIncomingError(e instanceof Error ? e.message : String(e));
        } finally {
            setApplyingIncoming(false);
        }
    };

    const enableIncomingWithConflictCheck = () => {
        if (rdpNoTimeoutsEnabled) {
            setPendingIncomingConflict(true);
            return;
        }
        void applyIncoming(true, rdpIncomingSeconds);
    };

    const confirmIncomingConflict = async () => {
        setPendingIncomingConflict(false);
        setApplyingIncoming(true);
        setIncomingError(null);
        try {
            await disableRdpNoTimeouts();
            await applyIncoming(true, rdpIncomingSeconds);
        } catch (e) {
            setIncomingError(e instanceof Error ? e.message : String(e));
            setApplyingIncoming(false);
        }
    };

    const patchRdpTracking = (patch: Record<string, unknown>) =>
        patchAppSettings({ ideal: { privacy: { tracking: patch } } } as any).catch(() => {});

    const patchRdpDismountVaults = (next: boolean) => {
        setRdpDismountDraft(next);
        patchAppSettings({ ideal: { privacy: { tracking: { rdpDismountVaultsOnDisconnect: next } } } } as any)
            .catch((err) => {
                setRdpDismountDraft(rdpDismountVaults);
                console.error("[RdpIdle] Dismount checkbox save failed:", err);
            });
    };

    const patchRdpIncomingDismount = (next: boolean) => {
        setIncomingDismountDraft(next);
        patchAppSettings({ ideal: { tweaks: { rdp: { incomingDismountOnEmpty: next } } } } as any)
            .catch((err) => {
                setIncomingDismountDraft(rdpIncomingDismount);
                console.error("[RdpIdle] Incoming dismount checkbox save failed:", err);
            });
    };

    const patchRdpIncomingSignOffOnDisconnect = (next: boolean) => {
        setIncomingSignOffDraft(next);
        patchAppSettings({ ideal: { tweaks: { rdp: { incomingSignOffOnDisconnect: next } } } } as any)
            .catch((err) => {
                setIncomingSignOffDraft(rdpIncomingSignOffOnDisconnect);
                console.error("[RdpIdle] Incoming sign-off-on-disconnect checkbox save failed:", err);
            });
    };

    // Hide entirely if the user is free-tier AND doesn't have a Pro-capable host
    if (!canHostRdp && !hasPaid) return null;

    const rdpActiveCount = (hasPaid && rdpIdleEnabled ? 1 : 0) + (hasPaid && rdpIncomingEnabled ? 1 : 0);
    const headerRight = canHostRdp ? (
        <Tag minimal intent={rdpActiveCount > 0 ? "success" : "none"} className="font-mono">
            {rdpActiveCount > 0 ? `${rdpActiveCount} ACTIVE` : "OFF"}
        </Tag>
    ) : undefined;

    return (
        <SectionCard title="RDP Idle" icon="desktop" headerRight={headerRight}>
            {!canHostRdp && hasPaid ? (
                <div className="rounded-lg border p-5 flex items-start gap-3"
                    style={{ background: 'var(--shield-bg-idle)', borderColor: 'var(--color-border)', opacity: 0.85 }}>
                    <Icon icon="lock" size={18} className="text-[var(--color-text-muted)] flex-shrink-0 mt-0.5" />
                    <div className="flex flex-col gap-1 min-w-0">
                        <div className="flex items-center gap-2 flex-wrap">
                            <span className="text-sm font-semibold text-[var(--shield-text-primary)]">Not supported on this Windows edition</span>
                            <span className="text-[9px] px-2 py-0.5 rounded flex-shrink-0"
                                style={{ background: 'rgba(148,163,184,0.12)', color: 'var(--color-text-muted)', border: '1px solid var(--color-border)', fontFamily: 'var(--font-mono)', fontWeight: 700, letterSpacing: 1 }}>
                                EDITION LOCKED
                            </span>
                        </div>
                        <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[480px]">
                            RDP host features require Windows Pro, Enterprise, Education, or Server. Your current edition ({systemInfo?.osName || "Windows Home"}) cannot host inbound RDP sessions.
                        </p>
                    </div>
                </div>
            ) : (
                <>
                    <div className="rounded-lg border p-4 transition-colors"
                        style={{ background: rdpIdleEnabled ? 'var(--shield-bg-running)' : 'var(--shield-bg-idle)', borderColor: rdpIdleEnabled ? 'var(--accent-line)' : 'var(--color-border)' }}>
                            <div className="flex flex-col gap-4">
                                <div className="flex items-start justify-between gap-4">
                                    <div className="flex flex-col gap-1 min-w-0">
                                        <div className="flex items-center gap-2 flex-wrap">
                                            <div className={`size-2 rounded-full flex-shrink-0 ${rdpIdleEnabled ? 'bg-[var(--color-success)]' : 'bg-[var(--color-text-muted)]'}`} />
                                            <span className="text-sm font-semibold text-[var(--shield-text-primary)]">
                                                RDP Outgoing
                                            </span>
                                            {rdpIdleEnabled && hasPaid && !rdpIdleLocked && (
                                                <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0">Monitoring</span>
                                            )}
                                            {rdpIdleLocked && (
                                                <span className="text-[9px] px-2 py-0.5 rounded flex-shrink-0 flex items-center gap-1"
                                                    style={{ background: 'rgba(148,163,184,0.12)', color: 'var(--color-text-muted)', border: '1px solid var(--color-border)', fontFamily: 'var(--font-mono)', fontWeight: 700, letterSpacing: 1 }}>
                                                    <Icon icon="lock" size={9} />MANAGED BY ORG
                                                </span>
                                            )}
                                            {!hasPaid && !rdpIdleLocked && (
                                                <span onClick={() => window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Idle Session Monitor" } }))}
                                                    className="text-[9px] px-2 py-0.5 rounded flex-shrink-0 cursor-pointer"
                                                    style={{ background: 'var(--color-accent-subtle)', color: 'var(--color-accent)', border: '1px solid var(--color-border-accent)', fontFamily: 'var(--font-mono)', fontWeight: 700, letterSpacing: 1 }}
                                                    title="Click to unlock with WinCommander Pro">
                                                    PRO
                                                </span>
                                            )}
                                        </div>
                                        <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[280px]">
                                            {isAdvanced ? "Closes outgoing RDP client sessions when you have been idle beyond the threshold." : "Automatically closes remote desktop sessions if you step away too long."}
                                        </p>
                                    </div>
                                    <Switch checked={hasPaid && rdpIdleEnabled} disabled={rdpIdleLocked} aria-label="Close outgoing RDP sessions after idle timeout" onChange={(e) => {
                                        if (!hasPaid) { window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Idle Session Monitor" } })); return; }
                                        const next = e.currentTarget.checked;
                                        console.log("[RdpIdle] Toggle requested:", next);
                                        patchAppSettings({ ideal: { privacy: { tracking: { rdpIdleDisconnectEnabled: next } } } } as any)
                                            .then(() => console.log("[RdpIdle] Toggle saved:", next))
                                            .catch((err) => console.error("[RdpIdle] Toggle save failed:", err));
                                    }} />
                                </div>

                                {hasPaid && rdpIdleEnabled && (
                                    <div className="rounded-md border border-[var(--shield-inner-border)] bg-[var(--shield-inner-bg)] p-4 flex flex-col gap-4">
                                        <div className="flex items-center justify-between gap-3">
                                            <span className="text-xs font-mono text-[var(--shield-text-subtle)]">{isAdvanced ? "Idle timeout" : "Disconnect after"}</span>
                                            <HTMLSelect
                                                aria-label="Outgoing RDP idle timeout"
                                                value={timeoutPreset}
                                                onChange={(e) => {
                                                    const v = Number(e.currentTarget.value);
                                                    setTimeoutPreset(v);
                                                    if (v === -1) { setRdpCustomTime(String(rdpIdleTimeout)); }
                                                    else { patchRdpTracking({ rdpIdleDisconnectTimeout: v }); }
                                                }}
                                                options={RDP_TIMEOUT_PRESETS.map(p => ({ label: p.label, value: p.value }))}
                                                minimal style={{ fontSize: 12 }}
                                            />
                                        </div>
                                        {timeoutPreset === -1 && (
                                            <div className="flex items-center gap-2">
                                                <input type="number" min={10} max={86400} value={rdpCustomTime}
                                                    aria-label="Custom outgoing RDP idle timeout in seconds"
                                                    onChange={e => setRdpCustomTime(e.target.value)}
                                                    onKeyDown={e => {
                                                        if (e.key === 'Enter') {
                                                            const v = Math.max(3, Math.min(86400, Number(rdpCustomTime) || 120));
                                                            setRdpCustomTime(String(v)); patchRdpTracking({ rdpIdleDisconnectTimeout: v });
                                                            (e.target as HTMLInputElement).blur();
                                                        }
                                                    }}
                                                    onBlur={() => {
                                                        const v = Math.max(3, Math.min(86400, Number(rdpCustomTime) || 120));
                                                        setRdpCustomTime(String(v)); patchRdpTracking({ rdpIdleDisconnectTimeout: v });
                                                    }}
                                                    className="w-20 text-xs font-mono rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-primary)] text-[var(--color-text-primary)] px-2 py-1 text-center" />
                                                <span className="text-xs text-[var(--shield-text-muted)]">seconds</span>
                                            </div>
                                        )}
                                        <div className="flex items-center justify-between gap-3">
                                            <span className="text-xs font-mono text-[var(--shield-text-subtle)]">{isAdvanced ? "Warning countdown" : "Warn before close"}</span>
                                            <HTMLSelect
                                                aria-label="Outgoing RDP warning countdown"
                                                value={warningPreset}
                                                onChange={(e) => {
                                                    const v = Number(e.currentTarget.value);
                                                    setWarningPreset(v);
                                                    if (v === -1) { setRdpCustomWarningTime(String(rdpIdleWarningSeconds)); }
                                                    else { patchRdpTracking({ rdpIdleWarningSeconds: v }); }
                                                }}
                                                options={RDP_WARNING_PRESETS.map(p => ({ label: p.label, value: p.value }))}
                                                minimal style={{ fontSize: 12 }}
                                            />
                                        </div>
                                        {warningPreset === -1 && (
                                            <div className="flex items-center gap-2">
                                                <input type="number" min={5} max={3600} value={rdpCustomWarningTime}
                                                    aria-label="Custom outgoing RDP warning countdown in seconds"
                                                    onChange={e => setRdpCustomWarningTime(e.target.value)}
                                                    onKeyDown={e => {
                                                        if (e.key === 'Enter') {
                                                            const v = Math.max(5, Math.min(3600, Number(rdpCustomWarningTime) || 5));
                                                            setRdpCustomWarningTime(String(v)); patchRdpTracking({ rdpIdleWarningSeconds: v });
                                                            (e.target as HTMLInputElement).blur();
                                                        }
                                                    }}
                                                    onBlur={() => {
                                                        const v = Math.max(5, Math.min(3600, Number(rdpCustomWarningTime) || 5));
                                                        setRdpCustomWarningTime(String(v)); patchRdpTracking({ rdpIdleWarningSeconds: v });
                                                    }}
                                                    className="w-20 text-xs font-mono rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-primary)] text-[var(--color-text-primary)] px-2 py-1 text-center" />
                                                <span className="text-xs text-[var(--shield-text-muted)]">seconds</span>
                                            </div>
                                        )}
                                        <div className="pt-3 border-t border-[var(--shield-inner-border)] flex flex-col gap-2">
                                            <div className="text-[10px] font-mono uppercase tracking-widest text-[var(--shield-text-muted)] mb-1">On disconnect</div>
                                            <div className="grid grid-cols-2 gap-2">
                                                <div onClick={() => !rdpClearCacheLocked && patchRdpTracking({ rdpClearCacheOnDisconnect: !rdpClearCache })} className={`flex items-center gap-2.5 px-3 py-2 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] transition-colors ${rdpClearCacheLocked ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer hover:border-[var(--color-accent)]/40'}`} style={{ userSelect: 'none' }}>
                                                    <CheckboxControl checked={rdpClearCache} disabled={rdpClearCacheLocked} ariaLabel="Clear RDP history and cache" onChange={e => patchRdpTracking({ rdpClearCacheOnDisconnect: e.currentTarget.checked })} onClick={event => event.stopPropagation()} />
                                                    <span className="text-[11px] font-mono text-[var(--shield-text-subtle)] leading-tight">Clear RDP history & cache</span>
                                                    {rdpClearCacheLocked && <Icon icon="lock" size={10} className="ml-auto flex-shrink-0 text-[var(--color-text-muted)]" />}
                                                </div>
                                                <div onClick={() => !rdpRemoveCredsLocked && patchRdpTracking({ rdpRemoveCredsOnDisconnect: !rdpRemoveCreds })} className={`flex items-center gap-2.5 px-3 py-2 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] transition-colors ${rdpRemoveCredsLocked ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer hover:border-[var(--color-accent)]/40'}`} style={{ userSelect: 'none' }}>
                                                    <CheckboxControl checked={rdpRemoveCreds} disabled={rdpRemoveCredsLocked} ariaLabel="Remove saved credentials" onChange={e => patchRdpTracking({ rdpRemoveCredsOnDisconnect: e.currentTarget.checked })} onClick={event => event.stopPropagation()} />
                                                    <span className="text-[11px] font-mono text-[var(--shield-text-subtle)] leading-tight">Remove saved credentials</span>
                                                    {rdpRemoveCredsLocked && <Icon icon="lock" size={10} className="ml-auto flex-shrink-0 text-[var(--color-text-muted)]" />}
                                                </div>
                                                <div onClick={() => !rdpDismountLocked && patchRdpDismountVaults(!rdpDismountDraft)} className={`flex items-center gap-2.5 px-3 py-2 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] transition-colors ${rdpDismountLocked ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer hover:border-[var(--color-accent)]/40'}`} style={{ userSelect: 'none' }}>
                                                    <CheckboxControl checked={rdpDismountDraft} disabled={rdpDismountLocked} ariaLabel="Dismount server vaults" onChange={e => patchRdpDismountVaults(e.currentTarget.checked)} onClick={event => event.stopPropagation()} />
                                                    <span className="text-[11px] font-mono text-[var(--shield-text-subtle)] leading-tight">Dismount server vaults</span>
                                                    {rdpDismountLocked && <Icon icon="lock" size={10} className="ml-auto flex-shrink-0 text-[var(--color-text-muted)]" />}
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                )}
                            </div>
                        </div>
                    {/* ── Server-side incoming session sign-out ── */}
                    <div className="rounded-lg border p-4 transition-colors mt-3"
                        style={{ background: (hasPaid && rdpIncomingEnabled) ? 'var(--shield-bg-running)' : 'var(--shield-bg-idle)', borderColor: (hasPaid && rdpIncomingEnabled) ? 'var(--accent-line)' : 'var(--color-border)' }}>
                        <div className="flex flex-col gap-4">
                            <div className="flex items-start justify-between gap-4">
                                <div className="flex flex-col gap-1 min-w-0">
                                    <div className="flex items-center gap-2 flex-wrap">
                                        <div className={`size-2 rounded-full flex-shrink-0 ${(hasPaid && rdpIncomingEnabled) ? 'bg-[var(--color-success)]' : 'bg-[var(--color-text-muted)]'}`} />
                                        <span className="text-sm font-semibold text-[var(--shield-text-primary)]">
                                            RDP Incoming
                                        </span>
                                        {hasPaid && rdpIncomingEnabled && (
                                            <span className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-success)]/15 text-[var(--color-success)] border border-[var(--color-success)]/30 flex-shrink-0">Active</span>
                                        )}
                                        {!hasPaid && (
                                            <span onClick={() => window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Incoming Idle Sign-Out" } }))}
                                                className="text-[9px] px-2 py-0.5 rounded flex-shrink-0 cursor-pointer"
                                                style={{ background: 'var(--color-accent-subtle)', color: 'var(--color-accent)', border: '1px solid var(--color-border-accent)', fontFamily: 'var(--font-mono)', fontWeight: 700, letterSpacing: 1 }}
                                                title="Click to unlock with WinCommander Pro">
                                                PRO
                                            </span>
                                        )}
                                    </div>
                                    <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[300px]">
                                        {isAdvanced
                                            ? "Server-enforced Group Policy signs out (not just disconnects) idle inbound RDP sessions via HKLM Terminal Services policy."
                                            : "Automatically signs out anyone connected to this machine if they go idle — enforced server-side, requires no client software."}
                                    </p>
                                </div>
                                <div className="flex items-center gap-2 flex-shrink-0">
                                    {applyingIncoming && <Spinner size={14} />}
                                    <Switch
                                        aria-label="Sign out idle incoming RDP sessions"
                                        checked={hasPaid && rdpIncomingEnabled}
                                        disabled={applyingIncoming}
                                        onChange={(e) => {
                                            if (!hasPaid) { window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy", featureLabel: "Incoming Idle Sign-Out" } })); return; }
                                            if (e.currentTarget.checked) enableIncomingWithConflictCheck();
                                            else void applyIncoming(false, rdpIncomingSeconds);
                                        }}
                                    />
                                </div>
                            </div>

                            {hasPaid && rdpIncomingEnabled && (
                                <div className="rounded-md border border-[var(--shield-inner-border)] bg-[var(--shield-inner-bg)] p-4 flex flex-col gap-3">
                                    <div className="flex items-center justify-between gap-3">
                                        <span className="text-xs font-mono text-[var(--shield-text-subtle)]">Sign out after idle</span>
                                        <HTMLSelect
                                            value={incomingCustomMode ? -1 : rdpIncomingSeconds}
                                            disabled={applyingIncoming}
                                            onChange={(e) => {
                                                const v = Number(e.currentTarget.value);
                                                if (v === -1) { setIncomingCustomMode(true); setIncomingCustomSecs(String(rdpIncomingSeconds)); }
                                                else { setIncomingCustomMode(false); void applyIncoming(true, v); }
                                            }}
                                            options={RDP_INCOMING_PRESETS.map(p => ({ label: p.label, value: p.value }))}
                                            minimal style={{ fontSize: 12 }}
                                        />
                                    </div>
                                    {incomingCustomMode && (
                                        <div className="flex items-center gap-2">
                                            <input
                                                type="number" min={60} max={86400} value={incomingCustomSecs}
                                                onChange={e => setIncomingCustomSecs(e.target.value)}
                                                onBlur={() => {
                                                    const v = Math.max(60, Math.min(86400, Number(incomingCustomSecs) || 900));
                                                    setIncomingCustomSecs(String(v));
                                                    void applyIncoming(true, v);
                                                }}
                                                className="w-20 text-xs font-mono rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-primary)] text-[var(--color-text-primary)] px-2 py-1 text-center"
                                            />
                                            <span className="text-xs text-[var(--shield-text-muted)]">seconds</span>
                                        </div>
                                    )}
                                    <div className="pt-2 border-t border-[var(--shield-inner-border)]">
                                        <div className="grid grid-cols-2 gap-2">
                                            <div onClick={() => patchRdpIncomingDismount(!incomingDismountDraft)} className="flex cursor-pointer items-center gap-2.5 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] px-3 py-2 transition-colors hover:border-[var(--color-accent)]/40" style={{ userSelect: 'none' }}>
                                                <CheckboxControl checked={incomingDismountDraft} ariaLabel="Dismount local vaults when all sessions end" onChange={e => patchRdpIncomingDismount(e.currentTarget.checked)} onClick={event => event.stopPropagation()} />
                                                <span className="text-[11px] font-mono text-[var(--shield-text-subtle)] leading-tight">Dismount local vaults when all sessions end</span>
                                            </div>
                                            <div onClick={() => patchRdpIncomingSignOffOnDisconnect(!incomingSignOffDraft)} className="flex cursor-pointer items-center gap-2.5 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] px-3 py-2 transition-colors hover:border-[var(--color-accent)]/40" style={{ userSelect: 'none' }}>
                                                <CheckboxControl checked={incomingSignOffDraft} ariaLabel="Sign off when RDP window is closed" onChange={e => patchRdpIncomingSignOffOnDisconnect(e.currentTarget.checked)} onClick={event => event.stopPropagation()} />
                                                <span className="text-[11px] font-mono text-[var(--shield-text-subtle)] leading-tight">Sign off when RDP window is closed</span>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            )}

                            {incomingError && (
                                <div className="flex items-center gap-2 text-[var(--color-danger)] text-xs">
                                    <Icon icon="warning-sign" size={12} />
                                    <span>{incomingError}</span>
                                </div>
                            )}
                        </div>
                    </div>

                </>
            )}
            {pendingIncomingConflict && (
                <ConflictToggleDialog
                    isOpen={true}
                    toggleLabel="Incoming Idle Sign-Out"
                    conflictingLabels={["No Timeouts"]}
                    onCancel={() => setPendingIncomingConflict(false)}
                    onConfirm={() => { void confirmIncomingConflict(); }}
                />
            )}
        </SectionCard>
    );
}
