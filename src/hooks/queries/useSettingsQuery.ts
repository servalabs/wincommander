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
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, SettingsPatch } from "../../types/settings";
import { useAuthMode } from "../../context/AuthModeContext";
import { recoverSettingsWrite } from "../../lib/settingsWriteRecovery";
export {
  getControlLifecycle,
  getControlLifecycleTone,
  type ControlAccountIdentity,
  type ControlLifecycle,
  type ControlLifecycleInput,
  type ControlLifecycleState,
} from "../../lib/settingsControlLifecycle";

// ── Query Keys ──────────────────────────────────────────────────────
export const settingsKeys = {
  all: ["settings"] as const,
  detail: () => [...settingsKeys.all, "detail"] as const,
};

// ── Core Query ──────────────────────────────────────────────────────
/** Fetches the full AppSettings from Rust. Fast (~1ms — cached in memory).
 *  Disabled in decoy mode — real settings must not be fetched during a
 *  coerced session. Consumers get undefined data (same as loading/first-mount),
 *  which every panel already handles gracefully via null guards in AppContext. */
export function useSettingsQuery() {
  const { mode: authMode } = useAuthMode();
  return useQuery<AppSettings>({
    queryKey: settingsKeys.detail(),
    queryFn: () => invoke<AppSettings>("get_settings"),
    // KT: disabled in decoy — prevents real settings from reaching useAutoHeal
    // and useAdoptCurrentState which both consume this query via BackgroundPollers.
    enabled: authMode !== 'decoy',
  });
}

// ── Mutations ───────────────────────────────────────────────────────

/** Patch settings (deep merge) and update the query cache with the result. */
export function usePatchSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (patch: SettingsPatch) =>
      recoverSettingsWrite(
        () => invoke<AppSettings>("patch_settings_cmd", { patch }),
        () => invoke<AppSettings>("get_settings"),
        (restored) => qc.setQueryData(settingsKeys.detail(), restored),
      ),
    onSuccess: (updated) => {
      qc.setQueryData(settingsKeys.detail(), updated);
    },
  });
}

/** Full replace of settings. */
export function useSetSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (settings: AppSettings) =>
      recoverSettingsWrite(
        () => invoke<AppSettings>("set_settings", { settings }),
        () => invoke<AppSettings>("get_settings"),
        (restored) => qc.setQueryData(settingsKeys.detail(), restored),
      ),
    onSuccess: (updated) => {
      qc.setQueryData(settingsKeys.detail(), updated);
    },
  });
}

/** Update current state from a probe result. */
export function useUpdateCurrentState() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (probe: Record<string, unknown>) =>
      invoke<AppSettings>("update_current_state", { probe }),
    onSuccess: (updated) => {
      qc.setQueryData(settingsKeys.detail(), updated);
    },
  });
}

// ── Imperative invalidation ─────────────────────────────────────────
/** Call after a backend command to refetch settings. */
export function useInvalidateSettings() {
  const qc = useQueryClient();
  return () => qc.invalidateQueries({ queryKey: settingsKeys.all });
}
