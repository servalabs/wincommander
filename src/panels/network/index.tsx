import { Icon, Dialog, FormGroup, InputGroup, Button, Classes, HTMLSelect, type IconName } from "@/components/ui/bp";
import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import useBackend, { BlocklistStatus, type MacRandomizerMode, type PhysicalNetworkAdapter } from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import useVisibility from "../../hooks/useVisibility";
import useEntitlements from "../../hooks/useEntitlements";
import SectionCard from "../../components/shared/SectionCard";
import UniversalCallout from "../../components/shared/UniversalCallout";
import UniversalToggle from "../../components/shared/UniversalToggle";
import ActivePorts from "../../components/network/ActivePorts";
import WCSwitch from "../../components/shared/WCSwitch";
import PortGuardSection from "./PortGuardSection";
import WifiGuardSection from "./WifiGuardSection";
import { NetworkMaintenanceTools } from "./NetworkMaintenanceTools";
import { showSuccess, showError } from "../../utils/toast";
import { BLOCKLISTS, blocklistBackendId } from "../../registry/features";
import { DNS_CATEGORIES, DNS_CATEGORY_DEFAULT_IDS, buildControldSlug, parseControldSlug } from "../../registry/dnsCategories";
import { blocklistLogos, companyLogos, software } from "@/assets";
import PanelHeader from "../../components/shared/PanelHeader";
import './index.css';

// Free tier is capped at this many ControlD Simple-Firewall categories.
const FREE_CONTROLD_LIMIT = 3;

// ─── Auto-apply debounce (5 s) ───────────────────────────────────────────────

/** Debounces an async apply action by `delayMs` (default 5 000 ms).
 *  Returns { schedule, status } where schedule(fn) resets the countdown
 *  each call, and status is one of: 'idle' | 'pending' | 'applying' | 'applied'. */
function useDebounceApply(delayMs = 5000) {
    const [status, setStatus] = useState<'idle' | 'pending' | 'applying' | 'applied'>('idle');
    const [secsLeft, setSecsLeft] = useState(0);
    const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const tickRef = useRef<ReturnType<typeof setInterval> | null>(null);
    // KT: resetRef must be tracked so it can be cancelled on unmount — bare setTimeout would fire setState after unmount
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
            } finally {
                setStatus('applied');
                resetRef.current = setTimeout(() => setStatus('idle'), 2000);
            }
        }, delayMs);
    }, [delayMs, clearTimers]);

    // Cancels any pending/in-flight-scheduled apply without running it, and
    // resets status back to idle. Used for cross-cancellation between the
    // independent DNS-affecting debounces (main toggle / category set /
    // advanced provider) so the last user action wins instead of a stale
    // timer reviving or overriding it later.
    const cancel = useCallback(() => {
        fnRef.current = null;
        clearTimers();
        setStatus('idle');
        setSecsLeft(0);
    }, [clearTimers]);

    // Cleanup on unmount
    useEffect(() => () => clearTimers(), [clearTimers]);

    return { schedule, cancel, status, secsLeft };
}

type AdapterMode = "off" | MacRandomizerMode;

function macDisplay(mac: string | null | undefined): string {
    if (!mac) return "—";
    const m = mac.replace(/[:\-]/g, "").toUpperCase();
    return m.match(/.{1,2}/g)?.join(":") ?? mac;
}

function deriveAdapterMode(adapter: PhysicalNetworkAdapter): AdapterMode {
    return adapter.isSpoofed ? "static-random" : "off";
}

// Blocklist logos resolve through the shared-asset manifest (assets/…).
// The inversion/height lookups below key on the SAME resolved URLs the <img>
// renders, so the equality checks still match after Vite fingerprints them.
const DNS_CATEGORY_LOGOS: Record<string, string[]> = {
    'ads':      [blocklistLogos['ads-logo-1.webp'], blocklistLogos['tracker-logo.webp']],
    'porn':     [blocklistLogos['onlyfans-logo.webp'], blocklistLogos['pornhub-logo.png']],
    'dating':   [blocklistLogos['tinder-app-logo.png'], blocklistLogos['bumble-app-logo.webp']],
    // drugs-logo.webp is a busy illustration that turns illegible at card
    // size — use the fallback category Icon instead (see dnsCategories.ts).
    'drugs':    [],
    'gambling': [blocklistLogos['bet365-logo.webp'], blocklistLogos['parimatch-logo.webp'], blocklistLogos['stake-logo.png'], blocklistLogos['1xbet-logo.webp']],
    'gov':      [],
    'malware':  [],
    'phishing': [blocklistLogos['phishing-logo-1.webp']],
    'social':   [blocklistLogos['telegram-logo.webp'], blocklistLogos['instagram-logo.webp'], blocklistLogos['snapchat-logo.webp']],
};

const DNS_LOGOS_NEEDING_INVERSION: string[] = [blocklistLogos['stake-logo.png']];

// Per-logo height overrides (px) — applied as inline style on the img
const DNS_LOGO_HEIGHTS: Record<string, number> = {
    [blocklistLogos['tinder-app-logo.png']]: 36,
    [blocklistLogos['parimatch-logo.webp']]:   22,
};

const BRAND_LOGOS: Record<string, string[]> = {
    'adobe': [companyLogos['adobe-logo.png'], software['photoshop.png'], software['illustrator.png'], software['premiere-pro.png']],
    'autodesk': [software['autodesk.png'], software['autocad.png'], software['3ds-max.png']],
    'corel': [software['coreldraw.png']],
    'glasswire': [software['glasswire.png']],
    'lightburn': [software['lightburn.png']],
    'piracy-torrent': [software['piratebay.png']],
    'ai-sites': [companyLogos['openai-logo.svg'], companyLogos['meta-logo.png'], companyLogos['google-logo.svg']],
    'telemetry-blocklist': [companyLogos['microsoft-logo.svg'], companyLogos['nvidia-logo.svg'], companyLogos['google-logo.svg']],
    'cloud-upload': [companyLogos['google-logo.svg'], companyLogos['microsoft-logo.svg']],
};

const LOGOS_NEEDING_INVERSION = [
    companyLogos['openai-logo.svg'],
    software['autodesk.png'],
    software['piratebay.png'],
];

interface BlocklistItemProps {
    name: string;
    description: string;
    entryCount: number;
    isApplied: boolean;
    onToggle: (enabled: boolean) => void;
    loading?: boolean;
    disabled?: boolean;
    logos?: string[];
}

