import { useCallback, useEffect, useMemo, useState } from "react";
import { useSearchQuery } from "../../context/SearchContext";
import { useAppState } from "../../context/AppContext";
import useVisibility from "../../hooks/useVisibility";
import useEntitlements from "../../hooks/useEntitlements";
import { useTourActive } from "../../lib/tourActive";
import { argus } from "../../hooks/useArgus";
import { authAnomalyStatus, sessionMonitorStatus } from "../../hooks/monitorStatus";
import ToggleSection from "../../components/shared/ToggleSection";
import SectionCard from "../../components/shared/SectionCard";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { PRIVACY_SECTIONS, PRIVACY_TOGGLES } from "../../registry/privacy.toggles";
import { CAPABILITY_SECTIONS, CAPABILITY_TOGGLES } from "../../registry/capabilities.toggles";
import GlobalSearchNoResults from "../../components/shared/GlobalSearchNoResults";
import { resolveToggleText, getByPath } from "../../types/toggles";
import { usePrivacySessionState } from "./privacySessionState";
import BrowserHardeningSection from "./BrowserHardeningSection";
import PrivacyShieldCard from "./PrivacyShieldCard";
import ScreenCaptureSection from "./ScreenCaptureSection";
import PasteMonitorSection from "./PasteMonitorSection";
import DecoyMonitorSection from "./DecoyMonitorSection";
import RansomwareMonitorSection from "./RansomwareMonitorSection";
import RemoteAccessMonitorSection from "./RemoteAccessMonitorSection";
import MonitoringMirrorSection from "./MonitoringMirrorSection";
import ArgusDlpSection from "./ArgusDlpSection";
import ArgusTamperSection from "./ArgusTamperSection";
import ArgusPrintUsbSection from "./ArgusPrintUsbSection";
import CanaryTokensSection from "./CanaryTokensSection";
import UsbDevicesSection from "./UsbDevicesSection";
import PrintActivitySection from "./PrintActivitySection";
import RdpIdleCard from "./RdpIdleCard";
import PanelHeader from "../../components/shared/PanelHeader";
import { useSettingsQuery } from "../../hooks/queries/useSettingsQuery";
import {
    DEFAULT_RANSOMWARE_THRESHOLD,
    DEFAULT_RANSOMWARE_WINDOW_SECONDS,
    DEFAULT_RANSOMWARE_ACTION,
} from "../../hooks/useRansomwareMonitor";
import type { RansomwareAction } from "../../types/settings";
import './index.css';

