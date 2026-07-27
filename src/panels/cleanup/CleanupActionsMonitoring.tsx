// src/panels/cleanup/CleanupActionsMonitoring.tsx
// The "One-Time Actions & Monitoring" section: one-shot destructive actions
// on the left, read-only system monitors on the right. Extracted verbatim
// from src/panels/cleanup/index.tsx — pure move, no behavior change.
import { Button, Icon } from "@/components/ui/bp";
import SectionCard from "../../components/shared/SectionCard";
import CleanupTraceCard from "../../components/cleanup/CleanupTraceCard";
import { ACTION_CATEGORIES, VIEW_ONLY_CATEGORIES, type CleanupCategory } from "./cleanupCategories";
import type { CardData } from "./useCleanupScan";

interface Props {
    cardDataMap: Record<string, CardData>;
    isInvestigator: boolean;
    detailOpenerMap: Record<string, (() => void) | undefined>;
    handleCardLoad: (cat: CleanupCategory) => void;
    handleCardClear: (cat: CleanupCategory, onDriveWipe?: () => void) => void;
    onDriveWipe: () => void;
}

export default function CleanupActionsMonitoring({
    cardDataMap,
    isInvestigator,
    detailOpenerMap,
    handleCardLoad,
    handleCardClear,
    onDriveWipe,
}: Props) {
    return (
        <SectionCard title="One-Time Actions & Monitoring" style={{ marginBottom: '32px' }}>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '24px', alignItems: 'start' }}>
                {/* LEFT: One-Time Actions */}
                <div data-tour="cleanup-one-time-actions">
                    <div className="flex items-center gap-3 mb-6 py-3">
                        <span className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>One-Time Actions</span>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                        <span className="text-[9px] italic opacity-60 whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>run on demand</span>
                    </div>
                    <div className="flex flex-col gap-2">
                        {/* Force SSD TRIM moved to the Maintenance panel's Repair & Hygiene tab (2026-07) — ACTION_CATEGORIES no longer carries it. */}
                        {ACTION_CATEGORIES.map(cat => {
                            const d = cardDataMap[cat.id] || { count: -1, items: [], loading: false, clearing: false };
                            return (
                                <div
                                    key={cat.id}
                                    className="flex items-center gap-3 px-3 py-2.5 rounded-lg"
                                    style={{ background: 'var(--color-bg-secondary)', border: '1px dashed var(--color-border)' }}
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
                                    <Button
                                        small
                                        icon="play"
                                        text={d.clearing ? 'Running…' : 'Run'}
                                        intent="danger"
                                        loading={d.clearing}
                                        disabled={isInvestigator}
                                        onClick={() => handleCardClear(cat, cat.id === 'unallocatedErase' ? onDriveWipe : undefined)}
                                        style={{ fontSize: 10, fontWeight: 700, letterSpacing: '0.4px', flexShrink: 0 }}
                                    />
                                </div>
                            );
                        })}
                    </div>
                </div>

                {/* RIGHT: System Monitoring */}
                <div data-tour="cleanup-process-review">
                    <div className="flex items-center gap-3 mb-6 py-3">
                        <span className="text-[10px] font-semibold uppercase tracking-wider whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>System Monitoring</span>
                        <div className="flex-1 h-px bg-[var(--color-border)] opacity-50" />
                        <span className="text-[9px] italic opacity-60 whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>view only</span>
                    </div>
                    <div className="grid grid-cols-2 gap-4">
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
                </div>
            </div>
        </SectionCard>
    );
}