function BlocklistItem({ name, description, entryCount, isApplied, onToggle, loading, disabled, logos }: BlocklistItemProps) {
    if (logos && logos.length > 0) {
        return (
            <div
                className={`blocklist-brand-card ${isApplied ? 'bbc-applied' : ''} ${loading ? 'bbc-loading' : ''}`}
                style={{ opacity: loading ? 0.7 : 1, pointerEvents: loading ? 'none' : 'auto' }}
            >
                <div className="bbc-top">
                    <div className="bbc-logos">
                        {logos.map((src, i) => (
                            <img
                                key={i}
                                src={src}
                                alt=""
                                className={`bbc-logo ${LOGOS_NEEDING_INVERSION.includes(src) ? 'invert-in-dark' : ''}`}
                            />
                        ))}
                    </div>
                    <span className="bbc-count">{entryCount.toLocaleString()} domains</span>
                </div>

                <div className="bbc-content" onClick={(e) => {
                    if (!(e.target as HTMLElement).closest('[role="switch"], .bp5-switch, .bp6-switch')) {
                        if (!disabled && !loading) onToggle(!isApplied);
                    }
                }}>
                    <div className="bbc-text">
                        <div className="bbc-title">{name === 'telemetry-blocklist' ? 'telemetry' : name.replace(/-/g, ' ')}</div>
                        <div className="bbc-desc">{description}</div>
                    </div>
                    <div className="bbc-action">
                        {loading && <div className="loader-spinner small" style={{ marginRight: 8 }} />}
                        <div className={`wc-custom-switch ${isApplied ? 'is-on' : 'is-off'}`}>
                            <div className="wc-switch-knob">
                                {isApplied && <Icon icon="tick" size={10} color="var(--color-success)" />}
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        );
    }

    return (
        <UniversalToggle
            label={`${name} (${entryCount.toLocaleString()} ENTRIES)`}
            description={description}
            checked={isApplied}
            onChange={onToggle}
            loading={loading}
            disabled={disabled}
            severity={isApplied ? "success" : "none"}
        />
    );
}

function DnsCategoryCard({
    id, label, description, icon, active, isAtLimit, onToggle,
}: {
    id: string; label: string; description: string; icon: IconName;
    active: boolean; isAtLimit: boolean; onToggle: () => void;
}) {
    const logos = DNS_CATEGORY_LOGOS[id];
    return (
        <div
            className={`blocklist-brand-card${active ? ' bbc-applied' : ''}`}
            style={{ cursor: isAtLimit ? 'not-allowed' : 'pointer', opacity: isAtLimit ? 0.45 : 1 }}
            role="button"
            tabIndex={isAtLimit ? -1 : 0}
            aria-pressed={active}
            onKeyDown={(e) => {
                if (!isAtLimit && (e.key === 'Enter' || e.key === ' ')) {
                    e.preventDefault();
                    onToggle();
                }
            }}
        >
            <div className="bbc-top">
                <div className="bbc-logos dns-bbc-logos">
                    {id === 'gov' ? (
                        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 15, fontWeight: 900, letterSpacing: 3, color: 'var(--color-accent)', lineHeight: 1 }}>GOV.</span>
                    ) : logos && logos.length > 0 ? logos.map((src, i) => (
                        <img
                            key={i}
                            src={src}
                            alt=""
                            className={`bbc-logo dns-bbc-logo${id === 'social' ? ' dns-bbc-logo-social' : ''}${DNS_LOGOS_NEEDING_INVERSION.includes(src) ? ' invert-in-dark' : ''}`}
                            style={DNS_LOGO_HEIGHTS[src] ? { height: DNS_LOGO_HEIGHTS[src] } : undefined}
                        />
                    )) : (
                        <span style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', width: 36, height: 36, borderRadius: 8, background: 'var(--color-bg-tertiary)' }}>
                            <Icon icon={icon} size={20} />
                        </span>
                    )}
                </div>
                {isAtLimit && (
                    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 3, fontFamily: 'var(--font-mono)', fontSize: 8, fontWeight: 700, color: 'var(--color-text-muted)' }}>
                        <Icon icon="lock" size={9} /> PRO
                    </span>
                )}
            </div>
            <div className="bbc-content" onClick={() => { if (!isAtLimit) onToggle(); }}>
                <div className="bbc-text">
                    <div className="bbc-title">{label}</div>
                    <div className="bbc-desc">{description}</div>
                </div>
                <div className="bbc-action">
                    <div className={`wc-custom-switch ${active ? 'is-on' : 'is-off'}`}>
                        <div className="wc-switch-knob">
                            {active && <Icon icon="tick" size={10} color="var(--color-success)" />}
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}

function resolveDnsProvider(provider: string | null): string {
    if (!provider) return "custom";
    const map: Record<string, string> = {
        Cloudflare_Malware_Adult: "cloudflare-malware-adult",
        AdGuard_Ads_Trackers: "adguard",
        Swiss_Firewall: "swiss-firewall",
        ControlD: "controld",
    };
    return map[provider] || "custom";
}

// ─── Adapters List (MAC per-adapter) ─────────────────────────────────────────

interface AdaptersListProps {
    adapters: PhysicalNetworkAdapter[] | null;
    adaptersLoading: boolean;
    adapterBusyId: string | null;
    modeOverrides: Record<string, AdapterMode>;
    showInactiveAdapters: boolean;
    onModeChange: (adapter: PhysicalNetworkAdapter, mode: AdapterMode) => void;
    onShowInactiveToggle: (show: boolean) => void;
    /** When true, render contents without an outer SectionCard so it can sit
     *  inside another card alongside another sub-section. */
    embedded?: boolean;
}

function AdaptersList({
    adapters,
    adaptersLoading,
    adapterBusyId,
    modeOverrides,
    showInactiveAdapters,
    onModeChange,
    onShowInactiveToggle,
    embedded = false,
}: AdaptersListProps) {
    const activeAdapters = useMemo(() => (adapters ?? []).filter(a => a.status === "Up"), [adapters]);
    const inactiveAdapters = useMemo(() => (adapters ?? []).filter(a => a.status !== "Up"), [adapters]);

    const body = (
        <>
            <div className="adapter-section-divider">
                <span>ADAPTERS</span>
            </div>

            <div className="adapter-list">
                {adaptersLoading && !adapters && (
                    <div className="font-mono text-[10px] text-[var(--color-text-muted)] py-2">Detecting adapters…</div>
                )}
                {adapters && activeAdapters.length === 0 && (
                    <div className="font-mono text-[10px] text-[var(--color-text-muted)] py-2">No active physical adapters detected.</div>
                )}

                {activeAdapters.map(a => {
                    const liveMode = deriveAdapterMode(a);
                    const selectedMode = modeOverrides[a.id] ?? liveMode;
                    const isBusy = adapterBusyId === a.id;
                    const kindIcon = a.kind === 'wifi' ? 'cell-tower' : 'globe-network';

                    return (
                        <div key={a.id} className={`adapter-row adapter-row--active${isBusy ? ' adapter-row--busy' : ''}`}>
                            <Icon icon={kindIcon as any} size={16} className="text-[var(--color-text-muted)] flex-shrink-0" />
                            <div className="adapter-row__identity">
                                <div className="flex items-center gap-2">
                                    <span className="font-mono text-[12px] font-bold text-[var(--color-text-primary)]">{a.name}</span>
                                    <span className="adapter-status-chip adapter-status-chip--up">UP</span>
                                    {a.linkSpeedMbps && (
                                        <span className="font-mono text-[9px] text-[var(--color-text-muted)]">{a.linkSpeedMbps}</span>
                                    )}
                                </div>
                                <div className="flex items-center gap-2">
                                    <span className="font-mono text-[10px] text-[var(--color-text-secondary)]">{macDisplay(a.currentMac)}</span>
                                    {a.isSpoofed && (
                                        <span className="font-mono text-[9px] font-bold text-[var(--color-accent)] tracking-wider">SPOOFED</span>
                                    )}
                                </div>
                            </div>

                            <div className="adapter-row__mac">
                                <HTMLSelect
                                    value={selectedMode}
                                    onChange={e => onModeChange(a, e.currentTarget.value as AdapterMode)}
                                    disabled={isBusy}
                                    options={[
                                        { value: "off", label: "Factory MAC" },
                                        { value: "static-random", label: "Random (static)" },
                                        { value: "rotate-on-launch", label: "Random (rotate)" },
                                    ]}
                                    minimal
                                />
                                {selectedMode !== "off" && (
                                    <Button
                                        small minimal icon="refresh"
                                        title="Re-roll random MAC"
                                        disabled={isBusy}
                                        onClick={() => onModeChange(a, selectedMode as MacRandomizerMode)}
                                    />
                                )}
                            </div>
                        </div>
                    );
                })}

                {inactiveAdapters.length > 0 && (
                    <>
                        <button
                            type="button"
                            className="hardware-process-toggle mt-1"
                            onClick={() => onShowInactiveToggle(!showInactiveAdapters)}
                        >
                            <span>{showInactiveAdapters ? "HIDE" : "SHOW"} {inactiveAdapters.length} INACTIVE</span>
                            <Icon icon={showInactiveAdapters ? "chevron-up" : "chevron-down"} size={11} />
                        </button>
                        {showInactiveAdapters && inactiveAdapters.map(a => (
                            <div key={a.id} className="adapter-row adapter-row--inactive">
                                <Icon icon={(a.kind === 'wifi' ? 'cell-tower' : 'globe-network') as any} size={14} />
                                <span className="font-mono text-[11px] text-[var(--color-text-muted)]">{a.name}</span>
                                <span className="adapter-status-chip adapter-status-chip--down">{a.status.toUpperCase()}</span>
                                <span className="font-mono text-[10px] text-[var(--color-text-muted)] ml-auto">{macDisplay(a.currentMac)}</span>
                            </div>
                        ))}
                    </>
                )}
            </div>
        </>
    );

    if (embedded) {
        return body;
    }

    return (
        <SectionCard title="Network Adapters" icon="globe-network">
            {body}
        </SectionCard>
    );
}

