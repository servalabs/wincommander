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
import './index.css';

function detectGpuVendor(gpu: string | null | undefined): "amd" | "nvidia" | "intel" | null {
    const g = (gpu ?? "").toLowerCase();
    if (g.includes("amd") || g.includes("radeon")) return "amd";
    if (g.includes("nvidia") || g.includes("geforce")) return "nvidia";
    if (g.includes("intel") || g.includes("iris") || g.includes("arc") || g.includes("uhd")) return "intel";
    return null;
}

export default function TweaksPanel() {
    const { searchQuery } = useSearchQuery();
    const visibility = useVisibility();
    const { systemInfo } = useAppState();

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
        const checkMatch = (t: any) => {
            const wording = resolveToggleText(t, 'simple');
            return wording.label.toLowerCase().includes(q) ||
                   wording.description.toLowerCase().includes(q) ||
                   t.label.toLowerCase().includes(q) ||
                   t.description.toLowerCase().includes(q) ||
                   (t.keywords && t.keywords.some((k: string) => k.toLowerCase().includes(q)));
        };
        const matchesContextMenu = "context menu secure delete".includes(q) || "secure right-click".includes(q);
        return TWEAKS_TOGGLES.some(checkMatch) || matchesContextMenu;
    }, [searchQuery]);

    const isAdvanced = visibility.density === "expert";
    const showExpertSpeed = visibility.isVisible({ minDensity: "expert" });
    const noSearch = !searchQuery.trim();

    const gpuFilteredToggles = useMemo(() => {
        const vendor = detectGpuVendor(systemInfo?.gpu);
        if (!vendor) return TWEAKS_TOGGLES;
        return TWEAKS_TOGGLES.filter((t) => {
            if (t.section !== "gpu") return true;
            if (t.id.startsWith("amd")) return vendor === "amd";
            if (t.id.startsWith("nv")) return vendor === "nvidia";
            if (t.id.startsWith("intel")) return vendor === "intel";
            return true;
        });
    }, [systemInfo?.gpu]);

    return (
        <div className="panel-container tweaks-panel">
            <PanelHeader
                panelId="tweaks"
                title="Windows Settings"
                description="Tune Windows for speed and sanity — power, Explorer, context menus, startup, and the debloat + annoyance fixes Microsoft leaves on by default."
            />
            <div className="panel-columns">
                {/* LEFT COLUMN — appearance / performance */}
                <div className="column-stack">
                    {/* Appearance & Explorer — the most common toggles */}
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

                    {/* Power plan stays with the other Windows settings. */}
                    {noSearch && (
                        <PowerPlanCard titleOverride={isAdvanced ? "Power Plan" : "Energy & Speed"} />
                    )}

                    {/* System Hardware — OS tweaks + Boot grouped under one card (expert) */}
                    {showExpertSpeed && (
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
                    )}

                    {noSearch && <VmSandboxSection />}
                </div>

                {/* RIGHT COLUMN — power, security, actions */}
                <div className="column-stack">
                    {/* Power Management toggles */}
                    <ToggleSection
                        section={TWEAKS_SECTIONS[6]}
                        toggles={TWEAKS_TOGGLES}
                        onToggled={handlePostToggle}
                        searchQuery={searchQuery}
                    />

                    {/* Security & Apps + Defender Auditor (expert) */}
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

                    {/* Exploit Protection — DEP/ASLR/CFG/SEHOP + anti-acquisition hardening */}
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

                    {/* GPU Vendor Tweaks (advanced only) */}
                    {isAdvanced && (
                        <ToggleSection
                            section={TWEAKS_SECTIONS[5]}
                            toggles={gpuFilteredToggles}
                            onToggled={handlePostToggle}
                            searchQuery={searchQuery}
                        />
                    )}

                    {/* Context Menu Integration (expert) */}
                    {showExpertSpeed && (
                        <ContextMenuIntegrationCard
                            isAdvanced={isAdvanced}
                            searchQuery={searchQuery}
                        />
                    )}

                    {/* SFC/DISM repair, Windows Update repair, and defrag moved to
                        Maintenance → Repair & hygiene (OsRepairCard). They were
                        "System Maintenance"/"Deep Fix" here, colliding with
                        Maintenance's unrelated "System repair" tab. */}
                </div>
            </div>

            {/* Users/Tasks/Services/Conceal manager workbench moved to
                Maintenance → Startup & drivers (merged with Startup apps) —
                see StartupDriverTools.tsx. */}

            {(!hasResults && !noSearch) && (
                <GlobalSearchNoResults searchQuery={searchQuery} currentPanelId="tweaks" />
            )}
        </div>
    );
}
