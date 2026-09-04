// src/panels/cleanup/CleanupActionsMonitoring.tsx
// The "One-Time Actions & Monitoring" section: one-shot destructive actions
// on the left, read-only system monitors on the right. Extracted verbatim
// from src/panels/cleanup/index.tsx — pure move, no behavior change.
import { Icon } from "@/components/ui/bp";
import type { ReactNode } from "react";
import SectionCard from "../../components/shared/SectionCard";
import CleanupTraceCard from "../../components/cleanup/CleanupTraceCard";
import RunOnceButton from "../../components/cleanup/RunOnceButton";
import { ACTION_CATEGORIES, VIEW_ONLY_CATEGORIES, type CleanupCategory } from "./cleanupCategories";
import type { CardData } from "./useCleanupScan";
import RoutineHygieneCard from "./RoutineHygieneCard";

interface Props {
    cardDataMap: Record<string, CardData>;
    isInvestigator: boolean;
    detailOpenerMap: Record<string, (() => void) | undefined>;
    handleCardLoad: (cat: CleanupCategory) => void;
    handleCardClear: (cat: CleanupCategory, onDriveWipe?: () => void) => void;
    onDriveWipe: () => void;
    repairActions: ReactNode;
}

export default function CleanupActionsMonitoring({
    cardDataMap,
    isInvestigator,
    detailOpenerMap,
    handleCardLoad,
    handleCardClear,
    onDriveWipe,
    repairActions,
}: Props) {
    return (
        <SectionCard title="One-Time Actions & Monitoring" style={{ marginBottom: '32px' }}>
            <div className="flex flex-col gap-8">
                <RoutineHygieneCard
                    cardDataMap={cardDataMap}
                    isInvestigator={isInvestigator}
                    handleCardLoad={handleCardLoad}
                    handleCardClear={handleCardClear}
                />
                {/* Keep on-demand actions and their review in one card, with the
                    read-only review directly below the actions it relates to. */}
                <section data-tour="cleanup-one-time-actions" aria-labelledby="cleanup-one-time-actions-heading">
                    <div className="flex items-center gap-3 mb-6 py-3">
                        <h3 id="cleanup-one-time-actions-heading" className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>One-Time Actions</h3>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                        <span className="text-[9px] italic opacity-60 whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>run on demand</span>
                    </div>
                    <div className="grid grid-cols-1 gap-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-bg-secondary)] p-2 md:grid-cols-2 xl:grid-cols-3">
                        {/* Force SSD TRIM lives in OsRepairCard, rendered by SystemCleanupPanel
                            just below this section (Maintenance's old "Repair & hygiene" tab that
                            used to host it is gone, 2026-07) — ACTION_CATEGORIES no longer carries it. */}
                        {ACTION_CATEGORIES.map(cat => {
                            const d = cardDataMap[cat.id] || { count: -1, items: [], loading: false, clearing: false };
                            return (
                                <div
                                    key={cat.id}
                                    className="flex min-h-[104px] items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-bg-card)] px-3 py-2.5"
                                >
                                    <div
                                        className="flex-shrink-0 flex items-center justify-center"
                                        style={{ width: 30, height: 30, borderRadius: 6, background: `${cat.color}1e` }}
                                    >
                                        <Icon icon={cat.icon as any} size={15} style={{ color: cat.color }} />
                                    </div>
                                    <div className="flex flex-col min-w-0 flex-1">
                                        <span className="font-bold uppercase truncate" style={{ fontSize: 11, letterSpacing: '0.4px', color: 'var(--color-text-primary)' }}>
                                            {cat.label}
                                        </span>
                                        <span className="truncate" style={{ fontSize: 10.5, color: 'var(--color-text-muted)' }}>
                                            {cat.description}
                                        </span>
                                    </div>
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
                    </div>
                    <div className="flex items-center gap-3 mb-4 mt-6 py-2">
                        <h4 className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>Windows repair</h4>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                    </div>
                    {repairActions}
                </section>

                {/* System Monitoring */}
                <section data-tour="cleanup-process-review" aria-labelledby="cleanup-system-monitoring-heading">
                    <div className="flex items-center gap-3 mb-6 py-3">
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