export default function PrivacyPanel() {
    const { appSettings, patchAppSettings } = useAppState();
    const { data: settingsData } = useSettingsQuery();
    const { searchQuery } = useSearchQuery();
    const visibility = useVisibility();
    const { density } = visibility;
    const { canUse } = useEntitlements();

    const isAdvanced = density === "expert";
    const tourActive = useTourActive();

    const showPrivacyControls = visibility.isVisible({ capability: ["privacy"] });
    // Revealed during a tour as well as in Expert: the walkthrough anchors a
    // step to Browser Hardening (and, via showMonitoring below, to Privacy
    // Shield and RDP Idle), and a Guided user got a broken step where the
    // anchor never mounted (2026-07-26 fix). `isAdvanced` stays density-only,
    // so the section keeps its Guided wording — only its presence changes.
    const showExpertPrivacy = density === "expert" || tourActive;
    const showMonitoring = visibility.isVisible({ capability: ["monitoring"] }) || showExpertPrivacy;

    const [activeTab, setActiveTab] = usePrivacySessionState("privacy.active-tab", "tracking");

    // The guide tour anchors three in-panel steps (Browser Hardening, Privacy
    // Shield, RDP Idle) inside the Monitor tab. Tabs unmount inactive content,
    // so without forcing the tab here the anchors never mount and the tour
    // silently stalls on whichever tab the user last had open.
    useEffect(() => {
        if (tourActive) setActiveTab("monitor");
    }, [tourActive, setActiveTab]);

    const privacyToggles = PRIVACY_TOGGLES;

    const pasteMonitorEnabled = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorEnabled ?? false;
    const pasteMonitorCategories = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorCategories ?? null;
    const pasteCryptoSwapEnabled = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorCryptoSwapEnabled ?? null;
    const pasteAutoClearEnabled = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorAutoClearEnabled ?? null;
    const pasteAutoClearSeconds = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorAutoClearSeconds ?? null;
    const pasteAutoClearOnLock = appSettings?.ideal?.privacy?.clipboard?.pasteMonitorAutoClearOnLock ?? null;
    const patchClipboard = (patch: Record<string, unknown>) =>
        patchAppSettings({ ideal: { privacy: { clipboard: patch } } } as any).catch(() => {});

    const decoyEnabled = appSettings?.ideal?.privacy?.decoyMonitor?.enabled ?? false;
    const decoyEnrolledPaths = appSettings?.ideal?.privacy?.decoyMonitor?.enrolledPaths ?? [];
    const patchDecoy = (patch: { enabled?: boolean; enrolledPaths?: string[] }) =>
        patchAppSettings({ ideal: { privacy: { decoyMonitor: patch } } } as any).catch(() => {});

    const ransomwareEnabled = appSettings?.ideal?.privacy?.ransomwareMonitor?.enabled ?? false;
    const ransomwareThreshold = appSettings?.ideal?.privacy?.ransomwareMonitor?.threshold ?? DEFAULT_RANSOMWARE_THRESHOLD;
    const ransomwareWindowSeconds = appSettings?.ideal?.privacy?.ransomwareMonitor?.windowSeconds ?? DEFAULT_RANSOMWARE_WINDOW_SECONDS;
    const ransomwareCustomDirs = appSettings?.ideal?.privacy?.ransomwareMonitor?.customWatchDirs ?? [];
    const ransomwareAction = appSettings?.ideal?.privacy?.ransomwareMonitor?.action ?? DEFAULT_RANSOMWARE_ACTION;
    const patchRansomware = (patch: {
        enabled?: boolean;
        threshold?: number;
        windowSeconds?: number;
        customWatchDirs?: string[];
        action?: RansomwareAction;
    }) =>
        patchAppSettings({ ideal: { privacy: { ransomwareMonitor: patch } } } as any).catch(() => {});

    const remoteAccessEnabled = appSettings?.ideal?.privacy?.remoteAccessMonitor?.enabled ?? false;
    const remoteAccessTools = appSettings?.ideal?.privacy?.remoteAccessMonitor?.tools ?? null;
    const patchRemoteAccess = (patch: { enabled?: boolean; tools?: Record<string, boolean> }) =>
        patchAppSettings({ ideal: { privacy: { remoteAccessMonitor: patch } } } as any).catch(() => {});

    const screenCaptureDetectionEnabled = appSettings?.ideal?.privacy?.screenCapture?.detectionEnabled ?? false;
    const screenCaptureProtectWindow = appSettings?.ideal?.privacy?.screenCapture?.protectWindow ?? false;
    const patchScreenCapture = (patch: { detectionEnabled?: boolean; protectWindow?: boolean }) =>
        patchAppSettings({ ideal: { privacy: { screenCapture: patch } } } as any).catch(() => {});

    // These six Pro monitors are not represented in appSettings. Poll their
    // status endpoints so the top strip reflects the three controls now in
    // What my employer sees alongside the remaining Argus monitor cards.
    const [extraMonitorsRunning, setExtraMonitorsRunning] = useState(0);
    const refreshExtraMonitors = useCallback(async () => {
        const checks: Promise<boolean>[] = [
            argus.appUsageStatus().then((s) => s?.running === true).catch(() => false),
            argus.dlpStatus().then((s) => s?.running === true).catch(() => false),
            argus.tamperStatus().then((s) => s?.running === true).catch(() => false),
            argus.printUsbStatus().then((s) => s?.running === true).catch(() => false),
            authAnomalyStatus().then((s) => !!s?.running).catch(() => false),
            sessionMonitorStatus().then((s) => !!s?.running).catch(() => false),
        ];
        const results = await Promise.all(checks);
        setExtraMonitorsRunning(results.filter(Boolean).length);
    }, []);

    useEffect(() => {
        // These commands are Pro-gated on the backend and the cards
        // themselves are only mounted while the monitoring section is
        // visible, so skip polling entirely otherwise.
        if (!showMonitoring || !canUse("paid")) {
            setExtraMonitorsRunning(0);
            return;
        }
        void refreshExtraMonitors();
        const id = setInterval(() => void refreshExtraMonitors(), 15_000);
        return () => clearInterval(id);
    }, [showMonitoring, canUse, refreshExtraMonitors]);

    // Stats strip — counts active privacy controls and armed monitors
    const activePrivacyCount = useMemo(() => {
        if (!settingsData) return 0;
        return [...privacyToggles, ...CAPABILITY_TOGGLES].filter(t => {
            const val = getByPath(settingsData as any, t.currentPath);
            if (t.checkedWhen !== undefined) return val === t.checkedWhen;
            return Boolean(val);
        }).length;
    }, [settingsData, privacyToggles]);

    const totalPrivacyCount = useMemo(
        () => [...privacyToggles, ...CAPABILITY_TOGGLES].length,
        [privacyToggles],
    );

    const monitorCount = useMemo(() => {
        let c = 0;
        if (pasteMonitorEnabled) c++;
        if (decoyEnabled) c++;
        if (ransomwareEnabled) c++;
        if (screenCaptureDetectionEnabled) c++;
        if (remoteAccessEnabled) c++;
        c += extraMonitorsRunning;
        return c;
    }, [pasteMonitorEnabled, decoyEnabled, ransomwareEnabled, screenCaptureDetectionEnabled, remoteAccessEnabled, extraMonitorsRunning]);

    const warningCount = useMemo(() => {
        if (!settingsData) return 0;
        return privacyToggles.filter(t => {
            if (!t.defaultOn) return false;
            const val = getByPath(settingsData as any, t.currentPath);
            if (t.checkedWhen !== undefined) return val !== t.checkedWhen;
            return !Boolean(val);
        }).length;
    }, [settingsData, privacyToggles]);

    const monitoringMatchesSearch = useMemo(() => {
        if (!searchQuery.trim()) return true;
        const q = searchQuery.toLowerCase().trim();
        const terms = [
            "alerts",
            "monitoring",
            "paste",
            "clipboard",
            "decoy",
            "honeypot",
            "ransomware",
            "screen capture",
            "screen privacy",
            "remote access",
            "remote control",
            "shield",
            "gaze",
            "camera",
        ];
        return terms.some((term) => term.includes(q));
    }, [searchQuery]);

    // Results counting for empty state
    const hasResults = useMemo(() => {
        if (!searchQuery.trim()) return true;
        const q = searchQuery.toLowerCase().trim();
        const browserTerms = ["browser", "browsers", "browser hardening", "secure browsers", "extensions", "firefox", "chrome", "edge", "brave"];
        const checkMatch = (t: any) => {
            const wording = resolveToggleText(t, 'simple');
            return wording.label.toLowerCase().includes(q) ||
                   wording.description.toLowerCase().includes(q) ||
                   t.label.toLowerCase().includes(q) ||
                   t.description.toLowerCase().includes(q) ||
                   (t.keywords && t.keywords.some((k: string) => k.toLowerCase().includes(q)));
        };
        const matchesBrowserHardening = showExpertPrivacy && browserTerms.some((term) => term.includes(q));
        return privacyToggles.some(checkMatch) || CAPABILITY_TOGGLES.some(checkMatch) || matchesBrowserHardening || (showMonitoring && monitoringMatchesSearch);
    }, [monitoringMatchesSearch, privacyToggles, searchQuery, showExpertPrivacy, showMonitoring]);

    const noSearch = !searchQuery.trim();

    return (
        <>
            <div className="panel-container">
                <PanelHeader
                    panelId="privacy"
                    title="Privacy Settings"
                    description="Stop Windows from watching you — telemetry, tracking, app permissions, camera & mic, and one-tap privacy cleanup."
                />

                {/* Stats strip — quick at-a-glance summary */}
                {noSearch && showPrivacyControls && (
                    <div className="privacy-stats-strip">
                        <div className="privacy-stat">
                            <span className="privacy-stat-n" style={{ color: warningCount === 0 ? 'var(--color-success)' : 'var(--color-warning)' }}>
                                {activePrivacyCount}<span className="privacy-stat-total">/{totalPrivacyCount}</span>
                            </span>
                            <span className="privacy-stat-l">Controls on</span>
                        </div>
                        {warningCount > 0 && (
                            <div className="privacy-stat">
                                <span className="privacy-stat-n" style={{ color: 'var(--color-warning)' }}>{warningCount}</span>
                                <span className="privacy-stat-l">Warnings</span>
                            </div>
                        )}
                        {showMonitoring && (
                            <div className="privacy-stat">
                                <span className="privacy-stat-n" style={{ color: monitorCount > 0 ? 'var(--color-info)' : 'var(--color-text-muted)' }}>
                                    {monitorCount}
                                </span>
                                <span className="privacy-stat-l">Monitors armed</span>
                            </div>
                        )}
                        <div className="privacy-stat-divider" />
                        <div className="privacy-stat-pills">
                            {warningCount === 0 && activePrivacyCount > 0 && (
                                <span className="privacy-pill privacy-pill-ok">All safe</span>
                            )}
                            {warningCount > 0 && (
                                <span className="privacy-pill privacy-pill-warn">{warningCount} setting{warningCount > 1 ? 's' : ''} exposed</span>
                            )}
                            {monitorCount === 0 && showMonitoring && (
                                <span className="privacy-pill privacy-pill-mute">No monitors active</span>
                            )}
                        </div>
                    </div>
                )}

                <Tabs value={activeTab} onValueChange={setActiveTab}>
                    <TabsList className="w-full flex-wrap justify-start">
                        {showPrivacyControls && <TabsTrigger value="tracking">Privacy &amp; Tracking</TabsTrigger>}
                        {showPrivacyControls && <TabsTrigger value="capabilities">App Capabilities</TabsTrigger>}
                        {showMonitoring && <TabsTrigger value="monitor">Monitor</TabsTrigger>}
                    </TabsList>

                    {showPrivacyControls && (
                        <TabsContent value="tracking">
                            <ToggleSection
                                section={{
                                    ...PRIVACY_SECTIONS[0],
                                    title: isAdvanced ? PRIVACY_SECTIONS[0].title : "Privacy basics",
                                }}
                                toggles={privacyToggles}
                                gridClassName="privacy-3col-grid privacy-compact-grid"
                                searchQuery={searchQuery}
                            />
                        </TabsContent>
                    )}

                    {showPrivacyControls && (
                        <TabsContent value="capabilities">
                            <ToggleSection
                                section={{
                                    ...CAPABILITY_SECTIONS[0],
                                    title: isAdvanced ? CAPABILITY_SECTIONS[0].title : "App permissions",
                                }}
                                toggles={CAPABILITY_TOGGLES}
                                gridClassName="privacy-compact-grid capability-grid"
                                searchQuery={searchQuery}
                            />
                        </TabsContent>
                    )}

                    {showMonitoring && (
                        <TabsContent value="monitor" className="flex flex-col gap-4">
                            {showExpertPrivacy && (
                                <BrowserHardeningSection
                                    isAdvanced={isAdvanced}
                                    searchQuery={searchQuery}
                                />
                            )}

                            {monitoringMatchesSearch && (
                                <SectionCard title="Alerts & Monitoring">
                                    {/* Top row — Privacy Shield (left) and RDP Idle (right)
                                      * sit side-by-side as the headline monitoring controls. */}
                                    <div className="privacy-monitor-toprow">
                                        <PrivacyShieldCard />
                                        <div data-tour="privacy-rdp-idle">
                                            <RdpIdleCard />
                                        </div>
                                    </div>

                                    {/* Keep monitor ordering stable. The previous expanded-card
                                      * reflow moved cards between columns, which left large blank
                                      * areas and made controls appear to jump around. */}
                                    <div className="panel-grid privacy-monitoring-grid" style={{ marginTop: 12 }}>
                                        <div className="privacy-monitor-cell">
                                            <ScreenCaptureSection
                                                detectionEnabled={screenCaptureDetectionEnabled}
                                                protectWindow={screenCaptureProtectWindow}
                                                onPatch={patchScreenCapture}
                                            />
                                        </div>
                                        <div className="privacy-monitor-cell">
                                            <DecoyMonitorSection
                                                isAdvanced={isAdvanced}
                                                searchQuery=""
                                                enabled={decoyEnabled}
                                                enrolledPaths={decoyEnrolledPaths}
                                                onPatchDecoy={patchDecoy}
                                            />
                                        </div>
                                        <div className="privacy-monitor-cell">
                                            <RemoteAccessMonitorSection
                                                isAdvanced={isAdvanced}
                                                searchQuery=""
                                                enabled={remoteAccessEnabled}
                                                toolOverrides={remoteAccessTools}
                                                onPatch={patchRemoteAccess}
                                            />
                                        </div>
                                        <div className="privacy-monitor-cell"><MonitoringMirrorSection /></div>
                                        <div className="privacy-monitor-cell"><ArgusDlpSection /></div>
                                        <div className="privacy-monitor-cell"><ArgusTamperSection /></div>
                                        <div className="privacy-monitor-cell"><ArgusPrintUsbSection /></div>
                                        <div className="privacy-monitor-cell"><CanaryTokensSection /></div>
                                        <div className="privacy-monitor-cell"><UsbDevicesSection /></div>
                                        <div className="privacy-monitor-cell">
                                            <PasteMonitorSection
                                                isAdvanced={isAdvanced}
                                                searchQuery=""
                                                enabled={pasteMonitorEnabled}
                                                categories={pasteMonitorCategories}
                                                cryptoSwapEnabled={pasteCryptoSwapEnabled}
                                                autoClearEnabled={pasteAutoClearEnabled}
                                                autoClearSeconds={pasteAutoClearSeconds}
                                                autoClearOnLock={pasteAutoClearOnLock}
                                                onPatchClipboard={patchClipboard}
                                            />
                                        </div>
                                        <div className="privacy-monitor-cell">
                                            <RansomwareMonitorSection
                                                isAdvanced={isAdvanced}
                                                searchQuery=""
                                                enabled={ransomwareEnabled}
                                                threshold={ransomwareThreshold}
                                                windowSeconds={ransomwareWindowSeconds}
                                                customWatchDirs={ransomwareCustomDirs}
                                                action={ransomwareAction}
                                                onPatchRansomware={patchRansomware}
                                            />
                                        </div>
                                        <div className="privacy-monitor-cell"><PrintActivitySection /></div>
                                    </div>
                                </SectionCard>
                            )}
                        </TabsContent>
                    )}
                </Tabs>

                {/* Calculator Mode moved to the Secret Settings panel (2026-06-12). */}

                {!hasResults && (
                    <GlobalSearchNoResults searchQuery={searchQuery} currentPanelId="privacy" />
                )}
            </div>
        </>
    );
}
