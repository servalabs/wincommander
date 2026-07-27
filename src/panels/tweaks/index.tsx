import { useEffect, useCallback, useMemo } from "react";
import { useSearchQuery } from "../../context/SearchContext";
import useBackend from "../../hooks/useBackend";
import useVisibility from "../../hooks/useVisibility";
import SectionCard from "../../components/shared/SectionCard";
import ToggleSection, { type ExternalToggleConflict } from "../../components/shared/ToggleSection";
import DefenderExclusionAuditor from "../../components/tweaks/DefenderExclusionAuditor";
import ExploitProtectionExtras from "../../components/tweaks/ExploitProtectionExtras";
import PowerPlanCard from "../../components/tweaks/PowerPlanCard";
import WindowsAiAdvancedActions from "../../components/tweaks/WindowsAiAdvancedActions";
import VmSandboxSection from "./VmSandboxSection";
import ContextMenuIntegrationCard from "../../components/tweaks/ContextMenuIntegrationCard";
import { TWEAKS_SECTIONS, TWEAKS_TOGGLES } from "../../registry/tweaks.toggles";
import GlobalSearchNoResults from "../../components/shared/GlobalSearchNoResults";
import { resolveToggleText } from "../../types/toggles";
import type { ToggleDef } from "../../types/toggles";
import { showError } from "../../utils/toast";
import PanelHeader from "../../components/shared/PanelHeader";
import { useAppState } from "../../context/AppContext";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../components/ui/tabs";
import { useTweaksSessionState } from "./tweaksSessionState";
import './index.css';

type TweaksTab = "appearance" | "performance" | "os-boot" | "security" | "exploit-protection";

function detectGpuVendor(gpu: string | null | undefined): "amd" | "nvidia" | "intel" | null {
    const g = (gpu ?? "").toLowerCase();
    if (g.includes("amd") || g.includes("radeon")) return "amd";
    if (g.includes("nvidia") || g.includes("geforce")) return "nvidia";
    if (g.includes("intel") || g.includes("iris") || g.includes("arc") || g.includes("uhd")) return "intel";
    return null;
}

function toggleMatchesQuery(t: ToggleDef, q: string): boolean {
    const wording = resolveToggleText(t, 'simple');
    return wording.label.toLowerCase().includes(q) ||
           wording.description.toLowerCase().includes(q) ||
           t.label.toLowerCase().includes(q) ||
           t.description.toLowerCase().includes(q) ||
           (t.keywords ? t.keywords.some((k: string) => k.toLowerCase().includes(q)) : false);
}

// Whether any toggle in `sectionId` matches the (already lowercased, trimmed)
// query — used both for the panel-wide "no results" check and to work out
// which tab a match lives in so search can jump there (see the tab-switch
// effect below).
function sectionHasMatch(sectionId: string, q: string): boolean {
    return TWEAKS_TOGGLES.some((t) => t.section === sectionId && toggleMatchesQuery(t, q));
}