// ─── Auto-apply status pill ──────────────────────────────────────────────────

function AutoApplyPill({ status, secsLeft }: { status: string; secsLeft: number }) {
    if (status === 'idle') return null;
    return (
        <span className={`auto-apply-pill auto-apply-pill--${status}`}>
            {status === 'pending' && `Applying in ${secsLeft}s…`}
            {status === 'applying' && 'Applying…'}
            {status === 'applied' && 'Applied'}
        </span>
    );
}

// ─── DNS card ────────────────────────────────────────────────────────────────

function NetworkAdaptersCard({ embedded = false }: { embedded?: boolean } = {}) {
    const { networkDnsStatus, refreshNetwork, patchAppSettings, appSettings } = useAppState();
    const { canUse: canUseTier } = useEntitlements();
    const isPaid = canUseTier('paid');
    const {
        getDNSStatus,
        setSecureDNS: applySecureDNS,
        clearSecureDNS,
        disableDnsCensorshipProtection,
    } = useBackend();

    // DNS state
    const ignoreDnsStatusUpdate = useRef(false);
    const [secureDNS, setSecureDNS] = useState<boolean | null>(null);
    const [dnsProvider, setDnsProvider] = useState("adguard");
    const [previousProvider, setPreviousProvider] = useState("adguard");
    const [dnsBusy, setDnsBusy] = useState(false);
    const [swissDialogOpen, setSwissDialogOpen] = useState(false);
    const [swissId, setSwissId] = useState("");
    const [swissDevice, setSwissDevice] = useState("");
    const [swissPrimary, setSwissPrimary] = useState("");
    const [swissSecondary, setSwissSecondary] = useState("");
    const [controldFilters, setControldFilters] = useState<Set<string>>(new Set());
    const [dnsLoading, setDnsLoading] = useState(!networkDnsStatus);
    const [dohGuide, setDohGuide] = useState<{ provider: string } | null>(null);
    // Advanced DNS (AdGuard / Enterprise Firewall / custom) disclosure — closed
    // by default so ControlD's Simple Firewall categories are the primary flow.
    const [advancedOpen, setAdvancedOpen] = useState(false);

    const persistedControldSlug = appSettings?.ideal?.network?.dns?.controlDFilterSlug ?? null;
    useEffect(() => {
        setControldFilters(parseControldSlug(persistedControldSlug));
    }, [persistedControldSlug]);

    // Censorship protection now lives on the Settings panel (IdentityPanel),
    // which reads/writes the same ideal.network.dns.censorshipProtection
    // setting. Still read here (read-only) so turning DNS off can lift it —
    // see handleDnsToggle below, which must disable it server-side when the
    // last thing carrying it (encrypted DNS) goes away.
    const censorshipProtectionRef = useRef(false);
    useEffect(() => {
        censorshipProtectionRef.current = Boolean(appSettings?.ideal?.network?.dns?.censorshipProtection);
    }, [appSettings?.ideal?.network?.dns?.censorshipProtection]);

    useEffect(() => {
        if (ignoreDnsStatusUpdate.current) {
            ignoreDnsStatusUpdate.current = false;
            return;
        }
        if (networkDnsStatus) {
            const provider = resolveDnsProvider(networkDnsStatus.provider || null);
            setSecureDNS(provider !== "custom");
            setDnsProvider(provider === "custom" ? "custom" : provider);
            setPreviousProvider(provider === "custom" ? "adguard" : provider);
            if (networkDnsStatus.dohId) setSwissId(networkDnsStatus.dohId);
            if (networkDnsStatus.deviceName) setSwissDevice(networkDnsStatus.deviceName);
            if (networkDnsStatus.provider === "Swiss_Firewall" && networkDnsStatus.servers?.length) {
                setSwissPrimary(networkDnsStatus.servers[0] || "");
                setSwissSecondary(networkDnsStatus.servers[1] || "");
            }
            setDnsLoading(false);
        }
    }, [networkDnsStatus]);

    const loadDns = useCallback(async (silent = false) => {
        if (!silent) setDnsLoading(true);
        try {
            const dnsRes = await getDNSStatus();
            if (dnsRes.success && dnsRes.data) {
                const provider = resolveDnsProvider(dnsRes.data.provider || null);
                setSecureDNS(provider !== "custom");
                setDnsProvider(provider === "custom" ? "custom" : provider);
                setPreviousProvider(provider === "custom" ? "adguard" : provider);
                if (dnsRes.data.dohId) setSwissId(dnsRes.data.dohId);
                if (dnsRes.data.deviceName) setSwissDevice(dnsRes.data.deviceName);
                if (dnsRes.data.provider === "Swiss_Firewall" && dnsRes.data.servers?.length) {
                    setSwissPrimary(dnsRes.data.servers[0] || "");
                    setSwissSecondary(dnsRes.data.servers[1] || "");
                }
            }
        } finally {
            if (!silent) setDnsLoading(false);
        }
    }, [getDNSStatus]);

    // eslint-disable-next-line react-hooks/exhaustive-deps
    useEffect(() => { if (!networkDnsStatus) void loadDns(); }, []);

    // Debounce auto-apply: the main DNS toggle and the Simple Firewall
    // categories each get a 5s debounce. Destructure schedule separately so
    // useCallback deps reference the stable fn.
    const dnsDebounce = useDebounceApply(5000);
    const { schedule: scheduleDns, cancel: cancelDns } = dnsDebounce;
    // Simple Firewall categories auto-apply 5s after the last toggle — same
    // debounce contract as DNS so the inline cards need no Apply button.
    const controldDebounce = useDebounceApply(5000);
    const { schedule: scheduleControld, cancel: cancelControld } = controldDebounce;

    // Optimistic-toggle state for the card row (updates immediately so the card
    // reads as on/off while the debounce countdown runs).
    const [optimisticDns, setOptimisticDns] = useState<boolean | null>(null);

    const effectiveDns = optimisticDns !== null ? optimisticDns : Boolean(secureDNS);

    // KT: refs let debounce callbacks read the latest values without re-creating
    // the debounced handler on every state change (which would reset the countdown).
    const dnsProviderRef = useRef(dnsProvider);
    useEffect(() => { dnsProviderRef.current = dnsProvider; }, [dnsProvider]);
    const controldFiltersRef = useRef(controldFilters);
    useEffect(() => { controldFiltersRef.current = controldFilters; }, [controldFilters]);

    // Apply a Simple Firewall (ControlD) filter set: activates the provider and
    // pushes the encrypted filter slug. Called by the debounced inline toggles
    // AND by the main toggle's auto-select-defaults path below.
    const applyControldSet = useCallback(async (nextSet: Set<string>) => {
        // Free tier is capped at FREE_CONTROLD_LIMIT categories. The per-category
        // toggle enforces this, but the main-toggle auto-default path applies the
        // full 7-category default set; clamp here (preserving DNS_CATEGORIES
        // order) so no path can push a Free user past the cap, then sync the
        // visible grid to what was actually applied.
        const effectiveSet = (!isPaid && nextSet.size > FREE_CONTROLD_LIMIT)
            ? new Set(DNS_CATEGORIES.filter(c => nextSet.has(c.id)).slice(0, FREE_CONTROLD_LIMIT).map(c => c.id))
            : nextSet;
        const slug = buildControldSlug(effectiveSet);
        setDnsProvider("controld");
        setDnsBusy(true);
        const response = await applySecureDNS("controld", {
            filterSlug: slug || undefined,
            silent: true,
        });
        setDnsBusy(false);
        if (response.success) {
            ignoreDnsStatusUpdate.current = true;
            setSecureDNS(true);
            setControldFilters(effectiveSet);
            refreshNetwork(true, true);
            patchAppSettings({
                ideal: { network: { dns: { provider: 'controld', controlDFilterSlug: slug || null } } },
            }).catch(() => { });
        }
    }, [applySecureDNS, refreshNetwork, patchAppSettings, isPaid]);

    const handleDnsToggle = useCallback(async (checked: boolean) => {
        if (secureDNS === null || dnsBusy) return;
        // Cancel any pending category-set apply first: the main toggle is a
        // more recent, more sweeping user action and must win. Without this,
        // an older pending category timer can fire after this one and either
        // revive DNS the user just turned off, or clobber the state this
        // toggle is about to set.
        cancelControld();
        // Optimistic update so the card reflects the new state immediately.
        setOptimisticDns(checked);
        // Optimistically reflect the default categories as active right away
        // (matches toggleControldCategory's immediate setControldFilters) so
        // the grid doesn't sit inactive for the full 5s debounce window while
        // the header already reads ON. Local UI state only — the actual
        // backend apply still runs inside the debounced callback below.
        let optimisticNextSet: Set<string> | null = null;
        if (checked && (dnsProviderRef.current === 'controld' || dnsProviderRef.current === 'custom' || !dnsProviderRef.current)) {
            optimisticNextSet = controldFiltersRef.current.size > 0
                ? controldFiltersRef.current
                : new Set(DNS_CATEGORY_DEFAULT_IDS);
            setControldFilters(optimisticNextSet);
        }
        scheduleDns(async () => {
            setOptimisticDns(null);
            if (!checked) {
                setDnsBusy(true);
                // Turning encrypted DNS off must also lift censorship protection:
                // otherwise the port-53 block would remain with no encrypted resolver
                // to carry DNS, breaking all name resolution.
                let censorshipDisabled = true;
                if (censorshipProtectionRef.current) {
                    const censorshipResponse = await disableDnsCensorshipProtection();
                    censorshipDisabled = censorshipResponse.success;
                    if (!censorshipDisabled) {
                        showError(censorshipResponse.error || "Failed to disable censorship protection");
                    }
                }
                const response = await clearSecureDNS();
                setDnsBusy(false);
                if (response.success) {
                    ignoreDnsStatusUpdate.current = true;
                    setSecureDNS(false);
                    setDnsProvider("custom");
                    setControldFilters(new Set());
                    refreshNetwork(true, true);
                    patchAppSettings({ ideal: { network: { dns: { provider: null, censorshipProtection: censorshipDisabled ? false : censorshipProtectionRef.current, controlDFilterSlug: null } } } }).catch(() => { });
                }
            } else if (dnsProviderRef.current === 'controld' || dnsProviderRef.current === 'custom' || !dnsProviderRef.current) {
                // ControlD is the primary flow: turning the main toggle on with no
                // categories picked yet auto-enables the sensible default set
                // (everything except gov + social — those are opt-in, not defaults).
                // nextSet was already computed and optimistically applied to
                // local state above, before the debounce window started.
                const nextSet = optimisticNextSet ?? (controldFiltersRef.current.size > 0
                    ? controldFiltersRef.current
                    : new Set(DNS_CATEGORY_DEFAULT_IDS));
                setControldFilters(nextSet);
                await applyControldSet(nextSet);
            } else {
                // An Advanced provider (AdGuard / Enterprise Firewall / custom) was
                // previously active — re-enable that same provider.
                setDnsBusy(true);
                const provider = dnsProviderRef.current;
                const response = await applySecureDNS(provider);
                setDnsBusy(false);
                if (response.success) {
                    ignoreDnsStatusUpdate.current = true;
                    setSecureDNS(true);
                    setDnsProvider(provider);
                    refreshNetwork(true, true);
                    patchAppSettings({ ideal: { network: { dns: { provider } } } }).catch(() => { });
                } else {
                    setOptimisticDns(false);
                    setSecureDNS(false);
                }
            }
        });
    }, [secureDNS, dnsBusy, disableDnsCensorshipProtection, clearSecureDNS, applySecureDNS,
        applyControldSet, refreshNetwork, patchAppSettings, scheduleDns, cancelControld]);

    const openDnsProGate = useCallback(() => {
        window.dispatchEvent(new CustomEvent("license-gate-open", {
            detail: { tab: "buy", featureLabel: "Simple Firewall Pro filtering" },
        }));
    }, []);

    // Turning off the last selected category auto-turns the main Encrypted
    // DNS toggle off too — keeps the UI/backend state coherent (never "DNS is
    // ON" with nothing actually applied).
    const clearDnsForEmptyCategories = useCallback(async () => {
        setDnsBusy(true);
        let censorshipDisabled = true;
        if (censorshipProtectionRef.current) {
            const censorshipResponse = await disableDnsCensorshipProtection();
            censorshipDisabled = censorshipResponse.success;
            if (!censorshipDisabled) {
                showError(censorshipResponse.error || "Failed to disable censorship protection");
            }
        }
        const response = await clearSecureDNS();
        setDnsBusy(false);
        if (response.success) {
            ignoreDnsStatusUpdate.current = true;
            setSecureDNS(false);
            setDnsProvider("custom");
            refreshNetwork(true, true);
            patchAppSettings({ ideal: { network: { dns: { provider: null, censorshipProtection: censorshipDisabled ? false : censorshipProtectionRef.current, controlDFilterSlug: null } } } }).catch(() => { });
        }
    }, [clearSecureDNS, disableDnsCensorshipProtection, refreshNetwork, patchAppSettings]);

    const toggleControldCategory = (catId: string) => {
        if (dnsBusy) return;
        const willAdd = !controldFilters.has(catId);
        if (willAdd && !isPaid && controldFilters.size >= FREE_CONTROLD_LIMIT) {
            openDnsProGate();
            return;
        }
        const next = new Set(controldFilters);
        if (next.has(catId)) next.delete(catId);
        else next.add(catId);
        setControldFilters(next);
        // Cancel any pending main-toggle apply first: a category edit is a
        // more recent user action and must win. Without this, an older
        // pending DNS-off/on timer can fire after this one and clobber the
        // category state this edit is about to apply.
        cancelDns();
        // Auto-apply 5s after the last toggle (no modal, no Apply button).
        // Turning off the last category clears DNS entirely instead of
        // applying an empty ControlD filter set.
        scheduleControld(() => next.size === 0 ? clearDnsForEmptyCategories() : applyControldSet(next));
    };

    const applySwissFirewall = async () => {
        setDnsProvider("swiss-firewall");
        setSwissDialogOpen(false);
        setDnsBusy(true);
        const response = await applySecureDNS("swiss-firewall", {
            dohId: swissId,
            deviceName: swissDevice,
            primary: swissPrimary,
            secondary: swissSecondary,
            silent: true,
        });
        setDnsBusy(false);
        if (response.success) {
            setSecureDNS(true);
            setDohGuide({ provider: 'swiss-firewall' });
            refreshNetwork(true, true);
            patchAppSettings({ ideal: { network: { dns: { provider: 'swiss-firewall' } } } }).catch(() => { });
        } else {
            setDnsProvider(previousProvider);
        }
    };

    const handleDnsRefresh = useCallback(async () => {
        setDnsLoading(true);
        try {
            await Promise.all([
                loadDns(true),
                refreshNetwork(true, true),
            ]);
        } finally {
            setDnsLoading(false);
        }
    }, [loadDns, refreshNetwork]);

    // Selecting AdGuard queues through the same 5s auto-apply lane as the main
    // DNS switch. Enterprise Firewall still uses its explicit dialog Apply.
    const handleSelectAdvancedProvider = useCallback((provider: string) => {
        if (dnsBusy) return;
        // Cancel any pending category-set apply first: this newer provider
        // selection must win after the shared DNS debounce fires.
        cancelDns();
        cancelControld();

        if (effectiveDns && dnsProvider === provider) {
            setOptimisticDns(false);
            scheduleDns(async () => {
                setOptimisticDns(null);
                setDnsBusy(true);
                let censorshipDisabled = true;
                if (censorshipProtectionRef.current) {
                    const censorshipResponse = await disableDnsCensorshipProtection();
                    censorshipDisabled = censorshipResponse.success;
                    if (!censorshipDisabled) {
                        showError(censorshipResponse.error || "Failed to disable censorship protection");
                    }
                }
                const response = await clearSecureDNS();
                setDnsBusy(false);
                if (response.success) {
                    ignoreDnsStatusUpdate.current = true;
                    setSecureDNS(false);
                    setDnsProvider("custom");
                    setControldFilters(new Set());
                    refreshNetwork(true, true);
                    patchAppSettings({
                        ideal: {
                            network: {
                                dns: {
                                    provider: null,
                                    censorshipProtection: censorshipDisabled ? false : censorshipProtectionRef.current,
                                    controlDFilterSlug: null,
                                },
                            },
                        },
                    }).catch(() => { });
                }
            });
            return;
        }

        setDnsProvider(provider);
        setOptimisticDns(true);
        setControldFilters(new Set());
        scheduleDns(async () => {
            setOptimisticDns(null);
            setDnsBusy(true);
            const response = await applySecureDNS(provider, { silent: true });
            setDnsBusy(false);
            if (response.success) {
                ignoreDnsStatusUpdate.current = true;
                setSecureDNS(true);
                refreshNetwork(true, true);
                patchAppSettings({
                    ideal: { network: { dns: { provider, controlDFilterSlug: null } } },
                }).catch(() => { });
            } else {
                setDnsProvider(previousProvider);
                setOptimisticDns(null);
            }
        });
    }, [dnsBusy, effectiveDns, dnsProvider, disableDnsCensorshipProtection, clearSecureDNS, applySecureDNS, previousProvider, refreshNetwork, patchAppSettings, scheduleDns, cancelDns, cancelControld]);

    // An Advanced provider is "active" only once it's actually applied
    // (dnsProvider reflects backend state) — not merely selected in the UI.
    const isAdvancedProviderActive = effectiveDns && (dnsProvider === 'adguard' || dnsProvider === 'swiss-firewall' || dnsProvider === 'cloudflare-malware-adult');

    const orderedDnsCategories = useMemo(() => {
        const specialCategoryRank = (id: string) => id === 'gov' ? 2 : id === 'social' ? 3 : 0;
        return [...DNS_CATEGORIES].sort((left, right) => {
            const specialOrder = specialCategoryRank(left.id) - specialCategoryRank(right.id);
            if (specialOrder !== 0) return specialOrder;
            return Number(controldFilters.has(right.id)) - Number(controldFilters.has(left.id));
        });
    }, [controldFilters]);
    const advancedProviderLabel = dnsProvider === 'adguard' ? 'AdGuard' : dnsProvider === 'swiss-firewall' ? 'Enterprise Firewall' : 'Custom DNS';

    const headerRight = (
                    <div className="dns-header-right">
                        {/* Plain ON/OFF switch — replaces the old "DoH OFF" pill
                            which read as jargon to non-technical users (owner
                            2026-06-11). Same wiring as the previous toggle card. */}
                        <AutoApplyPill status={dnsDebounce.status} secsLeft={dnsDebounce.secsLeft} />
                        <span className="dns-toggle-cluster" title={effectiveDns ? "Encrypted DNS is ON — click to disable" : "Encrypted DNS is OFF — click to enable"}>
                            <span className={`dns-toggle-label${effectiveDns ? ' dns-toggle-label--on' : ''}`}>
                                {secureDNS === null ? "…" : effectiveDns ? "ON" : "OFF"}
                            </span>
                            <WCSwitch
                                checked={effectiveDns}
                                onChange={(next) => handleDnsToggle(next)}
                                disabled={secureDNS === null || dnsBusy}
                                size="sm"
                                label="Encrypted DNS"
                            />
                        </span>
                        <button
                            type="button"
                            className="refresh-btn"
                            onClick={() => { void handleDnsRefresh(); }}
                            disabled={dnsLoading}
                            title="Re-scan"
                        >
                            <Icon icon="refresh" size={14} className={dnsLoading ? "spinning" : ""} />
                        </button>
                    </div>
    );

    const body = (
        <>
                {/* Provider picker — the old full-width Encrypted-DNS toggle card
                    is gone; the state lives in the header pill above. The picker
                    is always rendered so users can change providers without first
                    toggling DoH on (picking a provider also enables it). */}
                <div className="dns-content" style={{ position: 'relative' }}>
                    {dnsLoading && (
                        <div className="scanning-overlay">
                            <div className="loader-spinner" />
                        </div>
                    )}
                    <div style={{ opacity: dnsLoading ? 0.3 : 1, pointerEvents: dnsLoading ? 'none' : 'auto' }}>
                        {isAdvancedProviderActive && (
                            <UniversalCallout
                                message={`${advancedProviderLabel} is active as your DNS provider — Simple Firewall categories below are inactive until you switch back.`}
                                intent="warning"
                                className="mb-3"
                            />
                        )}
                        <p className="dns-blurb">
                            Encrypted DNS is applied per adapter.
                        </p>

                        {/* Simple Firewall categories — the primary DNS flow. Each
                            card toggles a category and auto-applies after a 5s
                            debounce (no Apply button). Turning the main Encrypted
                            DNS toggle on with nothing selected auto-picks the
                            sensible defaults; clearing every category auto-turns
                            the main toggle off. */}
                        <div className="simple-firewall-inline">
                            <div className="dns-provider-grid__header">
                                <h3 className="dns-provider-grid__label simple-firewall-title">SIMPLE FIREWALL — CATEGORIES</h3>
                                <div className="flex items-center gap-2">
                                    {!isPaid && (
                                        <span className={`font-mono text-[9px] ${controldFilters.size >= FREE_CONTROLD_LIMIT ? 'text-[var(--color-accent)]' : 'text-[var(--color-text-muted)]'}`}>
                                            {controldFilters.size >= FREE_CONTROLD_LIMIT
                                                ? `${controldFilters.size}/${FREE_CONTROLD_LIMIT} — Pro for more`
                                                : `${controldFilters.size}/${FREE_CONTROLD_LIMIT} free`}
                                        </span>
                                    )}
                                    <AutoApplyPill status={controldDebounce.status} secsLeft={controldDebounce.secsLeft} />
                                    <span className="dns-provider-grid__count">
                                        {controldFilters.size} of {DNS_CATEGORIES.length} active
                                    </span>
                                </div>
                            </div>
                            <span className="dns-provider-grid__hint">Choose categories to filter.</span>
                            <div className={`simple-firewall-grid${isAdvancedProviderActive ? ' simple-firewall-grid--inactive' : ''}`}>
                                {orderedDnsCategories.map(cat => {
                                    const active = !isAdvancedProviderActive && controldFilters.has(cat.id);
                                    const isAtLimit = !isAdvancedProviderActive && !isPaid && !active && controldFilters.size >= FREE_CONTROLD_LIMIT;
                                    return (
                                        <DnsCategoryCard
                                            key={cat.id}
                                            id={cat.id}
                                            label={cat.label}
                                            description={cat.description}
                                            icon={cat.icon}
                                            active={active}
                                            isAtLimit={isAtLimit}
                                            onToggle={() => toggleControldCategory(cat.id)}
                                        />
                                    );
                                })}
                            </div>
                        </div>

                        {/* Advanced DNS — AdGuard / Enterprise Firewall / custom
                            provider selection. Collapsed by default: this is an
                            alternative to the ControlD category flow above, not
                            a primary control. */}
                        <div className="advanced-dns-disclosure">
                            <button
                                type="button"
                                className="advanced-dns-disclosure__trigger"
                                onClick={() => setAdvancedOpen(v => !v)}
                                aria-expanded={advancedOpen}
                            >
                                <Icon icon={advancedOpen ? 'chevron-down' : 'chevron-right'} size={12} />
                                <span>Advanced DNS</span>
                                {isAdvancedProviderActive && (
                                    <span className="advanced-dns-disclosure__badge">{advancedProviderLabel} active</span>
                                )}
                            </button>
                            {advancedOpen && (
                                <div className="advanced-dns-disclosure__body">
                                    <span className="dns-provider-grid__hint">
                                        Use a different provider instead of Simple Firewall categories.
                                    </span>
                                    <div className="advanced-dns-options">
                                        <button
                                            type="button"
                                            className={`advanced-dns-option${dnsProvider === 'adguard' ? ' advanced-dns-option--active' : ''}`}
                                            disabled={dnsBusy}
                                            onClick={() => { void handleSelectAdvancedProvider('adguard'); }}
                                        >
                                            <Icon icon="shield" size={14} />
                                            <div className="advanced-dns-option__text">
                                                <span className="advanced-dns-option__title">AdGuard</span>
                                                <span className="advanced-dns-option__desc">Ads &amp; tracker blocking DNS</span>
                                            </div>
                                            {dnsProvider === 'adguard' && <Icon icon="tick" size={13} className="text-[var(--color-success)]" />}
                                        </button>
                                        <button
                                            type="button"
                                            className={`advanced-dns-option${dnsProvider === 'swiss-firewall' ? ' advanced-dns-option--active' : ''}`}
                                            disabled={dnsBusy}
                                            onClick={() => { setSwissDialogOpen(true); }}
                                        >
                                            <Icon icon="endorsed" size={14} />
                                            <div className="advanced-dns-option__text">
                                                <span className="advanced-dns-option__title">Enterprise Firewall</span>
                                                <span className="advanced-dns-option__desc">Per-device filtering via your organisation's account</span>
                                            </div>
                                            {dnsProvider === 'swiss-firewall' && <Icon icon="tick" size={13} className="text-[var(--color-success)]" />}
                                        </button>
                                    </div>
                                </div>
                            )}
                        </div>
                    </div>
                </div>

                {dohGuide && (
                    <div className="doh-guide">
                        <div className="doh-guide__header">
                            <div className="flex items-center gap-2">
                                <Icon icon="tick-circle" size={13} className="text-[var(--color-success)]" />
                                <span className="font-mono text-[10px] font-bold tracking-wider text-[var(--color-text-primary)] uppercase">
                                    Internet filter applied
                                </span>
                            </div>
                            <button className="doh-guide__close" onClick={() => setDohGuide(null)} aria-label="Dismiss">
                                <Icon icon="cross" size={11} />
                            </button>
                        </div>
                    </div>
                )}

                {/* WebRTCLeakInline moved out of this card — now sits inside the
                    PortGuard "Port Watch & Firewall" body so the Network row 1
                    section reads as a single "outbound surface" group and the
                    DNS card no longer overflows when the picker grid is wide. */}

        </>
    );


    return (
        <>
            {embedded ? (
                <>
                    <section className="firewall-subcard firewall-subcard--dns">
                        <header className="firewall-subcard__header">
                            <div className="firewall-subcard__title">
                                <Icon icon="cloud" size={13} />
                                <span>DNS</span>
                            </div>
                            {headerRight}
                        </header>
                        {body}
                    </section>
                </>
            ) : (
                <SectionCard
                    title="Encrypted DNS"
                    icon="cloud"
                    className="ncr-stretch-card"
                    headerRight={headerRight}
                >
                    {body}
                </SectionCard>
            )}

            {/* Enterprise Firewall config dialog */}
            <Dialog
                isOpen={swissDialogOpen}
                onClose={() => { setSwissDialogOpen(false); setDnsProvider(previousProvider); }}
                title="Enterprise Firewall"
                className={`wc-dialog swiss-dialog ${document.documentElement.classList.contains('light') ? '' : Classes.DARK}`}
                icon="endorsed"
                style={{ width: 520 }}
            >
                <div className="wc-dialog-body" style={{ padding: '24px 28px 20px' }}>
                    <p className="font-mono text-[10px] text-[var(--color-text-muted)] uppercase tracking-widest mb-5">
                        Per-device DNS filtering via your Enterprise Firewall account
                    </p>
                    <div className="flex flex-col gap-4">
                        <div className="grid grid-cols-2 gap-3">
                            <FormGroup label={<span className="font-mono text-[10px] tracking-wider text-[var(--color-text-secondary)]">CUSTOMER ID</span>} style={{ marginBottom: 0 }}>
                                <InputGroup
                                    leftIcon="id-number"
                                    placeholder="e.g. c39532"
                                    value={swissId}
                                    onChange={(e) => setSwissId(e.target.value.trim())}
                                    className="enterprise-input"
                                />
                            </FormGroup>
                            <FormGroup label={<span className="font-mono text-[10px] tracking-wider text-[var(--color-text-secondary)]">DEVICE NAME</span>} style={{ marginBottom: 0 }}>
                                <InputGroup
                                    leftIcon="desktop"
                                    placeholder="e.g. Work-Laptop"
                                    value={swissDevice}
                                    onChange={(e) => setSwissDevice(e.target.value)}
                                    className="enterprise-input"
                                />
                            </FormGroup>
                        </div>
                        <div className="grid grid-cols-2 gap-3">
                            <FormGroup label={<span className="font-mono text-[10px] tracking-wider text-[var(--color-text-secondary)]">PRIMARY DNS (IPv4)</span>} style={{ marginBottom: 0 }}>
                                <InputGroup
                                    leftIcon="ip-address"
                                    placeholder="45.90.28.0"
                                    value={swissPrimary}
                                    onChange={(e) => setSwissPrimary(e.target.value.trim())}
                                    className="enterprise-input"
                                />
                            </FormGroup>
                            <FormGroup label={<span className="font-mono text-[10px] tracking-wider text-[var(--color-text-secondary)]">SECONDARY DNS (IPv4)</span>} style={{ marginBottom: 0 }}>
                                <InputGroup
                                    leftIcon="ip-address"
                                    placeholder="45.90.30.0"
                                    value={swissSecondary}
                                    onChange={(e) => setSwissSecondary(e.target.value.trim())}
                                    className="enterprise-input"
                                />
                            </FormGroup>
                        </div>
                    </div>
                </div>
                <div className="wc-dialog-footer" style={{ padding: '16px 28px', borderTop: '1px solid var(--color-border)' }}>
                    <div className="flex justify-end items-center gap-3">
                        <Button
                            text="Cancel"
                            onClick={() => { setSwissDialogOpen(false); setDnsProvider(previousProvider); }}
                            minimal
                        />
                        <Button
                            intent="primary"
                            text={dnsBusy ? "Applying…" : "Apply Now"}
                            icon={dnsBusy ? undefined : "endorsed"}
                            loading={dnsBusy}
                            disabled={!swissId || !swissDevice || !swissPrimary || !swissSecondary || dnsBusy}
                            onClick={applySwissFirewall}
                        />
                    </div>
                </div>
            </Dialog>
        </>
    );
}

