import { useEffect, useState } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../components/ui/card";
import { Segmented } from "../../components/ui/segmented";
import DiskCleanupGranular from "../../components/tweaks/managers/DiskCleanupGranular";
import { RoutineCleanerPanel } from "./RoutineCleanerPanel";
import { APP_CACHE_CLEANUP_CATEGORIES } from "./routineCleanerHelpers";

type ReclaimScope = "windows" | "apps";

const SCOPE_HINT: Record<ReclaimScope, string> = {
  windows: "Windows owns these paths: temp, update and delivery caches, Prefetch, thumbnails, crash dumps, error reports, Recycle Bin, and a previous Windows installation. Needs Administrator.",
  apps: "Applications own these paths: browser, application, game-launcher, and SQLite caches. Every target is previewable per file, cancellable mid-run, and needs no elevation for user-owned data.",
};

/**
 * Single space-reclaim surface. Storage & files used to render two side-by-side
 * cards — Get-DiskCleanupScan and routine_cleaner — that cleaned nine of the
 * same paths through different backends. They are now one card with two
 * explicitly non-overlapping scopes: Windows-owned vs application-owned.
 */
export default function ReclaimSpaceCard() {
  const [scope, setScope] = useState<ReclaimScope>("windows");

  // The dashboard's reclaimable-space chip dispatches `open-disk-cleanup`
  // expecting the Windows scope on screen; snap back to it so the deep link
  // never lands on the app-cache scope.
  useEffect(() => {
    const showWindowsScope = () => setScope("windows");
    window.addEventListener("open-disk-cleanup", showWindowsScope);
    return () => window.removeEventListener("open-disk-cleanup", showWindowsScope);
  }, []);

  return (
    <Card data-tour="maintenance-disk-cleanup">
      <CardHeader>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <CardTitle>Reclaim disk space</CardTitle>
            <CardDescription>Preview before anything is deleted. Pick whose storage you are clearing — the two scopes never touch the same path.</CardDescription>
          </div>
          <Segmented
            size="sm"
            value={scope}
            onValueChange={(value) => setScope(value as ReclaimScope)}
            options={[{ value: "windows", label: "Windows storage" }, { value: "apps", label: "App & browser caches" }]}
          />
        </div>
      </CardHeader>
      <CardContent className="flex min-w-0 flex-col gap-3">
        <p className="text-xs text-[var(--text-mute)]">{SCOPE_HINT[scope]}</p>
        {scope === "windows" ? <DiskCleanupGranular /> : <RoutineCleanerPanel categories={APP_CACHE_CLEANUP_CATEGORIES} />}
      </CardContent>
    </Card>
  );
}
