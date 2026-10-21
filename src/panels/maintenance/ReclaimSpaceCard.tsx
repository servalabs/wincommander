import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import { RoutineCleanerHeaderActions, RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { APP_CACHE_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";
import { useRoutineCleaner } from "./useRoutineCleaner";
import { useCleanupScan } from "../cleanup/useCleanupScan";
import RoutineHygieneCard from "../cleanup/RoutineHygieneCard";
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
      </CardContent>
    </Card>
  );
}

export function AppBrowserCacheCard() {
  const cleaner = useRoutineCleaner(APP_CACHE_CLEANUP_CATEGORIES);

  return (
    <Card className="maintenance-storage-card maintenance-app-cache-card">
      <CardHeader className="flex-row items-start gap-3">
        <div className="min-w-0">
          <CardTitle>Application caches</CardTitle>
          <CardDescription>Browser caches, developer tools, applications, and game launchers. Review targets before cleaning.</CardDescription>
        </div>
        <RoutineCleanerHeaderActions cleaner={cleaner} />
      </CardHeader>
      <CardContent className="maintenance-app-cache-content"><RoutineCleanerPanel cleaner={cleaner} categories={APP_CACHE_CLEANUP_CATEGORIES} /></CardContent>
    </Card>
  );
}

/** Unique Windows cache actions share Maintenance with the file-based cleaners. */
export function RoutineCacheCleanupCard() {
  const { isInvestigator, isLoading } = useEntitlements();
  const scan = useCleanupScan({
    schedulesEnabled: false,
    entitlementsReady: !isLoading,
    migrationEnabled: false,
    auxiliaryStateEnabled: false,
  });

  return (
    <RoutineHygieneCard
      cardDataMap={scan.cardDataMap}
      isInvestigator={isInvestigator || isLoading}
      handleCardLoad={scan.handleCardLoad}
      handleCardClear={scan.handleCardClear}
    />
  );
}
