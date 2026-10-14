// src/hooks/queries/useSettingsQuery.ts
//
// ═══════════════════════════════════════════════════════════════════════
// REACT QUERY — Settings Cache
// ═══════════════════════════════════════════════════════════════════════
//
// Core query: reads appSettings from Rust (instantaneous — Rust reads a
// memory-mapped file, not the full PS probe). Every panel that needs
// settings.current or settings.ideal goes through this one query.
//
// The QueryClient is configured in main.tsx with staleTime: 30s.
// Panels that need fresh data after a toggle call invalidateSettings().
//
// USAGE:
//   const { data: settings, isLoading } = useSettingsQuery();
//   const value = getByPath(settings, "current.privacy.telemetry.windowsDisabled");

import { useQuery, useQueryClient, useMutation } from "@tanstack/react-query";
import type { AppSettings, SettingsPatch } from "../../types/settings";
import { useAuthMode } from "../../context/AuthModeContext";
import { useSettingsWriter } from "../../context/SettingsWriteContext";
import { settingsKeys, settingsQueryOptions } from "../../lib/settingsQueryPolicy";
export { settingsKeys } from "../../lib/settingsQueryPolicy";
export {
  getControlLifecycle,
  getControlLifecycleTone,
  type ControlAccountIdentity,
  type ControlLifecycle,
  type ControlLifecycleInput,
  type ControlLifecycleState,
} from "../../lib/settingsControlLifecycle";

// ── Core Query ──────────────────────────────────────────────────────
/** Fetches the full AppSettings from Rust. Fast (~1ms — cached in memory).
 *  Disabled in decoy mode — real settings must not be fetched during a
 *  coerced session. Consumers get undefined data (same as loading/first-mount),
 *  which every panel already handles gracefully via null guards in AppContext. */
export function useSettingsQuery() {
  const { mode: authMode } = useAuthMode();
  const writer = useSettingsWriter();
  return useQuery(settingsQueryOptions(authMode, writer.read));
}

// ── Mutations ───────────────────────────────────────────────────────

/** Patch settings (deep merge) and update the query cache with the result. */
export function usePatchSettings() {
  const writer = useSettingsWriter();
  return useMutation({
    mutationFn: (patch: SettingsPatch) => writer.patch(patch),
  });
}

/** Full replace of settings. */
export function useSetSettings() {
  const writer = useSettingsWriter();
  return useMutation({
    mutationFn: (settings: AppSettings) => writer.replace(settings),
  });
}

/** Update current state from a probe result. */
export function useUpdateCurrentState() {
  const writer = useSettingsWriter();
  return useMutation({
    mutationFn: (probe: Record<string, unknown>) => writer.updateCurrent(probe),
  });
}

// ── Imperative invalidation ─────────────────────────────────────────
/** Call after a backend command to refetch settings. */
export function useInvalidateSettings() {
  const qc = useQueryClient();
  return () => qc.invalidateQueries({ queryKey: settingsKeys.all });
}
