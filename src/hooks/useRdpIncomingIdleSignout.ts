/**
 * useRdpIncomingIdleSignout
 *
 * App-side enforcement for the "RDP Incoming" idle sign-out feature. Polls
 * Watch-RdpIncomingSessions and signs OFF (logoff — not just disconnect) every
 * incoming RDP session whose idle time has reached the configured threshold.
 *
 * Why this exists: enabling the feature only writes the Windows Terminal
 * Services Group Policy (MaxIdleTime / fResetBroken). That GPO enforces at
 * roughly 1-minute resolution and in practice was not reliably signing
 * sessions off, so nothing happened when a user went idle. The backend's
 * Watch-RdpIncomingSessions was always meant to drive a JS-side auto sign-out
 * (see commander-pro handlers.rs) — this hook is that missing piece.
 *
 * Idle source: the backend's per-session `idleSeconds`. For the session this
 * machine's app runs in it is a precise GetLastInputInfo value (advances every
 * second, sub-minute capable); for other users' sessions it is quser-derived
 * (whole-minute resolution — a Windows quser limitation). Poll cadence bounds
 * how soon after crossing the threshold the sign-off fires.
 *
 * Self sign-off: when the session being signed off is the one running this app
 * (`isCurrentSession`), the logoff kills the app — so if `dismountOnEmpty` is
 * set we dismount the local vaults FIRST, then issue the logoff. Otherwise the
 * post-sign-off dismount (handled by useRdpIncomingDismount while the app is
 * still alive) could never run for the app's own session.
 */
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { executeBackendCommand } from "./useBackend";
import { showError } from "../utils/toast";

const POLL_MS = 10_000;

// Write a state-transition event to the UNIFIED app log (Error Center) via
// write_log_record — mirrors useRdpIdleDisconnect's logRdp helper so idle
// sign-off failures are visible in the same place as other RDP events.
function logRdp(level: "info" | "warn" | "error", message: string): void {
  invoke("write_log_record", { level, message }).catch(() => {});
}

interface RdpSession {
  sessionId?: number;
  username?: string;
  sessionName?: string;
  /** quser STATE: "Active" | "Disc" */
  state?: string;
  /** Per-session idle in seconds — precise (GetLastInputInfo) for the current
   *  session, quser-derived for the others. Preferred when present. */
  idleSeconds?: number;
  /** Whole minutes idle, parsed from quser IDLE TIME. Fallback. */
  idleMinutes?: number;
  idleDisplay?: string;
  /** True for the session this app process runs in (logging it off kills us). */
  isCurrentSession?: boolean;
}

