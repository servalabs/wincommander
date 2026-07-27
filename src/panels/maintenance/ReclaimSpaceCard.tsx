import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import { RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { APP_CACHE_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";

/**
 * Single space-reclaim surface. Storage & files used to render two side-by-side
 * cards — Get-DiskCleanupScan and routine_cleaner — that cleaned nine of the
 * same paths through different backends, then briefly a Segmented toggle that
 * showed only one scope at a time. Both problems are gone: the two scopes are
 * explicitly non-overlapping (Windows-owned vs application-owned, see
 * APP_CACHE_CLEANUP_CATEGORIES) and are now shown side by side again so a user
 * doesn't have to flip a switch to see the other one.
 */
export default function ReclaimSpaceCard() {
  return (
    <Card data-tour="maintenance-disk-cleanup">
      <CardHeader>
        <CardTitle className="text-lg font-bold tracking-tight">Reclaim disk space</CardTitle>
        <CardDescription>Preview before anything is deleted. Windows-owned and app-owned storage are cleared through two non-overlapping scopes — the two never touch the same path.</CardDescription>
      </CardHeader>
      <CardContent className="min-w-0">
        <div className="maintenance-reclaim-grid">
          <div className="maintenance-reclaim-scope">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-[var(--text-mute)]">Windows storage</h3>
            <p className="text-xs text-[var(--text-mute)]">Windows owns these paths: temp, update and delivery caches, Prefetch, thumbnails, crash dumps, error reports, Recycle Bin, and a previous Windows installation. Needs Administrator.</p>
            <DiskCleanupGranular />
          </div>
          <div className="maintenance-reclaim-scope">
            <h3 className="text-[11px] font-semibold uppercase tracking-wider text-[var(--text-mute)]">App &amp; browser caches</h3>
            <p className="text-xs text-[var(--text-mute)]">Applications own these paths: browser, application, game-launcher, and SQLite caches. Every target is previewable per file, cancellable mid-run, and needs no elevation for user-owned data.</p>
            <RoutineCleanerPanel categories={APP_CACHE_CLEANUP_CATEGORIES} />
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
