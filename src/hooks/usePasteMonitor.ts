// src/hooks/usePasteMonitor.ts
//
// usePasteMonitor — drives the F-1 clipboard credential watcher off
// settings. Pattern mirrors useRdpIdleDisconnect: a single global hook
// in App.tsx reads the setting and asks the Rust side to start/stop +
// sync categories. The Privacy panel UI is just a setting patch; this
// hook does the rest. That way the setting is the single source of
// truth for "is the watcher running" and "which categories fire".
//
// Toast / native notification fires from the Rust side; the in-app
// listener for `paste-monitor-detected` lives in BackgroundPollers.tsx.
// The watcher itself is idempotent on the Rust side, so a duplicate
// `start_paste_monitor` call from a re-render is harmless.

import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PasteMonitorCategories } from "../types/settings";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

export interface PasteMonitorRustCategories {
  cloudApi: boolean;
  aiApi: boolean;
  devTools: boolean;
  paymentComms: boolean;
  keysAndCrypto: boolean;
  personalData: boolean;
  maliciousCommand: boolean;
  /** Homoglyph URLs, zero-width chars in code-context, bidi overrides. */
  unicode: boolean;
}

/** Default-everything-on. The Rust side has the same defaults — this is
 *  here so the React UI can render checkboxes against a stable shape
 *  even before the user has touched any of them. */
export const DEFAULT_PASTE_MONITOR_CATEGORIES: PasteMonitorRustCategories = {
  cloudApi: true,
  aiApi: true,
  devTools: true,
  paymentComms: true,
  keysAndCrypto: true,
  personalData: true,
  maliciousCommand: true,
  unicode: true,
};

/** Resolve a possibly-null/partial `PasteMonitorCategories` setting to
 *  the strict shape Rust expects. Missing fields fall back to default-on. */
export function resolveCategories(
  cats: PasteMonitorCategories | null | undefined,
): PasteMonitorRustCategories {
  return {
    cloudApi:         cats?.cloudApi         ?? DEFAULT_PASTE_MONITOR_CATEGORIES.cloudApi,
    aiApi:            cats?.aiApi            ?? DEFAULT_PASTE_MONITOR_CATEGORIES.aiApi,
    devTools:         cats?.devTools         ?? DEFAULT_PASTE_MONITOR_CATEGORIES.devTools,
    paymentComms:     cats?.paymentComms     ?? DEFAULT_PASTE_MONITOR_CATEGORIES.paymentComms,
    keysAndCrypto:    cats?.keysAndCrypto    ?? DEFAULT_PASTE_MONITOR_CATEGORIES.keysAndCrypto,
    personalData:     cats?.personalData     ?? DEFAULT_PASTE_MONITOR_CATEGORIES.personalData,
    maliciousCommand: cats?.maliciousCommand ?? DEFAULT_PASTE_MONITOR_CATEGORIES.maliciousCommand,
    unicode:          cats?.unicode          ?? DEFAULT_PASTE_MONITOR_CATEGORIES.unicode,
  };
}

/** Paid extensions: crypto-swap detection + auto-clear after detection.
 *  Defaults track the Rust side so a fresh install behaves the same way
 *  the runtime expects when no settings have been written yet. */
export const DEFAULT_PASTE_MONITOR_CRYPTO_SWAP_ENABLED = true;
export const DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ENABLED = false;
export const DEFAULT_PASTE_MONITOR_AUTO_CLEAR_SECONDS = 30;
export const DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ON_LOCK = false;