// ─── Hosts Protection (blocklists only) ──────────────────────────────────────

function NetworkSecurityControls() {
    const { networkBlocklistStatus, refreshNetwork } = useAppState();
    const visibility = useVisibility();
    const {
        getBlocklistStatus,
        addBlocklistToHosts,
        removeBlocklistFromHosts,
    } = useBackend();

    const [status, setStatus] = useState<BlocklistStatus | null>(networkBlocklistStatus);
    const [isScanning, setIsScanning] = useState(!networkBlocklistStatus);
    const [actionLoading, setActionLoading] = useState<string | null>(null);

    const blocklistInfo: Record<string, { description: string }> = Object.fromEntries(
        BLOCKLISTS.map((b) => [blocklistBackendId(b), { description: b.description }])
    );

    useEffect(() => {
        if (networkBlocklistStatus) {
            setStatus(networkBlocklistStatus);
            setIsScanning(false);
        }
    }, [networkBlocklistStatus]);

    // eslint-disable-next-line react-hooks/exhaustive-deps
    useEffect(() => { if (!networkBlocklistStatus) void loadStatus(); }, []);

    const loadStatus = useCallback(async (silent = false) => {
        if (!silent) setIsScanning(true);
        try {
            const result = await getBlocklistStatus();
            if (result.success && result.data) {
                const data = result.data;
                if (!Array.isArray(data.applied)) data.applied = [];
                setStatus(data);
            } else {
                showError(result.error || 'Failed to fetch blocklist status');
            }
        } catch {
            showError('An unexpected error occurred while scanning');
        } finally {
            if (!silent) setIsScanning(false);
        }
    }, [getBlocklistStatus]);

    const handleToggle = async (name: string, enabled: boolean) => {
        setActionLoading(name);
        try {
            const result = enabled
                ? await addBlocklistToHosts(name)
                : await removeBlocklistFromHosts(name);
            if (result.success) {
                showSuccess(`${name} blocklist ${enabled ? 'applied' : 'removed'} successfully`);
                await loadStatus(true);
                refreshNetwork(true, true);
            } else {
                showError(result.error || `Failed to ${enabled ? 'apply' : 'remove'} blocklist`);
            }
        } finally {
            setActionLoading(null);
        }
    };

    const group2Names = ['piracy-torrent', 'cloud-upload', 'ai-sites'];
    const group1 = status?.available?.filter(item => !group2Names.includes(item.name)) || [];
    const group2 = status?.available?.filter(item => group2Names.includes(item.name)) || [];

    const handleSelectAll = async (enabled: boolean) => {
        setActionLoading('all');
        try {
            for (const item of group1) {
                const isCurrentlyApplied = status?.applied?.includes(item.name);
                if (enabled && !isCurrentlyApplied) {
                    await addBlocklistToHosts(item.name);
                } else if (!enabled && isCurrentlyApplied) {
                    await removeBlocklistFromHosts(item.name);
                }
            }
            showSuccess(enabled ? 'Standard blocklists enabled' : 'Standard blocklists disabled');
            await loadStatus(true);
            refreshNetwork(true, true);
        } finally {
            setActionLoading(null);
        }
    };

    const allApplied = group1.length > 0 && group1.every(a => status?.applied?.includes(a.name));
    const anyLoading = actionLoading !== null;

    if (!visibility.isVisible({ capability: ["network"] })) return null;

    return (
        <SectionCard title="Firewall" icon="shield" className="firewall-card">
            {/* Tour anchor: covers both blocking layers — DNS-category
                filtering (NetworkAdaptersCard, embedded) and hosts-file
                blocklists (the Hosts Protection subcard below) — so one tour
                stop can explain them together. */}
            <div className="firewall-sections" data-tour="network-security-controls">
                <NetworkAdaptersCard embedded />

                {/* Host Protection detail — blocklist grids */}
                <section className="firewall-subcard firewall-subcard--hosts">
                    <header className="firewall-subcard__header">
                        <div className="firewall-subcard__title">
                            <Icon icon="shield" size={13} />
                            <span>Hosts Protection</span>
                        </div>
                        <div className="firewall-subcard__header-actions">
                            {group1.length > 0 && (
                                <div className="recommended-pill">
                                    <span className="recommended-pill__label">Recommended</span>
                                    <WCSwitch
                                        checked={allApplied}
                                        onChange={(next) => handleSelectAll(next)}
                                        size="sm"
                                        disabled={isScanning || anyLoading}
                                        label="Apply recommended protection"
                                    />
                                </div>
                            )}
                            <button className="refresh-btn" onClick={(e) => { e.stopPropagation(); loadStatus(); }} disabled={isScanning || anyLoading}>
                                <Icon icon="refresh" size={14} className={isScanning ? 'spinning' : ''} />
                            </button>
                        </div>
                    </header>
                    <div className="blocklists-container">
                        {isScanning ? (
                            <div className="scanning-overlay"><div className="loader-spinner" /></div>
                        ) : (
                            <div className="blocklist-groups-wrapper">
                                {group1.length > 0 && (
                                    <div className="blocklist-group">
                                        <div className="group-header">
                                            <div className="flex flex-col gap-1">
                                                <span className="group-title">STANDARD PROTECTION</span>
                                                <span className="group-desc">Telemetry, advertising, and analytics blocking</span>
                                            </div>
                                        </div>
                                        <div className="blocklists-grid four-columns">
                                            {group1.map(item => (
                                                <BlocklistItem
                                                    key={item.name}
                                                    name={item.name}
                                                    description={blocklistInfo[item.name]?.description || 'No description available'}
                                                    entryCount={item.entries}
                                                    isApplied={status?.applied?.includes(item.name) || false}
                                                    onToggle={(enabled) => handleToggle(item.name, enabled)}
                                                    loading={actionLoading === item.name}
                                                    disabled={anyLoading}
                                                    logos={BRAND_LOGOS[item.name]}
                                                />
                                            ))}
                                        </div>
                                    </div>
                                )}

                                {group2.length > 0 && (
                                    <div className="blocklist-group">
                                        <div className="group-header">
                                            <span className="group-title">CONTENT FILTERING</span>
                                            <span className="group-desc">Advanced blocks for AI, piracy, and cloud uploads</span>
                                        </div>
                                        <div className="blocklists-grid three-columns">
                                            {group2.map(item => (
                                                <BlocklistItem
                                                    key={item.name}
                                                    name={item.name === 'telemetry-blocklist' ? 'TELEMETRY' : item.name.replace(/-/g, ' ').toUpperCase()}
                                                    description={blocklistInfo[item.name]?.description || 'No description available'}
                                                    entryCount={item.entries}
                                                    isApplied={status?.applied?.includes(item.name) || false}
                                                    onToggle={(enabled) => handleToggle(item.name, enabled)}
                                                    loading={actionLoading === item.name}
                                                    disabled={anyLoading}
                                                    logos={BRAND_LOGOS[item.name]}
                                                />
                                            ))}
                                        </div>
                                    </div>
                                )}
                            </div>
                        )}
                    </div>
                </section>
            </div>
        </SectionCard>
    );
}

