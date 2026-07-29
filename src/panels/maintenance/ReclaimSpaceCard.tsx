import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import { FileHygieneTools } from "./FileHygieneTools";
import { RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { APP_CACHE_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";

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
  return (
    <Card className="maintenance-storage-card maintenance-app-cache-card">
      <CardHeader>
        <CardTitle>App &amp; browser cache</CardTitle>
        <CardDescription>Preview browser, application, game-launcher, and SQLite cache data before cleaning. Windows storage is not included here.</CardDescription>
      </CardHeader>
      <CardContent className="maintenance-app-cache-content"><RoutineCleanerPanel categories={APP_CACHE_CLEANUP_CATEGORIES} /></CardContent>
    </Card>
  );
}
