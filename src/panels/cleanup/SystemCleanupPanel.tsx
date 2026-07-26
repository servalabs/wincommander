// Recreated System Cleanup panel: one panel-owned scroller, preserved layout,
// and the existing backend hook contracts.
import { Icon } from "@/components/ui/bp";
import { useEffect, useState } from "react";
import useEntitlements from "../../hooks/useEntitlements";
import useProInstall from "../../hooks/useProInstall";
import SectionCard from "../../components/shared/SectionCard";
import UniversalCallout from "../../components/shared/UniversalCallout";
import TraceDetailDialog from "../../components/shared/TraceDetailDialog";
import PanelHeader from "../../components/shared/PanelHeader";
import DriveWipeDialog from "./DriveWipeDialog";
import CleanupCategoryGrid from "./CleanupCategoryGrid";
import CleanupActionsMonitoring from "./CleanupActionsMonitoring";
import { useCleanupScan } from "./useCleanupScan";
import { useCleanupLegacyDialogs } from "./useCleanupLegacyDialogs";

export default function SystemCleanupPanel() {
    const hasSafeguards = true;
    // A1-d — Investigator mode UI flip.
    //   When isInvestigator is true, render a top-banner explaining the
    //   mode, and the backend (A1-c) refuses every Clear-*/Erase-*/Remove-*
    //   command at the dispatch layer. The toast handler in useBackend
    //   surfaces the refusal so the user sees a clear error if they
    //   manage to click a Clear button anyway.
    //
    //   KT: the active card/detail path passes `clearDisabled` into
    //   CleanupTraceCard and omits TraceDetailDialog.onClear in review
    //   mode. Older per-category dialog JSX still exists in
    //   useCleanupLegacyDialogs.tsx as migration ballast, but card clicks
    //   no longer open it.
    const { hasPaid, isLoading: entitlementsLoading, isInvestigator } = useEntitlements();
    const { isInstalled: proInstalled } = useProInstall();

    const [driveWipeOpen, setDriveWipeOpen] = useState(false);
    const [otherDetail, setOtherDetail] = useState<{ catId: string; label: string; icon: string; color: string } | null>(null);
    const [traceDetailCatId, setTraceDetailCatId] = useState<string | null>(null);
    const [combinedDetail, setCombinedDetail] = useState<{ catId: string } | null>(null);

    const scan = useCleanupScan({
        schedulesEnabled: hasPaid && !isInvestigator,
        entitlementsReady: !entitlementsLoading,
        migrationEnabled: hasPaid && proInstalled && !isInvestigator,
    });
    const { cardDataMap, allTraceCategories, handleCardLoad, handleCardClear, handleOtherUserClear, handleCardClearAllUsers, otherUserDataMap, combinedDataMap, allUsersRaw, selectedDisplay, canSwitchUsers } = scan;

    const { openers, dialogs: legacyDialogs } = useCleanupLegacyDialogs(cardDataMap);

    // Most cards use the normalized detail dialog. Wi-Fi and Browser Audit
    // retain their bespoke dialogs because they support per-profile erasure.
    const openSharedDetails = (catId: string) => setTraceDetailCatId(catId);
    const detailOpenerMap: Record<string, (() => void) | undefined> = Object.fromEntries(
        allTraceCategories
            .filter((category) => !category.actionOnly && !!category.getDataKey)
            .map((category) => [category.id, () => openSharedDetails(category.id)]),
    );
    Object.assign(detailOpenerMap, {
        shellBags: () => openSharedDetails('shellBags'),
        usbHistory: () => openSharedDetails('usbHistory'),
        dnsCache: () => openSharedDetails('dnsCache'),
        execCache: () => openSharedDetails('execCache'),
        clipboardHistory: () => openSharedDetails('clipboardHistory'),
        wlanProfiles: () => openers.handleWlanProfiles(),
        btDevices: () => openSharedDetails('btDevices'),
        netDrives: () => openSharedDetails('netDrives'),
        processIntel: () => openSharedDetails('processIntel'),
        eventLogs: () => openSharedDetails('eventLogs'),
        srumData: () => openSharedDetails('srumData'),
        psHistory: () => openSharedDetails('psHistory'),
        recentFiles: () => openSharedDetails('recentFiles'),
        rdpHistory: () => openSharedDetails('rdpHistory'),
        connectivityHistory: () => openSharedDetails('connectivityHistory'),
        jumpLists: () => openSharedDetails('jumpLists'),
        browserFootprints: () => openers.handleBrowserFootprints(),
        prefetchFiles: () => openSharedDetails('prefetchFiles'),
        shadowCopies: () => openSharedDetails('shadowCopies'),
        ntfsJournals: () => openSharedDetails('ntfsJournals'),
        amcache: () => openSharedDetails('amcache'),
        ntUserTraces: () => openSharedDetails('ntUserTraces'),
        notepadState: () => openSharedDetails('notepadState'),
        pcaDatabase: () => openSharedDetails('pcaDatabase'),
        crashDumps: () => openSharedDetails('crashDumps'),
        searchIndex: () => openSharedDetails('searchIndex'),
        printSpooler: () => openSharedDetails('printSpooler'),
        walFiles: () => openSharedDetails('walFiles'),
        recallDb: () => openSharedDetails('recallDb'),
        recycleBin: () => openSharedDetails('recycleBin'),
        webCache: () => openSharedDetails('webCache'),
        thumbnailDb: () => openSharedDetails('thumbnailDb'),
        notificationDb: () => openSharedDetails('notificationDb'),
        branchCache: () => openSharedDetails('branchCache'),
        eventTranscript: () => openSharedDetails('eventTranscript'),
        activitiesTimeline: () => openSharedDetails('activitiesTimeline'),
        rdpBitmapCache: () => openSharedDetails('rdpBitmapCache'),
        servicingLogs: () => openSharedDetails('servicingLogs'),
        deviceInstallLogs: () => openSharedDetails('deviceInstallLogs'),
        usageTraceLogs: () => openSharedDetails('usageTraceLogs'),
        defenderHistory: () => openSharedDetails('defenderHistory'),
    });
    const traceDetailCategory = traceDetailCatId ? allTraceCategories.find((cat) => cat.id === traceDetailCatId) : undefined;
    const traceDetailData = traceDetailCatId ? cardDataMap[traceDetailCatId] : undefined;
    const otherDetailCategory = otherDetail
        ? allTraceCategories.find((cat) => cat.id === otherDetail.catId)
        : undefined;

    // Close every open dialog when the Pro activation celebration fires so the
    // confetti overlay isn't buried under a trace-detail or legacy dialog.
    // (The legacy dialogs' own instance of this listener lives in
    // useCleanupLegacyDialogs.tsx — both fire off the same event.)
    useEffect(() => {
        const closeAll = () => {
            setTraceDetailCatId(null); setOtherDetail(null); setCombinedDetail(null);
        };
        window.addEventListener("commander-dismiss-dialogs", closeAll);
        return () => window.removeEventListener("commander-dismiss-dialogs", closeAll);
    }, []);

    return (
        <div data-cleanup-panel-root="true" className="relative h-full min-h-0 overflow-hidden">
        <div
            data-cleanup-scroll-root="true"
            className="custom-scrollbar h-full min-h-0 overflow-x-hidden overflow-y-auto"
            style={{
                // Async scan counts must not make Chromium counter-scroll while
                // the user is moving through this independently owned scroller.
                overflowAnchor: 'none',
                overscrollBehavior: 'contain',
                scrollbarGutter: 'stable',
                paddingBottom: 56,
            }}
        >
        <div className="panel-container cleanup-panel">
            <PanelHeader
                panelId="cleanup"
                title="System Cleanup"
                description="Find and clear the traces Windows keeps about you — then schedule recurring cleanups."
            />
            {isInvestigator && (
                <div
                    style={{
                        margin: '0 16px 12px 16px',
                        padding: '10px 14px',
                        borderRadius: 8,
                        background: 'var(--color-warning-dim)',
                        border: '1px solid var(--color-warning)',
                        color: 'var(--color-warning)',
                        fontSize: 12,
                        fontWeight: 600,
                        letterSpacing: '0.3px',
                        display: 'flex',
                        alignItems: 'center',
                        gap: 10,
                    }}
                >
                    <Icon icon="shield" size={16} />
                    <span>
                        REVIEW MODE — cleanup operations are disabled while this mode is active.
                    </span>
                </div>
            )}
            {/* Full-width stack: this panel's sections (Privacy Clean with its
                4-col card grid, one-time actions, user profiles, safeguards)
                each want the full content width. Using a 2-col/masonry
                container here crammed them into half width and broke the
                inner grids (fix 2026-06-09). */}
            <div className="flex flex-col gap-3">
                <div className="col-span-2 flex flex-col gap-4">
                        {!hasSafeguards && (
                            <SectionCard title="System Cleanup">
                                <UniversalCallout
                                    intent="primary"
                                    message="Cleanup keeps routine disk and temporary-file maintenance in one place. Privacy Clean tools only appear after the safeguards profile is enabled and unlocked."
                                />
                            </SectionCard>
                        )}

                        {/* Space reclamation lives in Maintenance → Storage &
                            files ("Reclaim disk space"). This panel owns privacy
                            and forensic trace erasure only. The old
                            `<DiskCleanupGranular />` render used to be here. */}

                        {hasSafeguards && (
                            <CleanupCategoryGrid
                                scan={scan}
                                isInvestigator={isInvestigator}
                                schedulesEnabled={hasPaid && !isInvestigator}
                                onRequestScheduleAccess={isInvestigator
                                    ? undefined
                                    : () => {
                                        if (entitlementsLoading) return;
                                        window.dispatchEvent(new CustomEvent("license-gate-open", {
                                        detail: { tab: "buy", featureLabel: "Scheduled Auto-Clean" },
                                        }));
                                    }}
                                detailOpenerMap={detailOpenerMap}
                                onDriveWipe={() => setDriveWipeOpen(true)}
                                onOpenCombinedDetail={(catId) => setCombinedDetail({ catId })}
                                onOpenOtherDetail={(detail) => setOtherDetail(detail)}
                            />
                        )}

                        {/* One-Time Actions + System Monitoring, merged into one card and
                            placed just before Lockdown (owner). One-shot destructive ops on
                            top; read-only monitors below. */}
                        <CleanupActionsMonitoring
                            cardDataMap={cardDataMap}
                            isInvestigator={isInvestigator}
                            detailOpenerMap={detailOpenerMap}
                            handleCardLoad={handleCardLoad}
                            handleCardClear={handleCardClear}
                            onDriveWipe={() => setDriveWipeOpen(true)}
                        />

                </div>
            </div>
            {/* Dialogs */}

            {traceDetailCategory && (
                <TraceDetailDialog
                    category={traceDetailCategory}
                    isOpen={!!traceDetailCategory}
                    count={traceDetailData?.count ?? 0}
                    items={traceDetailData?.items ?? []}
                    clearing={traceDetailData?.clearing ?? false}
                    onClose={() => setTraceDetailCatId(null)}
                    onClear={traceDetailCategory.clearDataKey && !isInvestigator
                        ? () => {
                            setTraceDetailCatId(null);
                            void handleCardClear(traceDetailCategory);
                        }
                        : undefined}
                    clearDisabled={(traceDetailData?.count ?? 0) <= 0}
                    clearLabel="Clear"
                />
            )}

            {otherDetail && otherDetailCategory && (
                <TraceDetailDialog
                    category={{
                        ...otherDetailCategory,
                        label: `${otherDetailCategory.label} — ${selectedDisplay}`,
                    }}
                    isOpen={!!otherDetailCategory}
                    count={otherUserDataMap[otherDetail.catId]?.count ?? 0}
                    items={otherUserDataMap[otherDetail.catId]?.items ?? []}
                    clearing={otherUserDataMap[otherDetail.catId]?.clearing ?? false}
                    onClose={() => setOtherDetail(null)}
                    onClear={!isInvestigator && canSwitchUsers
                        ? () => {
                            const cat = allTraceCategories.find(c => c.id === otherDetail.catId);
                            setOtherDetail(null);
                            if (cat) void handleOtherUserClear(cat);
                        }
                        : undefined}
                    clearDisabled={(otherUserDataMap[otherDetail.catId]?.count ?? 0) <= 0}
                    clearLabel={`Clear for ${selectedDisplay}`}
                />
            )}

            {/* Combined "All users" detail — total count + per-user breakdown (#7). */}
            {combinedDetail && (() => {
                const cat = allTraceCategories.find(c => c.id === combinedDetail.catId);
                if (!cat) return null;
                const d = combinedDataMap[combinedDetail.catId];
                const groupedItems = allUsersRaw
                    .map(u => {
                        const userData = u.categories[combinedDetail.catId];
                        return {
                            title: u.displayName || u.username,
                            count: userData?.count ?? 0,
                            items: userData?.items ?? [],
                        };
                    })
                    .filter(group => group.count > 0 || group.items.length > 0);
                return (
                    <TraceDetailDialog
                        category={{ ...cat, label: `${cat.label} — All users` }}
                        isOpen={true}
                        count={d?.count ?? 0}
                        items={[]}
                        groupedItems={groupedItems}
                        clearing={false}
                        onClose={() => setCombinedDetail(null)}
                        onClear={!isInvestigator && canSwitchUsers
                            ? () => { setCombinedDetail(null); void handleCardClearAllUsers(cat); }
                            : undefined}
                        clearDisabled={(d?.count ?? 0) <= 0}
                        clearLabel="Clear for all users"
                    />
                );
            })()}

            <DriveWipeDialog open={driveWipeOpen} onClose={() => setDriveWipeOpen(false)} />

            {legacyDialogs}
        </div>
        </div>
        <div data-cleanup-overlay-root="true" className="pointer-events-none absolute inset-0" />
        </div>
    );
}
