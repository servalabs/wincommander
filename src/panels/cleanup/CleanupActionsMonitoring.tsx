// src/panels/cleanup/CleanupActionsMonitoring.tsx
// The "One-Time Actions & Monitoring" section: one-shot destructive actions
// on the left, read-only system monitors on the right. Extracted verbatim
// from src/panels/cleanup/index.tsx — pure move, no behavior change.
import { Icon } from "@/components/ui/bp";
import { Tooltip, TooltipContent, TooltipTrigger } from "../../components/ui/tooltip";
import type { ReactNode } from "react";
import SectionCard from "../../components/shared/SectionCard";
import CleanupTraceCard from "../../components/cleanup/CleanupTraceCard";
import RunOnceButton from "../../components/cleanup/RunOnceButton";
import { ACTION_CATEGORIES, VIEW_ONLY_CATEGORIES, type CleanupCategory } from "./cleanupCategories";
import type { CardData } from "./useCleanupScan";
import { useCleanupDisplay } from "./cleanupDisplay";
import { cleanupDisplayName } from "./cleanupDisplayModel";

interface Props {
    cardDataMap: Record<string, CardData>;
    isInvestigator: boolean;
    detailOpenerMap: Record<string, (() => void) | undefined>;
    handleCardLoad: (cat: CleanupCategory) => void;
    handleCardClear: (cat: CleanupCategory, onDriveWipe?: () => void) => void;
    onDriveWipe: () => void;
    diskActions: ReactNode;
    repairActions: ReactNode;
}

export default function CleanupActionsMonitoring({
    cardDataMap,
    isInvestigator,
    detailOpenerMap,
    handleCardLoad,
    handleCardClear,
    onDriveWipe,
    diskActions,
    repairActions,
}: Props) {
    const displayMode = useCleanupDisplay();
    return (
        <SectionCard title="One-Time Actions & Monitoring" style={{ marginBottom: '16px' }}>
            <div className="flex flex-col gap-5">
                {/* Keep on-demand actions and their review in one card, with the
                    read-only review directly below the actions it relates to. */}
                <section data-tour="cleanup-one-time-actions" aria-labelledby="cleanup-one-time-actions-heading">
                    <div className="flex items-center gap-3 mb-3 py-1">
                        <h3 id="cleanup-one-time-actions-heading" className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>One-Time Actions</h3>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                        <span className="text-[9px] italic opacity-60 whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>run on demand</span>
                    </div>
                    <div className="cleanup-action-grid grid grid-cols-1 gap-2 md:grid-cols-2 xl:grid-cols-4">
                        {ACTION_CATEGORIES.map(cat => {
                            const d = cardDataMap[cat.id] || { count: -1, items: [], loading: false, clearing: false };
                            const label = cleanupDisplayName(cat, displayMode ?? "advanced");
                            return (
                                <div
                                    key={cat.id}
                                    className="flex min-w-0 flex-wrap items-center gap-2 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-bg-card)] px-3 py-2.5"
                                >
                                    <div
                                        className="flex-shrink-0 flex items-center justify-center"
                                        style={{ width: 30, height: 30, borderRadius: 6, background: `${cat.color}1e` }}
                                    >
                                        <Icon icon={cat.icon as any} size={15} style={{ color: cat.color }} />
                                    </div>
                                    <div className="flex min-w-[100px] flex-1 flex-col">
                                        <span className="font-bold" style={{ fontSize: 12, color: 'var(--color-text-primary)' }}>
                                            {label}
                                        </span>
                                    </div>
                                    <Tooltip><TooltipTrigger asChild><button type="button" className="grid h-8 w-8 shrink-0 place-items-center rounded" aria-label={`About ${label}`}><Icon icon="info-sign" size={14} /></button></TooltipTrigger><TooltipContent side="bottom" className="max-w-[260px] text-xs">{cat.description}{cat.confirmMessage && <p>{cat.confirmMessage}</p>}</TooltipContent></Tooltip>
                                    <RunOnceButton
                                        isRunning={d.clearing}
                                        disabled={isInvestigator}
                                        onClick={() => handleCardClear(cat, cat.id === 'unallocatedErase' ? onDriveWipe : undefined)}
                                        className="shrink-0"
                                        actionLabel={cat.label}
                                    />
                                </div>
                            );
                        })}
                        {diskActions}
                    </div>
                    <div className="flex items-center gap-3 mb-3 mt-4 py-1">
                        <h4 className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>Windows repair</h4>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                    </div>
                    <div className="cleanup-action-grid grid grid-cols-1 gap-2 md:grid-cols-2 xl:grid-cols-4">{repairActions}</div>
                </section>

                {/* System Monitoring */}
                <section data-tour="cleanup-process-review" aria-labelledby="cleanup-system-monitoring-heading">
                    <div className="flex items-center gap-3 mb-3 py-1">
                        <h3 id="cleanup-system-monitoring-heading" className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>System Monitoring</h3>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                        <span className="text-[9px] italic opacity-60 whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>view only</span>
                    </div>
                    <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
                        {VIEW_ONLY_CATEGORIES.map(cat => {
                            const d = cardDataMap[cat.id] || { count: -1, items: [], loading: false, clearing: false };
                            return (
                                <CleanupTraceCard
                                    key={cat.id}
                                    category={cat}
                                    count={d.count}
                                    preview={d.items}
                                    loading={d.loading}
                                    clearing={d.clearing}
                                    error={d.error}
                                    rawData={d.raw}
                                    onClear={() => handleCardClear(cat)}
                                    clearDisabled={isInvestigator}
                                    onViewDetails={detailOpenerMap[cat.id]}
                                    onLoad={() => handleCardLoad(cat)}
                                />
                            );
                        })}
                    </div>
                </section>
            </div>
        </SectionCard>
    );
}