export default function TweaksPanel() {
    const { searchQuery } = useSearchQuery();
    const visibility = useVisibility();
    const { systemInfo } = useAppState();
    const [activeTab, setActiveTab] = useTweaksSessionState<TweaksTab>("tweaks.active-tab", "appearance");

    const { restartExplorer, error } = useBackend();

    const handlePostToggle = useCallback(async (t: ToggleDef) => {
        if (t.requiresRestart) await restartExplorer();
    }, [restartExplorer]);

    const getNotificationExternalConflicts = useCallback((toggle: ToggleDef): ExternalToggleConflict[] => {
        if (toggle.id !== "notifications") return [];
        return [{ id: "wincommander-alert-notifications", label: "WinCommander alert notifications" }];
    }, []);

    useEffect(() => { if (error) showError(error); }, [error]);

    const hasResults = useMemo(() => {
        if (!searchQuery.trim()) return true;
        const q = searchQuery.toLowerCase().trim();
        const matchesContextMenu = "context menu secure delete".includes(q) || "secure right-click".includes(q);
        return TWEAKS_TOGGLES.some((t) => toggleMatchesQuery(t, q)) || matchesContextMenu;
    }, [searchQuery]);

    const isAdvanced = visibility.density === "expert";
    const showExpertSpeed = visibility.isVisible({ minDensity: "expert" });
    const noSearch = !searchQuery.trim();

    const tabOrder = useMemo<TweaksTab[]>(() => {
        const order: TweaksTab[] = ["appearance", "performance"];
        if (showExpertSpeed) order.push("os-boot");
        order.push("security");
        if (showExpertSpeed) order.push("exploit-protection");
        return order;
    }, [showExpertSpeed]);

    // A tab whose TabsTrigger got hidden by a density downgrade (e.g. a
    // policy/entitlement change while the app is open) must not stay the
    // active tab with no visible way back to it.
    useEffect(() => {
        if (!tabOrder.includes(activeTab)) setActiveTab("appearance");
    }, [tabOrder, activeTab, setActiveTab]);

    // Search-aware tab switching: a match living in an inactive tab is
    // otherwise invisible with no indication of which tab to check. Only
    // reacts to the search query itself changing (typing a NEW search) —
    // never to a manual tab click, since clicking a tab doesn't touch
    // searchQuery and so can't retrigger this effect.
    useEffect(() => {
        const q = searchQuery.toLowerCase().trim();
        if (!q) return;
        const tabMatches = (tab: TweaksTab): boolean => {
            switch (tab) {
                case "appearance":
                    return sectionHasMatch("ui", q) ||
                        "context menu secure delete".includes(q) || "secure right-click".includes(q);
                case "performance":
                    return sectionHasMatch("power", q) || sectionHasMatch("performance", q) ||
                        (isAdvanced && sectionHasMatch("gpu", q));
                case "os-boot":
                    return showExpertSpeed && (sectionHasMatch("os", q) || sectionHasMatch("boot", q));
                case "security":
                    return showExpertSpeed && sectionHasMatch("security", q);
                case "exploit-protection":
                    return showExpertSpeed && sectionHasMatch("exploitProtection", q);
            }
        };
        if (tabMatches(activeTab)) return;
        const firstMatch = tabOrder.find(tabMatches);
        if (firstMatch) setActiveTab(firstMatch);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [searchQuery]);

    return (
        <div className="panel-container tweaks-panel">
            <PanelHeader
                panelId="tweaks"
                title="Windows Settings"
                description="Tune Windows for speed and sanity — power, Explorer, context menus, startup, and the debloat + annoyance fixes Microsoft leaves on by default."
            />

            <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as TweaksTab)}>
                <TabsList className="w-full flex-wrap justify-start">
                    <TabsTrigger value="appearance">Appearance &amp; Explorer</TabsTrigger>
                    <TabsTrigger value="performance">Performance &amp; power</TabsTrigger>
                    {showExpertSpeed && <TabsTrigger value="os-boot">OS &amp; boot</TabsTrigger>}
                    <TabsTrigger value="security">Security &amp; apps</TabsTrigger>
                    {showExpertSpeed && <TabsTrigger value="exploit-protection">Exploit protection</TabsTrigger>}
                </TabsList>

                <TabsContent value="appearance">
                    <AppearanceExplorerTab
                        isAdvanced={isAdvanced}
                        showExpertSpeed={showExpertSpeed}
                        searchQuery={searchQuery}
                        handlePostToggle={handlePostToggle}
                        getNotificationExternalConflicts={getNotificationExternalConflicts}
                    />
                </TabsContent>

                <TabsContent value="performance">
                    <PerformancePowerTab
                        isAdvanced={isAdvanced}
                        noSearch={noSearch}
                        searchQuery={searchQuery}
                        handlePostToggle={handlePostToggle}
                        gpu={systemInfo?.gpu}
                    />
                </TabsContent>

                <TabsContent value="os-boot">
                    <OsBootTab
                        showExpertSpeed={showExpertSpeed}
                        isAdvanced={isAdvanced}
                        searchQuery={searchQuery}
                        handlePostToggle={handlePostToggle}
                    />
                </TabsContent>

                <TabsContent value="security">
                    <SecurityAppsTab
                        showExpertSpeed={showExpertSpeed}
                        noSearch={noSearch}
                        searchQuery={searchQuery}
                        handlePostToggle={handlePostToggle}
                    />
                </TabsContent>

                <TabsContent value="exploit-protection">
                    <ExploitProtectionTab
                        showExpertSpeed={showExpertSpeed}
                        noSearch={noSearch}
                        searchQuery={searchQuery}
                        handlePostToggle={handlePostToggle}
                    />
                </TabsContent>
            </Tabs>

            {(!hasResults && !noSearch) && (
                <GlobalSearchNoResults searchQuery={searchQuery} currentPanelId="tweaks" />
            )}
        </div>
    );
}

