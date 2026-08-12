/**
 * useRdpIncomingDismount
 *
 * Polls Watch-RdpIncomingSessions every 10 s and optionally:
 *   • dismounts local VeraCrypt vaults as soon as no incoming RDP user is
 *     ATTENDING the machine, and
 *   • signs off sessions that Windows left in the disconnected state after a
 *     user closed the RDP client window.
 *
 * "Attended" = a session that is actively connected (quser/qwinsta STATE
 * "Active"). A session the user closed the window on WITHOUT signing off
 * lingers on the host in the "Disc" (Disconnected) state — no one is viewing
 * the screen, yet the session is still alive and reconnectable. For the
 * purpose of protecting mounted vaults that is UNATTENDED, so we dismount on
 * it just like a full sign-off.
 *
 * We fire Dismount-LocalVaults on EITHER transition (>0 → 0):
 *   • attended count → 0  — the last active user left (disconnect OR sign-off), and
 *   • total count    → 0  — every session disappeared (full sign-off).
 *
 * The total→0 clause is the safety net: it is exactly the original behaviour
 * and does NOT depend on the backend reporting a recognisable STATE string.
 * An earlier version dropped it and keyed solely off the attended count — when
 * the backend mislabels an active session's STATE (e.g. qwinsta sessions are
 * tagged "Disc"), attended stayed 0 forever and NOTHING dismounted, not even
 * on sign-off. The attended→0 clause only fires after we have actually seen an
 * attended session, so a mislabelled STATE can never cause a premature dismount
 * while a user is still connected.
 */
import { useEffect, useRef } from "react";
import { executeBackendCommand } from "./useBackend";

const POLL_MS = 10_000;

interface RdpSession {
  sessionId?: number;
  username?: string;
  sessionName?: string;
  /** quser/qwinsta STATE column: "Active", "Disc", "Conn", … */
  state?: string;
}

// Only an actively-connected session keeps the vault "in use". Anything else
// (Disconnected, gone, unknown) means no one is at the keyboard and must not
// block a dismount.
const ATTENDED_STATES = new Set(["active", "conn", "connected"]);

function isAttended(session: RdpSession): boolean {
  const state = (session?.state ?? "").toString().trim().toLowerCase();
  return ATTENDED_STATES.has(state);
}

export default function useRdpIncomingDismount(
  enabled: boolean,
  dismountOnEmpty: boolean = true,
  signOffOnDisconnect: boolean = false,
) {
  const mountedRef = useRef(true);
  const prevAttendedRef = useRef<number | null>(null);
  const prevTotalRef = useRef<number | null>(null);
  const inFlightRef = useRef(false);
  const signOffInFlightRef = useRef<Set<number>>(new Set());

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  useEffect(() => {
    if (!enabled) {
      prevAttendedRef.current = null;
      prevTotalRef.current = null;
      signOffInFlightRef.current.clear();
      console.log("[RdpIncomingDismount] Disabled — not polling");
      return;
    }

    console.log("[RdpIncomingDismount] Starting session monitor");

    const poll = async () => {
      if (!mountedRef.current || inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        const res = await executeBackendCommand<{ sessions?: RdpSession[]; queryError?: string }>(
          "Watch-RdpIncomingSessions",
          {}
        );
        if (!mountedRef.current) return;

        if (!res.success || res.data == null) {
          console.warn("[RdpIncomingDismount] Poll failed:", JSON.stringify(res));
          return;
        }

        // PowerShell's ConvertTo-Json serializes a SINGLE-element array as a bare
        // object, so one incoming session arrives as {…} not [{…}]. Normalize so a
        // lone session is counted (otherwise total stays 0 and dismount never fires).
        const rawSessions = res.data.sessions as RdpSession[] | RdpSession | undefined;
        const sessions: RdpSession[] = Array.isArray(rawSessions)
          ? rawSessions
          : rawSessions
            ? [rawSessions]
            : [];
        const attended = sessions.filter(isAttended).length;
        const total = sessions.length;
        const prevAttended = prevAttendedRef.current;
        const prevTotal = prevTotalRef.current;
        console.log(
          `[RdpIncomingDismount] Attended: ${attended} / ${total} total ` +
            `(prev attended: ${prevAttended ?? "unknown"}, prev total: ${prevTotal ?? "unknown"})`
        );
        const liveIds = new Set(
          sessions.map(session => session.sessionId).filter((id): id is number => typeof id === "number")
        );
        for (const id of [...signOffInFlightRef.current]) {
          if (!liveIds.has(id)) signOffInFlightRef.current.delete(id);
        }

        // Fire when the LAST attended user leaves (disconnect or sign-off) OR
        // when every session disappears (full sign-off — the format-agnostic net).
        const attendedDrained = prevAttended !== null && prevAttended > 0 && attended === 0;
        const totalDrained = prevTotal !== null && prevTotal > 0 && total === 0;
        if (dismountOnEmpty && (attendedDrained || totalDrained)) {
          console.log(
            `[RdpIncomingDismount] No attended RDP user remaining — dismounting local vaults ` +
              `(attendedDrained=${attendedDrained}, totalDrained=${totalDrained})`
          );
          executeBackendCommand("Dismount-LocalVaults", {})
            .then(r => console.log("[RdpIncomingDismount] Dismount result:", JSON.stringify(r)))
            .catch(e => console.error("[RdpIncomingDismount] Dismount error:", e));
        }

        if (signOffOnDisconnect) {
          const disconnected = sessions.filter((session) => {
            const id = session.sessionId;
            const state = (session.state ?? "").toString().trim().toLowerCase();
            return typeof id === "number" && id > 0 && state === "disc" && !signOffInFlightRef.current.has(id);
          });
          for (const session of disconnected) {
            const id = session.sessionId;
            if (typeof id !== "number") continue;
            signOffInFlightRef.current.add(id);
            console.log(`[RdpIncomingDismount] Signing off disconnected RDP session ${id}`);
            executeBackendCommand("Logoff-RdpIncomingSession", { SessionId: id })
              .then(r => console.log(`[RdpIncomingDismount] Logoff ${id} result:`, JSON.stringify(r)))
              .catch(e => {
                signOffInFlightRef.current.delete(id);
                console.error(`[RdpIncomingDismount] Logoff ${id} error:`, e);
              });
          }
        }

        prevAttendedRef.current = attended;
        prevTotalRef.current = total;
      } catch (e) {
        console.error("[RdpIncomingDismount] Poll exception:", e);
      } finally {
        inFlightRef.current = false;
      }
    };

    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => clearInterval(timer);
  }, [enabled, dismountOnEmpty, signOffOnDisconnect]);
}
