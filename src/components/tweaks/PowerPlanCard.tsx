import { Icon, IconName } from "@/components/ui/bp";
import { useCallback, useEffect, useState } from "react";
import SectionCard from "../shared/SectionCard";
import useBackend from "../../hooks/useBackend";
import { useAppState } from "../../context/AppContext";
import { showSuccess, showError } from "../../utils/toast";
import type { PowerPlanMode } from "../../types/settings";

interface PlanOption {
    id: PowerPlanMode;
    title: string;
    tag: string;
    icon: IconName;
    /** CSS modifier class on `.power-plan-card` */
    variant: "powersaver" | "balanced" | "performance" | "ultimate";
    /** Default for the experience copy */
    description: string;
}

const PLANS: PlanOption[] = [
    {
        id: "powersaving",
        title: "Power Saver",
        tag: "Prioritizes Battery",
        icon: "flash",
        variant: "powersaver",
        description: "POWER SAVER (Prioritizes Battery)",
    },
    {
        id: "balanced",
        title: "Balanced",
        tag: "Standard Behavior",
        icon: "dashboard",
        variant: "balanced",
        description: "BALANCED (Standard Behavior)",
    },
    {
        id: "performance",
        title: "Performance",
        tag: "Prioritizes Speed",
        icon: "rocket",
        variant: "performance",
        description: "PERFORMANCE (Prioritizes Speed)",
    },
    {
        id: "ultimate",
        title: "Ultimate",
        tag: "AC only · max responsiveness",
        icon: "rocket-slant",
        variant: "ultimate",
        description: "ULTIMATE PERFORMANCE (disables all idle states)",
    },
];

/**
 * Single source of truth for the active Windows power plan. Wraps
 * Set-PowerPlan (tweaks/maintenance) which handles the 4 modes including
 * the lazy duplicate-scheme step for `ultimate` — see
 * src-tauri/.../tweaks/maintenance.ps1 Set-PowerPlan.
 *
 * Previously there were two UIs for this setting (the 3-card picker and
 * a standalone "Ultimate Performance Power Plan" toggle). The toggle was
 * removed and Ultimate became a 4th radio here.
 */
export default function PowerPlanCard({ titleOverride }: { titleOverride?: string }) {
    const { appSettings, patchAppSettings } = useAppState();
    const { setPowerPlan } = useBackend();
    const [selected, setSelected] = useState<PowerPlanMode | null>(
        (appSettings?.ideal?.tweaks?.powerPlan ?? null) as PowerPlanMode | null
    );
    const [pending, setPending] = useState<PowerPlanMode | null>(null);

    useEffect(() => {
        const plan = (appSettings?.ideal?.tweaks?.powerPlan ?? null) as PowerPlanMode | null;
        setSelected(plan);
    }, [appSettings?.ideal?.tweaks?.powerPlan]);

    const choose = useCallback(async (plan: PlanOption) => {
        // Optimistic UI; revert on failure so the user sees what actually applied.
        const prior = selected;
        setSelected(plan.id);
        setPending(plan.id);
        try {
            await setPowerPlan(plan.id);
            await patchAppSettings({ ideal: { tweaks: { powerPlan: plan.id } } });
            showSuccess(`Power Plan: ${plan.description}`);
        } catch (e: any) {
            setSelected(prior);
            showError(
                plan.id === "ultimate"
                    ? "Ultimate Performance isn't available on this Windows SKU."
                    : (e?.message || "Could not change power plan"),
            );
        } finally {
            setPending(null);
        }
    }, [selected, setPowerPlan, patchAppSettings]);

    return (
        <SectionCard title={titleOverride ?? "Power Plan"}>
            <div className="flex flex-col gap-4 py-2">
                <div className="flex flex-col gap-2">
                    <span className="text-[11px] font-bold text-[var(--color-text-muted)] uppercase tracking-wider">
                        Select Active Profile
                    </span>
                    <div className={`power-plan-switch power-plan-switch--${Math.max(0, PLANS.findIndex((p) => p.id === selected))}`}>
                        <span className="power-plan-switch__thumb" aria-hidden="true" />
                        {PLANS.map(plan => {
                            const active = selected === plan.id;
                            const loading = pending === plan.id;
                            return (
                                <button
                                    key={plan.id}
                                    type="button"
                                    className={`power-plan-option ${plan.variant} ${active ? "active" : ""} ${loading ? "loading" : ""}`}
                                    onClick={() => !loading && choose(plan)}
                                    disabled={!!pending && pending !== plan.id}
                                    aria-pressed={active}
                                >
                                    <Icon icon={loading ? "refresh" : plan.icon} size={14} className={loading ? "power-plan-option__spin" : ""} />
                                    <span className="plan-title">{plan.title}</span>
                                    <span className="plan-tag">{plan.tag}</span>
                                </button>
                            );
                        })}
                    </div>
                </div>
            </div>
        </SectionCard>
    );
}
