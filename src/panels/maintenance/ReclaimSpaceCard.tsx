import { useState } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import { RoutineCleanerHeaderActions, RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { APP_CACHE_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";
import { useRoutineCleaner } from "./useRoutineCleaner";
import { useCleanupScan } from "../cleanup/useCleanupScan";
import WindowsStorageCacheTools from "./WindowsStorageCacheTools";
import useEntitlements from "../../hooks/useEntitlements";

/** Kept as a compatibility export for callers that need both reclaim scopes. */
export default function ReclaimSpaceCard() {
  return (
    <div className="maintenance-reclaim-grid">
      <WindowsStorageCard />
      <AppBrowserCacheCard />
    </div>
  );
}

export function WindowsStorageCard() {
  return (
    <Card className="maintenance-storage-card maintenance-storage-overview-card">
      <CardHeader>
        <CardTitle>Windows storage</CardTitle>
        <CardDescription>Review Windows-managed storage separately from application caches.</CardDescription>
      </CardHeader>
      <CardContent className="maintenance-storage-overview-content">
        <section data-tour="maintenance-disk-cleanup" aria-label="Windows storage cleanup" className="maintenance-storage-section">
          <DiskCleanupGranular />
        </section>
        <details className="maintenance-secondary-scope">
          <summary>Additional Windows caches</summary>
          <div className="pt-4"><AdditionalWindowsCaches /></div>
        </details>
      </CardContent>
    </Card>
  );
}

export function AppBrowserCacheCard() {
  const cleaner = useRoutineCleaner(APP_CACHE_CLEANUP_CATEGORIES);
  const [confirmOpen, setConfirmOpen] = useState(false);

  return (
    <Card className="maintenance-storage-card maintenance-app-cache-card">
      <CardHeader className="maintenance-cache-card-header">
        <div className="maintenance-cache-heading min-w-0">
          <CardTitle>Deep Cleanup</CardTitle>
          <CardDescription>Inspect supported browser, app and developer caches in their known locations. Privacy records and application activity are reviewed separately in System Cleanup.</CardDescription>
        </div>
        <RoutineCleanerHeaderActions cleaner={cleaner} onRequestClean={() => setConfirmOpen(true)} />
      </CardHeader>
      <CardContent className="maintenance-app-cache-content"><RoutineCleanerPanel cleaner={cleaner} categories={APP_CACHE_CLEANUP_CATEGORIES} confirmOpen={confirmOpen} onConfirmOpenChange={setConfirmOpen} /></CardContent>
    </Card>
  );
}

function AdditionalWindowsCaches() {
  const { isInvestigator, isLoading } = useEntitlements();
  const scan = useCleanupScan({
    schedulesEnabled: false,
    entitlementsReady: !isLoading,
    migrationEnabled: false,
    auxiliaryStateEnabled: false,
  });

  return (
    <WindowsStorageCacheTools
      cardDataMap={scan.cardDataMap}
      isInvestigator={isInvestigator || isLoading}
      handleCardLoad={scan.handleCardLoad}
      handleCardClear={scan.handleCardClear}
    />
  );
}