// ── Tab 1: Appearance & Explorer ─────────────────────────────────────────
function AppearanceExplorerTab({ isAdvanced, showExpertSpeed, searchQuery, handlePostToggle, getNotificationExternalConflicts }: {
    isAdvanced: boolean;
    showExpertSpeed: boolean;
    searchQuery: string;
    handlePostToggle: (t: ToggleDef) => Promise<void>;
    getNotificationExternalConflicts: (toggle: ToggleDef) => ExternalToggleConflict[];
}) {
    return (
        <div className="flex flex-col gap-4">
            <ToggleSection
                section={{
                    ...TWEAKS_SECTIONS[0],
                    title: isAdvanced ? TWEAKS_SECTIONS[0].title : "Appearance",
                }}
                toggles={TWEAKS_TOGGLES}
                onToggled={handlePostToggle}
                searchQuery={searchQuery}
                getExternalActiveConflicts={getNotificationExternalConflicts}
            />

            {/* Context Menu Integration (expert) — same gate as before the move. */}
            {showExpertSpeed && (
                <ContextMenuIntegrationCard
                    isAdvanced={isAdvanced}
                    searchQuery={searchQuery}
                />
            )}
        </div>
    );
}

// ── Tab 2: Performance & power ────────────────────────────────────────────
function PerformancePowerTab({ isAdvanced, noSearch, searchQuery, handlePostToggle, gpu }: {
    isAdvanced: boolean;
    noSearch: boolean;
    searchQuery: string;
    handlePostToggle: (t: ToggleDef) => Promise<void>;
    gpu: string | null | undefined;
}) {
    const gpuFilteredToggles = useMemo(() => {
        const vendor = detectGpuVendor(gpu);
        if (!vendor) return TWEAKS_TOGGLES;
        return TWEAKS_TOGGLES.filter((t) => {
            if (t.section !== "gpu") return true;
            if (t.id.startsWith("amd")) return vendor === "amd";
            if (t.id.startsWith("nv")) return vendor === "nvidia";
            if (t.id.startsWith("intel")) return vendor === "intel";
            return true;
        });
    }, [gpu]);

    return (
        <div className="flex flex-col gap-4">
            {noSearch && (
                <PowerPlanCard titleOverride={isAdvanced ? "Power Plan" : "Energy & Speed"} />
            )}

            {/* Performance & Gaming — fully defined in the registry but not
                previously rendered anywhere; wiring it up here is intentional. */}
            <ToggleSection
                section={TWEAKS_SECTIONS[4]}
                toggles={TWEAKS_TOGGLES}
                onToggled={handlePostToggle}
                searchQuery={searchQuery}
            />

            {/* Power Management pairs with GPU Vendor Tweaks (advanced only) so
                Power Management's 2 toggles don't sit alone in a full-width,
                mostly-empty section. Non-advanced users still get Power
                Management full width since GPU Vendor Tweaks is hidden. */}
            {isAdvanced ? (
                <div className="grid grid-cols-2 gap-4">
                    <ToggleSection
                        section={TWEAKS_SECTIONS[6]}
                        toggles={TWEAKS_TOGGLES}
                        onToggled={handlePostToggle}
                        searchQuery={searchQuery}
                    />
                    <ToggleSection
                        section={TWEAKS_SECTIONS[5]}
                        toggles={gpuFilteredToggles}
                        onToggled={handlePostToggle}
                        searchQuery={searchQuery}
                    />
                </div>
            ) : (
                <ToggleSection
                    section={TWEAKS_SECTIONS[6]}
                    toggles={TWEAKS_TOGGLES}
                    onToggled={handlePostToggle}
                    searchQuery={searchQuery}
                />
            )}
        </div>
    );
}

