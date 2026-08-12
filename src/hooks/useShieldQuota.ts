// src/hooks/useShieldQuota.ts
//
// ═══════════════════════════════════════════════════════════════════════
// PRIVACY SHIELD QUOTA — Frontend hooks
// ═══════════════════════════════════════════════════════════════════════
//
// Three exports:
//   - useShieldQuotaQuery() → reactive quota state (TanStack Query, 30s
//     staleTime). Re-fetched after every consume_shield_minutes call.
//   - useShieldQuotaTicker() → while a Privacy Shield session is running,
//     calls consume_shield_minutes(1.0) every 60s. Fires onExhausted()
//     when free-tier user hits the daily 15-minute cap. No-op for paid.
//   - useInvalidateShieldQuota() → manual cache buster.
//
// The frontend ticker design (vs. a Rust-side watchdog) keeps the quota
// loop close to the UI so we can update "X / 15 min today" smoothly and
// trigger the paywall dialog from a React component. The Rust side stays
// simple: state + read + increment, no timer.

import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

export interface ShieldQuotaStatus {
  is_unlimited: boolean;
  minutes_used: number;
  minutes_remaining: number;
  hard_cap_minutes: number;
  date: string;
}

export const shieldQuotaKeys = {
  all: ["shield-quota"] as const,
  status: () => [...shieldQuotaKeys.all, "status"] as const,
};

export function useShieldQuotaQuery() {
  return useQuery<ShieldQuotaStatus>({
    queryKey: shieldQuotaKeys.status(),
    queryFn: () => invoke<ShieldQuotaStatus>("get_shield_quota"),
    staleTime: 30 * 1000,
  });
}

export function useInvalidateShieldQuota() {
  const qc = useQueryClient();
  return () => qc.invalidateQueries({ queryKey: shieldQuotaKeys.all });
}

/** Periodic consumer for active Privacy Shield sessions.
 *
 *  - When `running` is true and the user has no paid entitlement,
 *    invokes `consume_shield_minutes(1.0)` every 60 seconds.
 *  - When the response reports minutes_remaining === 0 (and the user
 *    is not unlimited), `onExhausted` is called once. The caller is
 *    expected to stop the shield + surface the paywall.
 *  - When `running` is false the timer is torn down.
 *
 *  Note: This ticks once per minute so the granularity is whole minutes.
 *  If the user closes the app mid-minute, that minute is lost — acceptable
 *  for v1. A higher-resolution version would tick every 10s with 0.167
 *  minute increments.
 */
export function useShieldQuotaTicker(
  running: boolean,
  onExhausted: () => void,
) {
  const qc = useQueryClient();
  const exhaustedFiredRef = useRef(false);

  useEffect(() => {
    if (!running) {
      exhaustedFiredRef.current = false;
      return;
    }

    let cancelled = false;
    const tick = async () => {
      if (cancelled) return;
      try {
        const status = await invoke<ShieldQuotaStatus>("consume_shield_minutes", {
          minutes: 1.0,
        });
        if (cancelled) return;
        qc.setQueryData(shieldQuotaKeys.status(), status);
        if (
          !status.is_unlimited &&
          status.minutes_remaining <= 0 &&
          !exhaustedFiredRef.current
        ) {
          exhaustedFiredRef.current = true;
          onExhausted();
        }
      } catch {
        // Silent — quota errors shouldn't crash the shield session.
      }
    };

    const interval = setInterval(tick, 60_000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [running, onExhausted, qc]);
}
