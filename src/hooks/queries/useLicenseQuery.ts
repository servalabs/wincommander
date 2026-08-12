// src/hooks/queries/useLicenseQuery.ts
//
// ═══════════════════════════════════════════════════════════════════════
// REACT QUERY — License status cache
// ═══════════════════════════════════════════════════════════════════════
//
// Wraps the existing get_license_status Tauri command in a TanStack Query.
// Used by useEntitlements() and any UI that needs to react to license state
// changes (LicenseQuickPanel, AppLicensePanel, paywall components, etc.).
//
// staleTime: 12h — license state rarely changes. After activate /
// refresh / deactivate, callers invalidate the query to force a refetch.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { AppLicenseStatus } from "../useBackend";

// ── Query Keys ──────────────────────────────────────────────────────
export const licenseKeys = {
  all: ["license"] as const,
  status: () => [...licenseKeys.all, "status"] as const,
};

// ── Core Query ──────────────────────────────────────────────────────
/** Fetches the current license status from Rust. Cached for 12h. */
export function useLicenseQuery() {
  return useQuery<AppLicenseStatus>({
    queryKey: licenseKeys.status(),
    queryFn: () => invoke<AppLicenseStatus>("get_license_status"),
    staleTime: 12 * 60 * 60 * 1000,
  });
}

// ── Imperative invalidation ─────────────────────────────────────────
/** Call after activate / refresh / deactivate / start_trial. */
export function useInvalidateLicense() {
  const qc = useQueryClient();
  return () => qc.invalidateQueries({ queryKey: licenseKeys.all });
}
