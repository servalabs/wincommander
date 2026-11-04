import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type Contact = { deviceId: string; ms: number | null; serviceState: string; serviceMs: number | null };
function localTime(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 && Number.isFinite(new Date(value).getTime()) ? value : null;
}
export function useFleetServiceContact(deviceId: string) {
  const [contact, setContact] = useState<Contact | null>(null);
  useEffect(() => {
    let active = true;
    let pending = false;
    setContact(null);
    async function refresh() {
      if (pending) return;
      pending = true;
      try {
        const result: unknown = await invoke("fleet_service_contact_status", { deviceId });
        const value = result as { observation?: { observed_unix_ms?: unknown }; serviceReachability?: { state?: unknown; observed_unix_ms?: unknown } } | null;
        if (active) setContact({
          deviceId, ms: localTime(value?.observation?.observed_unix_ms),
          serviceState: typeof value?.serviceReachability?.state === "string" ? value.serviceReachability.state : "unconfirmed",
          serviceMs: localTime(value?.serviceReachability?.observed_unix_ms),
        });
      } catch {
        if (active) setContact(null);
      } finally { pending = false; }
    }
    void refresh();
    const timer = setInterval(() => { void refresh(); }, 30_000);
    return () => { active = false; clearInterval(timer); };
  }, [deviceId]);
  return contact?.deviceId === deviceId ? contact : null;
}
