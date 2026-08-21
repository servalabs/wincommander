import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import { FileHygieneTools } from "./FileHygieneTools";
import { RoutineCleanerHeaderActions, RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { APP_CACHE_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";
import { useRoutineCleaner } from "./useRoutineCleaner";

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
        <CardTitle>Windows storage &amp; file hygiene</CardTitle>
        <CardDescription>Review Windows-managed storage, then choose the folders you want inspected for duplicate files or empty folders.</CardDescription>
      </CardHeader>
      <CardContent className="maintenance-storage-overview-content">
        <section data-tour="maintenance-disk-cleanup" aria-label="Windows storage cleanup" className="maintenance-storage-section">
          <DiskCleanupGranular />
        </section>
        <section aria-label="Folder inspector and file hygiene" className="maintenance-file-hygiene-section">
          <FileHygieneTools />
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
          <CardTitle>App &amp; browser cache</CardTitle>
          <CardDescription>Preview browser, application, game-launcher, and SQLite cache data before cleaning. Windows storage is not included here.</CardDescription>
        </div>
        <RoutineCleanerHeaderActions cleaner={cleaner} />
      </CardHeader>
      <CardContent className="maintenance-app-cache-content"><RoutineCleanerPanel cleaner={cleaner} categories={APP_CACHE_CLEANUP_CATEGORIES} /></CardContent>
    </Card>
  );
}
