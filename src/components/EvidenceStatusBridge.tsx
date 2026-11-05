import { useEffect } from "react";
import { getEvidenceStatus } from "../lib/evidence";
import { pushNotification } from "../lib/notificationStore";

type Notice = "continuity" | "unavailable" | null;
type Notify = (message: string, id: string) => void;

export function updateEvidenceNotice(next: Notice, previous: Notice, notify: Notify): Notice {
  if (next && next !== previous) {
    if (next === "continuity") {
      notify("Some activity history may be missing or older records may need review. Check them before relying on it.", "evidence-continuity");
    } else {
      notify("Activity history could not be checked. Open WinCommander support before relying on it.", "evidence-unavailable");
    }
  }
  return next;
}

export default function EvidenceStatusBridge() {
  useEffect(() => {
    let active = true;
    let busy = false;
    let previous: Notice = null;
    const notify: Notify = (message, id) => { pushNotification("warn", message, undefined, "alert", id); };
    const refresh = async () => {
      if (busy) return;
      busy = true;
      try {
        const status = await getEvidenceStatus();
        if (active) previous = updateEvidenceNotice(status.continuityUnverified ? "continuity" : null, previous, notify);
      } catch {
        if (active) previous = updateEvidenceNotice("unavailable", previous, notify);
      } finally {
        busy = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => { void refresh(); }, 60_000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);
  return null;
}
