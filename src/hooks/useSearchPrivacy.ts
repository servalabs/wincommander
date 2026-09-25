// SPDX-License-Identifier: AGPL-3.0-or-later
import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getSearchPrivacy, setSearchPrivacy, subscribeSearchPrivacy } from "@/lib/searchPrivacy";
import type { ContentPrivacyStatus } from "@/lib/searchPrivacy";

let pending: Promise<void> | null = null;
let monitorEpoch = 0;
export function refreshSearchPrivacy(fresh = false): Promise<void> {
  // Post-result checks must start after the result arrived. Reusing a probe
  // already in flight could accept its pre-dismount snapshot as current.
  if (pending) return fresh ? pending.then(() => refreshSearchPrivacy()) : pending;
  const epoch = monitorEpoch;
  pending = invoke<ContentPrivacyStatus>("content_privacy_status")
    .then((status) => { if (epoch === monitorEpoch) setSearchPrivacy(status); })
    .catch(() => { if (epoch === monitorEpoch) setSearchPrivacy(null); })
    .finally(() => { pending = null; });
  return pending;
}

let consumers = 0;
let stopPolling: (() => void) | null = null;
function startPolling(): () => void {
  consumers += 1;
  if (consumers === 1) {
    let stopped = false;
    let running = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      if (running) return;
      running = true;
      clearTimeout(timer);
      await refreshSearchPrivacy();
      running = false;
      if (!stopped) timer = setTimeout(poll, document.hidden ? 15000 : 2000);
    };
    const focus = () => { void poll(); };
    window.addEventListener("focus", focus);
    document.addEventListener("visibilitychange", focus);
    void poll();
    stopPolling = () => {
      stopped = true;
      clearTimeout(timer);
      window.removeEventListener("focus", focus);
      document.removeEventListener("visibilitychange", focus);
    };
  }
  return () => {
    consumers -= 1;
    if (consumers === 0) {
      monitorEpoch += 1;
      stopPolling?.(); stopPolling = null; setSearchPrivacy(null);
    }
  };
}

export function useSearchPrivacy() {
  const state = useSyncExternalStore(subscribeSearchPrivacy, getSearchPrivacy, getSearchPrivacy);
  useEffect(startPolling, []);
  return state;
}
