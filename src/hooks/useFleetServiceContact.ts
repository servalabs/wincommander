import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export function useFleetServiceContact(deviceId: string) {
  const [observation, setObservation] = useState<{ deviceId: string; ms: number } | null>(null);
  useEffect(() => {
    let active = true;
    let pending = false;
    setObservation(null);
    async function refresh() {
      if (pending) return;
      pending = true;
      try {
        const result: unknown = await invoke("fleet_service_contact_status", { deviceId });
        const record = (result as { observation?: { observed_unix_ms?: unknown } } | null)?.observation;
        const ms = record?.observed_unix_ms;
        if (active) setObservation(typeof ms === "number" && Number.isSafeInteger(ms) && ms > 0 && Number.isFinite(new Date(ms).getTime()) ? { deviceId, ms } : null);
      } catch {
        if (active) setObservation(null);
      } finally { pending = false; }
    }
    void refresh();
    const timer = setInterval(() => { void refresh(); }, 30_000);
    return () => { active = false; clearInterval(timer); };
  }, [deviceId]);

  return observation?.deviceId === deviceId ? observation.ms : null;
}