export default function usePasteMonitor(
  enabled: boolean,
  categories: PasteMonitorRustCategories,
  cryptoSwapEnabled: boolean,
  autoClearEnabled: boolean,
  autoClearSeconds: number,
  autoClearOnLock: boolean,
) {
  // Start / stop the watcher.
  useEffect(() => {
    const operationId = newDiagnosticOperationId("clipboard");
    if (enabled) {
      invoke("start_paste_monitor").then(() => {
        recordDiagnostic({ operationId, feature: "clipboard", action: "paste_monitor", stage: "runtime",
          lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
          suggestedNextAction: "none", privacyClass: "restricted", context: { state: "armed" } });
      }).catch((err) => {
        console.warn("[usePasteMonitor] start failed:", err);
        recordDiagnostic({ operationId, feature: "clipboard", action: "paste_monitor", stage: "runtime",
          lifecycle: "applying", outcome: "failed", errorCode: "CLP.MONITOR.START_FAILED",
          severity: "warn", retryability: "manual", suggestedNextAction: "retry",
          privacyClass: "restricted", context: { state: "arming" } });
      });
    } else {
      invoke("stop_paste_monitor").then(() => {
        recordDiagnostic({ operationId, feature: "clipboard", action: "paste_monitor", stage: "runtime",
          lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
          suggestedNextAction: "none", privacyClass: "restricted", context: { state: "stopped" } });
      }).catch((err) => {
        console.warn("[usePasteMonitor] stop failed:", err);
        recordDiagnostic({ operationId, feature: "clipboard", action: "paste_monitor", stage: "runtime",
          lifecycle: "applying", outcome: "failed", errorCode: "CLP.MONITOR.STOP_FAILED",
          severity: "warn", retryability: "automatic", suggestedNextAction: "retry",
          privacyClass: "restricted", context: { state: "stopping" } });
      });
    }
  }, [enabled]);

  // Push category changes to the runtime authority. We don't gate this
  // on `enabled` so the categories are always in sync — when the
  // watcher next starts, it picks up the right mask immediately.
  useEffect(() => {
    const operationId = newDiagnosticOperationId("clipboard");
    invoke("set_paste_monitor_categories", { categories }).then(() => {
      recordDiagnostic({ operationId, feature: "clipboard", action: "paste_category_sync", stage: "runtime",
        lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "restricted", context: { state: "synced" } });
    }).catch((err) => {
      console.warn("[usePasteMonitor] sync categories failed:", err);
      recordDiagnostic({ operationId, feature: "clipboard", action: "paste_category_sync", stage: "runtime",
        lifecycle: "applying", outcome: "failed", errorCode: "CLP.CATEGORY.SYNC_FAILED",
        severity: "warn", retryability: "manual", suggestedNextAction: "review_settings",
        privacyClass: "restricted", context: { state: "sync_failed" } });
    });
  }, [
    categories.cloudApi,
    categories.aiApi,
    categories.devTools,
    categories.paymentComms,
    categories.keysAndCrypto,
    categories.personalData,
    categories.maliciousCommand,
    categories.unicode,
    categories,
  ]);

  // Sync the crypto-swap toggle.
  useEffect(() => {
    const operationId = newDiagnosticOperationId("clipboard");
    invoke("set_paste_monitor_crypto_swap", { enabled: cryptoSwapEnabled }).then(() => {
      recordDiagnostic({ operationId, feature: "clipboard", action: "crypto_swap_sync", stage: "runtime",
        lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "restricted", context: { state: "synced" } });
    }).catch((err) => {
      console.warn("[usePasteMonitor] sync crypto-swap failed:", err);
      recordDiagnostic({ operationId, feature: "clipboard", action: "crypto_swap_sync", stage: "runtime",
        lifecycle: "applying", outcome: "failed", errorCode: "CLP.CRYPTO_SWAP.SYNC_FAILED",
        severity: "warn", retryability: "manual", suggestedNextAction: "review_settings",
        privacyClass: "restricted", context: { state: "sync_failed" } });
    });
  }, [cryptoSwapEnabled]);

  // Sync the auto-clear config.
  useEffect(() => {
    const operationId = newDiagnosticOperationId("clipboard");
    invoke("set_paste_monitor_auto_clear", {
      enabled: autoClearEnabled,
      seconds: autoClearSeconds,
    }).then(() => {
      recordDiagnostic({ operationId, feature: "clipboard", action: "auto_clear_sync", stage: "runtime",
        lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "restricted", context: { state: "synced" } });
    }).catch((err) => {
      console.warn("[usePasteMonitor] sync auto-clear failed:", err);
      recordDiagnostic({ operationId, feature: "clipboard", action: "auto_clear_sync", stage: "runtime",
        lifecycle: "applying", outcome: "failed", errorCode: "CLP.AUTO_CLEAR.SYNC_FAILED",
        severity: "warn", retryability: "manual", suggestedNextAction: "review_settings",
        privacyClass: "restricted", context: { state: "sync_failed" } });
    });
  }, [autoClearEnabled, autoClearSeconds]);

  // Sync the auto-clear-on-lock toggle.
  useEffect(() => {
    const operationId = newDiagnosticOperationId("clipboard");
    invoke("set_paste_monitor_auto_clear_on_lock", { enabled: autoClearOnLock }).then(() => {
      recordDiagnostic({ operationId, feature: "clipboard", action: "auto_clear_on_lock_sync", stage: "runtime",
        lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never",
        suggestedNextAction: "none", privacyClass: "restricted", context: { state: "synced" } });
    }).catch((err) => {
      console.warn("[usePasteMonitor] sync auto-clear-on-lock failed:", err);
      recordDiagnostic({ operationId, feature: "clipboard", action: "auto_clear_on_lock_sync", stage: "runtime",
        lifecycle: "applying", outcome: "failed", errorCode: "CLP.AUTO_CLEAR_ON_LOCK.SYNC_FAILED",
        severity: "warn", retryability: "manual", suggestedNextAction: "review_settings",
        privacyClass: "restricted", context: { state: "sync_failed" } });
    });
  }, [autoClearOnLock]);
}