export default function useRdpIncomingIdleSignout(
  enabled: boolean,
  timeoutSeconds: number,
  dismountOnEmpty: boolean = false,
) {
  const mountedRef = useRef(true);
  const inFlightRef = useRef(false);
  // Sessions we've already issued a logoff for — avoids re-firing every poll
  // while the session winds down. Entries are cleared once the session is gone.
  const signedOffRef = useRef<Set<number>>(new Set());

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  useEffect(() => {
    if (!enabled || !Number.isFinite(timeoutSeconds) || timeoutSeconds <= 0) {
      signedOffRef.current.clear();
      console.log("[RdpIncomingSignout] Disabled — not polling");
      return;
    }

    console.log(`[RdpIncomingSignout] Starting idle sign-out monitor (threshold ${timeoutSeconds}s)`);

    const poll = async () => {
      if (!mountedRef.current || inFlightRef.current) return;
      inFlightRef.current = true;
      try {
        const res = await executeBackendCommand<{ sessions?: RdpSession[]; queryError?: string; rawLines?: string[] }>(
          "Watch-RdpIncomingSessions",
          {}
        );
        if (!mountedRef.current) return;

        if (!res.success || res.data == null) {
          console.warn("[RdpIncomingSignout] Poll failed:", JSON.stringify(res));
          return;
        }

        // PowerShell's ConvertTo-Json serializes a SINGLE-element array as a bare
        // object, so with exactly one incoming session `sessions` arrives as {…}
        // not [{…}]. Normalize so a lone session isn't silently dropped.
        const rawSessions = res.data.sessions as RdpSession[] | RdpSession | undefined;
        const sessions: RdpSession[] = Array.isArray(rawSessions)
          ? rawSessions
          : rawSessions
            ? [rawSessions]
            : [];
        if (sessions.length === 0) {
          // Diagnostic: show what quser/qwinsta actually returned. If rawLines
          // contains an rdp-tcp# row but sessions is empty, the parser (backend
          // build) is stale; if rawLines is empty, the host returned no sessions.
          console.log(
            `[RdpIncomingSignout] poll: 0 incoming sessions` +
              (res.data.queryError ? ` (queryError: ${res.data.queryError})` : "") +
              ` rawLines=${JSON.stringify(res.data.rawLines ?? [])}`
          );
        }

        // Forget sessions that have ended so a re-used session id re-arms.
        const liveIds = new Set(
          sessions.map(s => s.sessionId).filter((n): n is number => typeof n === "number")
        );
        for (const id of [...signedOffRef.current]) {
          if (!liveIds.has(id)) signedOffRef.current.delete(id);
        }

        for (const s of sessions) {
          const id = s.sessionId;
          if (typeof id !== "number" || id <= 0) continue;
          const idleSec = Math.max(0, s.idleSeconds ?? (s.idleMinutes ?? 0) * 60);
          const reached = idleSec >= timeoutSeconds;
          console.log(
            `[RdpIncomingSignout] session ${id} '${s.username ?? "?"}' state=${s.state ?? "?"} ` +
              `idle=${idleSec}s / threshold=${timeoutSeconds}s${reached ? " — REACHED" : ""}`
          );
          if (reached && !signedOffRef.current.has(id)) {
            signedOffRef.current.add(id); // optimistic: blocks duplicate in-flight logoffs
            const signOff = () => {
              console.log(`[RdpIncomingSignout] Signing off idle session ${id} ('${s.username ?? "?"}')`);
              return executeBackendCommand<{ ok?: boolean; success?: boolean }>("Logoff-RdpIncomingSession", { SessionId: id })
                .then(r => {
                  console.log(`[RdpIncomingSignout] Logoff ${id} result:`, JSON.stringify(r));
                  // Pro returns { ok: bool }, Free returns { success: bool }.
                  // If either signals failure, clear so the next poll retries
                  // rather than treating the session as permanently signed off.
                  const succeeded = r.success !== false && r.data?.ok !== false;
                  if (!succeeded) {
                    signedOffRef.current.delete(id);
                    console.warn(`[RdpIncomingSignout] Logoff ${id} failed (ok=${r.data?.ok}, success=${r.success}), will retry`);
                    logRdp("warn", `RDP incoming idle sign-off failed for session ${id} ('${s.username ?? "?"}') — will retry`);
                    void showError(`Could not sign off idle RDP session ${id} ('${s.username ?? "?"}') — will retry`);
                  }
                })
                .catch(e => {
                  signedOffRef.current.delete(id); // IPC error — let the next poll retry
                  console.error(`[RdpIncomingSignout] Logoff ${id} error:`, e);
                  const msg = e instanceof Error ? e.message : String(e);
                  logRdp("error", `RDP incoming idle sign-off error for session ${id} ('${s.username ?? "?"}'): ${msg}`);
                  void showError(`Error signing off idle RDP session ${id} ('${s.username ?? "?"}') — will retry`);
                });
            };
            // Logging off our OWN session kills this app, so the post-sign-off
            // dismount can never run. Dismount FIRST, then sign off.
            if (dismountOnEmpty && s.isCurrentSession) {
              console.log(`[RdpIncomingSignout] session ${id} is this app's session — dismounting vaults before sign-off`);
              void executeBackendCommand("Dismount-LocalVaults", {})
                .then(r => console.log("[RdpIncomingSignout] Pre-sign-off dismount result:", JSON.stringify(r)))
                .catch(e => console.error("[RdpIncomingSignout] Pre-sign-off dismount error:", e))
                .finally(signOff);
            } else {
              void signOff();
            }
          }
        }
      } catch (e) {
        console.error("[RdpIncomingSignout] Poll exception:", e);
      } finally {
        inFlightRef.current = false;
      }
    };

    poll();
    const timer = setInterval(poll, POLL_MS);
    return () => clearInterval(timer);
  }, [enabled, timeoutSeconds, dismountOnEmpty]);
}