// ── Tab 3: OS & boot ──────────────────────────────────────────────────────
function OsBootTab({ showExpertSpeed, isAdvanced, searchQuery, handlePostToggle }: {
    showExpertSpeed: boolean;
    isAdvanced: boolean;
    searchQuery: string;
    handlePostToggle: (t: ToggleDef) => Promise<void>;
}) {
    if (!showExpertSpeed) return null;
    return (
        <SectionCard title="System Hardware">
            <div className="tweaks-sublabel">OS &amp; Hardware</div>
            <ToggleSection
                section={{
                    ...TWEAKS_SECTIONS[2],
                    title: isAdvanced ? TWEAKS_SECTIONS[2].title : "System Hardware",
                }}
                toggles={TWEAKS_TOGGLES}
                onToggled={handlePostToggle}
                bare
                searchQuery={searchQuery}
            />
            <div className="tweaks-sublabel tweaks-sublabel-gap">Boot &amp; Kernel</div>
            <ToggleSection
                section={TWEAKS_SECTIONS[3]}
                toggles={TWEAKS_TOGGLES}
                onToggled={handlePostToggle}
                bare
                searchQuery={searchQuery}
            />
        </SectionCard>
    );
}

// ── Tab 4: Security & apps ───────────────────────────────────────────────
function SecurityAppsTab({ showExpertSpeed, noSearch, searchQuery, handlePostToggle }: {
    showExpertSpeed: boolean;
    noSearch: boolean;
    searchQuery: string;
    handlePostToggle: (t: ToggleDef) => Promise<void>;
}) {
    return (
        <div className="flex flex-col gap-4">
            {/* VmSandboxSection stays visible at every density (only hidden
                during search, as before) — rendered ahead of the expert-gated
                block below so it isn't swallowed by the showExpertSpeed gate. */}
            {noSearch && <VmSandboxSection />}

            {/* Security & Apps + AI actions + Defender Auditor (expert) */}
            {showExpertSpeed && (
                <SectionCard title="Security &amp; Apps">
                    <ToggleSection
                        section={TWEAKS_SECTIONS[1]}
                        toggles={TWEAKS_TOGGLES}
                        onToggled={handlePostToggle}
                        bare
                        searchQuery={searchQuery}
                    />
                    {noSearch && <WindowsAiAdvancedActions />}
                    {noSearch && (
                        <div style={{ marginTop: 14 }}>
                            <DefenderExclusionAuditor />
                        </div>
                    )}
                </SectionCard>
            )}
        </div>
    );
}

// ── Tab 5: Exploit protection ────────────────────────────────────────────
function ExploitProtectionTab({ showExpertSpeed, noSearch, searchQuery, handlePostToggle }: {
    showExpertSpeed: boolean;
    noSearch: boolean;
    searchQuery: string;
    handlePostToggle: (t: ToggleDef) => Promise<void>;
}) {
    return (
        <div className="flex flex-col gap-4">
            {showExpertSpeed && (
                <ToggleSection
                    section={TWEAKS_SECTIONS[7]}
                    toggles={TWEAKS_TOGGLES}
                    onToggled={handlePostToggle}
                    searchQuery={searchQuery}
                />
            )}

            {/* Exploit Protection extras — Acquisition Monitor switch + on-demand
                vulnerable-driver scan (Round 2 bespoke controls, see
                components/tweaks/ExploitProtectionExtras.tsx) */}
            {showExpertSpeed && noSearch && <ExploitProtectionExtras />}
        </div>
    );
}