// ─── Panel ────────────────────────────────────────────────────────────────────

function NetworkPanel() {
    const visibility = useVisibility();
    const { getPhysicalNetworkAdapters, setAdapterRandomMAC, restoreAdapterMAC } = useBackend();

    const showNetwork = visibility.isVisible({ capability: ["network"] });

    // Adapter state
    const [adapters, setAdapters] = useState<PhysicalNetworkAdapter[] | null>(null);
    const [adaptersLoading, setAdaptersLoading] = useState(false);
    const [adapterBusyId, setAdapterBusyId] = useState<string | null>(null);
    const [modeOverrides, setModeOverrides] = useState<Record<string, AdapterMode>>({});
    const [showInactiveAdapters, setShowInactiveAdapters] = useState(false);

    const refreshAdapters = useCallback(async () => {
        setAdaptersLoading(true);
        try {
            const res = await getPhysicalNetworkAdapters();
            if (res.success && res.data) setAdapters(res.data.adapters ?? []);
            else if (res.error) showError(res.error);
        } finally {
            setAdaptersLoading(false);
        }
    }, [getPhysicalNetworkAdapters]);

    useEffect(() => { refreshAdapters(); }, [refreshAdapters]);

    const handleAdapterModeChange = useCallback(async (adapter: PhysicalNetworkAdapter, nextMode: AdapterMode) => {
        setAdapterBusyId(adapter.id);
        setModeOverrides(m => ({ ...m, [adapter.id]: nextMode }));
        try {
            if (nextMode === "off") {
                const res = await restoreAdapterMAC(adapter.id);
                if (!res.success) { showError(res.error || "Failed to restore factory MAC"); return; }
                showSuccess(`Factory MAC restored on ${adapter.name}`);
            } else {
                const res = await setAdapterRandomMAC(adapter.id, nextMode);
                if (!res.success) { showError(res.error || "Failed to randomize MAC"); return; }
                const mac = res.data?.appliedMac ? ` → ${macDisplay(res.data.appliedMac)}` : "";
                showSuccess(`MAC randomized on ${adapter.name}${mac}`);
            }
            await refreshAdapters();
        } finally {
            setAdapterBusyId(null);
        }
    }, [restoreAdapterMAC, setAdapterRandomMAC, refreshAdapters]);

    return (
        <div className="h-full flex flex-col items-start">
            <div className="w-full max-w-7xl p-6 network-panel-stack">
                <PanelHeader
                    panelId="network"
                    title="Network Control"
                    description="See and control what your PC talks to — DNS, firewall rules, blocklists, and saved Wi-Fi networks."
                />

                <div className="network-panel-sections">
                    <NetworkSecurityControls />

                    {showNetwork && <NetworkMaintenanceTools />}

                    {/* Row 1: Port Watch (75% left) | Adapters + Wi-Fi Guard merged (25% right) */}
                    <div className="network-control-row network-control-row--port">
                        {showNetwork && (
                            <div className="ncr-main">
                                <PortGuardSection />
                            </div>
                        )}
                        {showNetwork && (
                            <div className="ncr-side">
                                <SectionCard
                                    title={"Adapters & Wi-Fi Guard"}
                                    icon="globe-network"
                                    className="ncr-stretch-card"
                                >
                                    <div className="merged-adapters-wifi-guard">
                                        {/* Segment 1 — Adapters. Shown to every density (was
                                            expert-only) — MAC randomization is a useful privacy
                                            control for Guided users too, not just power users. */}
                                        <section className="merged-segment merged-segment--adapters">
                                            <header className="merged-segment__header">
                                                <Icon icon="globe-network" size={11} />
                                                <span className="merged-segment__title">Network Adapters</span>
                                                <span className="merged-segment__count">
                                                    {adapters ? `${(adapters ?? []).filter(a => a.status === 'Up').length} up` : '—'}
                                                </span>
                                            </header>
                                            <p className="merged-segment__blurb">
                                                Randomize a MAC address so the adapter can't be tracked by hardware ID.
                                            </p>
                                            <div className="merged-segment__body">
                                                <AdaptersList
                                                    adapters={adapters}
                                                    adaptersLoading={adaptersLoading}
                                                    adapterBusyId={adapterBusyId}
                                                    modeOverrides={modeOverrides}
                                                    showInactiveAdapters={showInactiveAdapters}
                                                    onModeChange={handleAdapterModeChange}
                                                    onShowInactiveToggle={setShowInactiveAdapters}
                                                    embedded
                                                />
                                            </div>
                                        </section>
                                        {/* Segment 2 — Wi-Fi Guard (fake hotspot / rogue AP detector) */}
                                        <section className="merged-segment merged-segment--wifi">
                                            <div className="merged-segment__body">
                                                <WifiGuardSection embedded />
                                            </div>
                                        </section>
                                    </div>
                                </SectionCard>
                            </div>
                        )}
                    </div>

                    {/* Row 2: Active Ports */}
                    <div className="network-control-row network-control-row--active">
                        {visibility.isVisible({ minDensity: "expert", capability: ["network"] }) && (
                            <div className="ncr-main">
                                <ActivePorts />
                            </div>
                        )}
                    </div>

                    <div className="h-12"></div>
                </div>
            </div>
        </div>
    );
}

export default NetworkPanel;
