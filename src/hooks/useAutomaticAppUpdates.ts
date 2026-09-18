import { useEffect, useRef } from "react";
import useBackend from "./useBackend";
import { useAppState } from "../context/AppContext";
import {
  MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS,
  clearAutomaticAppUpdateAttempt,
  getAutomaticAppUpdateCandidates,
  recordAutomaticAppUpdateAttempt,
} from "../lib/automaticAppUpdates";
import { claimFreeAppUpdates, clearAppUpdatesQueued, isAppUpdateQueued } from "../lib/appUpdateQueue";
import { releasePackageOperation, waitForPackageOperation } from "../lib/packageOperationLock";
import type { AppAutoUpdateAttempt } from "../types/settings";

/**
 * Runs the opt-in machine app-update policy after an inventory snapshot arrives.
 * It deliberately does not install Winget or request elevation: automatic work
 * must not turn a passive setting into a surprise system change. An elevated
 * WinCommander session with an already-installed package manager is required.
 */
export default function useAutomaticAppUpdates(): void {
  const {
    appSettings,
    systemInfo,
    patchAppSettings,
    runAppInventoryScan,
  } = useAppState();
  const { testWingetInstalled, upgradeApp } = useBackend();
  const handledInventoryRef = useRef<string | null>(null);
  const livePolicyRef = useRef({ enabled: false, ignored: new Set<string>() });

  const enabled = appSettings?.ideal.apps.autoUpdate === true;
  const ignoredFindingIds = appSettings?.app.ignoredFindingIds ?? [];
  useEffect(() => {
    livePolicyRef.current = { enabled, ignored: new Set(ignoredFindingIds) };
  }, [enabled, ignoredFindingIds]);

  const inventory = appSettings?.current.apps.inventory;
  const inventoryKey = inventory?.lastScanAt ?? null;

  useEffect(() => {
    // The inventory key avoids re-running after each retry-ledger write. A new
    // scan (manual, scheduled, or post-upgrade) is the only thing that opens a
    // fresh automatic pass.
    if (!inventory || !inventoryKey || handledInventoryRef.current === inventoryKey) return;
    if (!enabled || systemInfo?.isAdmin !== true) return;

    const attempts = appSettings?.current.apps.autoUpdateAttempts ?? {};
    const candidates = getAutomaticAppUpdateCandidates({
      pendingUpdates: inventory.pendingUpdates ?? [],
      ignoredFindingIds,
      attemptsById: attempts,
      manifestOnly: appSettings?.ideal.apps.autoUpdateManifestOnly === true,
    });
    handledInventoryRef.current = inventoryKey;
    if (candidates.length === 0) return;

    let cancelled = false;
    const run = async () => {
      await waitForPackageOperation();
      const claimed: string[] = [];
      try {
        if (cancelled || !livePolicyRef.current.enabled) return;
        const winget = await testWingetInstalled();
        if (!winget.success || winget.data?.status !== "installed") return;

        let currentAttempts: Record<string, AppAutoUpdateAttempt> = {
          ...(appSettings?.current.apps.autoUpdateAttempts ?? {}),
        };
        for (const update of candidates) {
          const id = update.id.trim();
          if (!id || cancelled || !livePolicyRef.current.enabled) break;
          if (livePolicyRef.current.ignored.has(`app-update:${id}`) || isAppUpdateQueued(id)) continue;

          const mine = claimFreeAppUpdates([id]);
          if (mine.length === 0) continue;
          claimed.push(...mine);

          let succeeded = false;
          while (!cancelled && livePolicyRef.current.enabled && !succeeded) {
            const prior = currentAttempts[id];
            const sameVersion = prior?.version === (update.latestVersion?.trim() || null);
            if (sameVersion && prior.attempts >= MAX_AUTOMATIC_APP_UPDATE_ATTEMPTS) break;

            // Write BEFORE starting the package command. A crash/relaunch can
            // therefore never create an unlimited retry loop.
            currentAttempts = recordAutomaticAppUpdateAttempt(currentAttempts, update);
            try {
              await patchAppSettings({ current: { apps: { autoUpdateAttempts: currentAttempts } } });
            } catch (error) {
              console.warn("Automatic app update skipped because retry state could not be saved.", error);
              break;
            }

            const result = await upgradeApp(id);
            if (result.success) {
              succeeded = true;
              currentAttempts = clearAutomaticAppUpdateAttempt(currentAttempts, id);
              try {
                await patchAppSettings({ current: { apps: { autoUpdateAttempts: currentAttempts } } });
              } catch (error) {
                console.warn("Automatic app update completed but retry state could not be cleared.", error);
              }
            }
          }
        }
        // Refresh exactly once after the batch so successful updates disappear
        // and a future version can be considered on its own inventory snapshot.
        if (!cancelled) await runAppInventoryScan(true);
      } finally {
        clearAppUpdatesQueued(claimed);
        releasePackageOperation();
      }
    };
    void run();
    return () => { cancelled = true; };
  }, [appSettings, enabled, ignoredFindingIds, inventory, inventoryKey, patchAppSettings, runAppInventoryScan, systemInfo?.isAdmin, testWingetInstalled, upgradeApp]);
}
