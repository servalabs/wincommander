/**
 * useRdpIdleDisconnect
 *
 * Disconnects an OUTGOING RDP session (and optionally dismounts the remote
 * server's VeraCrypt vaults) once the local machine has been idle longer than
 * the configured timeout.
 *
 * Idle source of truth — read EVERY second from a cheap native call
 * (get_system_idle_seconds → GetLastInputInfo). It is system-wide, so ANY
 * keyboard/mouse activity ANYWHERE — including inside the remote mstsc window —
 * makes it drop back to ~0 and the counter resets within 1 s. We deliberately
 * do NOT use DOM activity listeners (mousemove/mousedown/…): while the user
 * works in mstsc those fire spuriously on our window (cursor drift, synthetic
 * moves, z-order churn from the warning's own setAlwaysOnTop/setFocus) and used
 * to reset the counter so it never reached the threshold.
 *
 * The slower PowerShell poll (every 5 s) is used ONLY to learn whether mstsc is
 * running at all (rdpOpen) — the idle timer is gated on that so we never count
 * idle time when there is no RDP session to disconnect.
 *
 * Startup behaviour: when monitoring begins (or a new RDP session appears) we
 * anchor a baseline to the CURRENT idle reading and only count NEW idle from
 * there, so opening WinCommander after walking away can't trigger an immediate
 * kill.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { clearCommand } from "../lib/commandIds";
import { executeBackendCommand } from "./useBackend";

const POLL_MS = 5_000; // PowerShell: is mstsc running? (rdpOpen + remote hosts)
const TICK_MS = 1_000; // native: system-wide idle seconds

// Write a meaningful RDP-idle event to the UNIFIED app log (Error Center) via
// the write_log_record command — not just the dev console. Only state
// transitions are logged (start / warning / disconnect / failure / snooze),
// never the per-second tick, so the log stays readable.
function logRdp(level: "info" | "warn" | "error", message: string): void {
  invoke("write_log_record", { level, message }).catch(() => {});
}

export interface RdpIdleState {
  secondsSinceActivity: number;
  isIdle: boolean;
  warningActive: boolean;
  warningSecondsLeft: number;
  snooze: () => void;
}

export default function useRdpIdleDisconnect(
  enabled: boolean,
  timeoutSeconds: number = 120,
  warningSeconds: number = 5,
  clearCacheOnDisconnect: boolean = false,
  removeCredsOnDisconnect: boolean = false,
  saveLog: boolean = false,
  dismountVaultsOnDisconnect: boolean = false,
  disabledReason: string = "disabled",
): RdpIdleState {
  const mountedRef = useRef(true);
  const killedRef = useRef(false);
  const killInFlightRef = useRef(false); // true while the disconnect command is awaited
  const tickBusyRef = useRef(false);     // re-entrancy guard for the 1 s tick
  const nativeWarnedRef = useRef(false); // only show the balloon once per idle window
  const alwaysOnTopRef = useRef(false);  // tracks whether we called setAlwaysOnTop(true)
  const snoozeUntilRef = useRef(0);
  const rdpActiveRef = useRef(false);    // last PS poll said mstsc is running
  // Idle reading that corresponds to effectiveIdle === 0. -1 = not yet anchored;
  // re-anchored whenever the native idle drops (i.e. the user did something).
  const baselineIdleRef = useRef(-1);
  const effectiveWarningSeconds = Math.max(5, warningSeconds || 5);

  const [seconds, setSeconds] = useState(0);
  const [warningActive, setWarning] = useState(false);
  const [warningLeft, setWarningLeft] = useState(effectiveWarningSeconds);
  const [isIdle, setIsIdle] = useState(false);

  const dropAlwaysOnTop = useCallback(() => {
    if (alwaysOnTopRef.current) {
      alwaysOnTopRef.current = false;
      getCurrentWindow().setAlwaysOnTop(false).catch(() => {});
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  useEffect(() => {
    setWarningLeft((prev) => (warningActive ? Math.min(prev, effectiveWarningSeconds) : effectiveWarningSeconds));
  }, [effectiveWarningSeconds, warningActive]);

  const snooze = useCallback(() => {
    snoozeUntilRef.current = Date.now() + effectiveWarningSeconds * 1000;
    // Clicking snooze is itself activity, so the next native tick re-anchors the
    // baseline anyway — but reset eagerly so the UI clears immediately.
    baselineIdleRef.current = -1;
    killedRef.current = false;
    killInFlightRef.current = false;
    nativeWarnedRef.current = false;
    dropAlwaysOnTop();
    setWarning(false);
    setIsIdle(false);
    setWarningLeft(effectiveWarningSeconds);
    console.log("[RdpIdle] Snoozed for", effectiveWarningSeconds, "s");
    logRdp("info", `RDP idle disconnect cancelled — snoozed ${effectiveWarningSeconds}s`);
  }, [effectiveWarningSeconds, dropAlwaysOnTop]);

  useEffect(() => {
    if (!enabled) {
      rdpActiveRef.current = false;
      baselineIdleRef.current = -1;
      killedRef.current = false;
      killInFlightRef.current = false;
      nativeWarnedRef.current = false;
      snoozeUntilRef.current = 0;
      dropAlwaysOnTop();
      setSeconds(0);
      setWarning(false);
      setWarningLeft(effectiveWarningSeconds);
      setIsIdle(false);
      console.log("[RdpIdle] Monitor not started —", disabledReason);
      return;
    }

    console.log("[RdpIdle] Starting — timeout:", timeoutSeconds, "s | warning:", effectiveWarningSeconds, "s");
    logRdp("info", `RDP idle monitor armed — disconnect after ${timeoutSeconds}s idle (warns ${effectiveWarningSeconds}s before)`);

    // ── PowerShell poll (5 s): does an RDP session exist right now? ──────────
    const poll = async () => {
      if (!mountedRef.current) return;
      try {
        const res = await executeBackendCommand<{ rdpOpen?: boolean; remoteHosts?: string[] | string; processCount?: number }>(
          "Watch-RDPClientIdle",
          { TimeoutSeconds: "999999" } // never auto-kill from PS — TS owns the kill
        );
        if (!mountedRef.current) return;

        if (!res.success || res.data == null) {
          console.warn("[RdpIdle] Poll failed:", JSON.stringify(res));
          return;
        }

        const open = res.data.rdpOpen === true;
        if (!open) {
          // No mstsc → nothing to disconnect. Reset everything and idle the timer.
          if (rdpActiveRef.current) console.log("[RdpIdle] No mstsc.exe running — monitor idle");
          rdpActiveRef.current = false;
          baselineIdleRef.current = -1;
          if (!killInFlightRef.current) killedRef.current = false;
          nativeWarnedRef.current = false;
          dropAlwaysOnTop();
          setSeconds(0);
          setWarning(false);
          setIsIdle(false);
          return;
        }

        if (!rdpActiveRef.current) {
          // RDP just appeared — re-anchor the baseline on the next native tick.
          baselineIdleRef.current = -1;
          const hosts = Array.isArray(res.data.remoteHosts)
            ? res.data.remoteHosts.join(", ")
            : (res.data.remoteHosts || "unknown");
          console.log("[RdpIdle] RDP session active — mstsc:", res.data.processCount ?? "?", "| hosts:", hosts);
        }
        rdpActiveRef.current = true;
      } catch (e) {
        console.error("[RdpIdle] Poll exception:", e);
      }
    };

    poll();
    const pollTimer = setInterval(poll, POLL_MS);

    // ── Native idle tick (1 s): the actual idle timer + kill decision ────────
    const tick = async () => {
      if (!mountedRef.current || tickBusyRef.current) return;
      // A disconnect is in flight — wait for the PS poll to confirm mstsc is gone.
      if (killedRef.current) return;
      // No RDP session → don't count idle.
      if (!rdpActiveRef.current) return;

      tickBusyRef.current = true;
      try {
        let idle: number;
        try {
          idle = await invoke<number>("get_system_idle_seconds");
        } catch (e) {
          console.warn("[RdpIdle] native idle probe failed:", e);
          return; // never advance the counter on a failed reading
        }
        if (!mountedRef.current || killedRef.current || !rdpActiveRef.current) return;

        if (baselineIdleRef.current < 0) baselineIdleRef.current = idle;
        // System-wide idle dropped below the anchor → the user did something.
        // Re-anchor (this is the reset) and clear any active warning.
        if (idle < baselineIdleRef.current) {
          baselineIdleRef.current = idle;
          nativeWarnedRef.current = false;
          dropAlwaysOnTop();
          console.log("[RdpIdle] Activity detected — counter reset");
        }

        const effectiveIdle = Math.max(0, idle - baselineIdleRef.current);
        setSeconds(effectiveIdle);

        if (Date.now() < snoozeUntilRef.current) {
          setWarning(false);
          setIsIdle(false);
          return;
        }

        const warningAt = Math.max(0, timeoutSeconds - effectiveWarningSeconds);
        const inWarning = effectiveIdle >= warningAt && effectiveIdle < timeoutSeconds;
        const timeUntilKill = Math.max(0, timeoutSeconds - effectiveIdle);
        const shouldKill = effectiveIdle >= timeoutSeconds;

        setWarning(inWarning);
        setWarningLeft(timeUntilKill);
        setIsIdle(shouldKill);

        if (inWarning && !nativeWarnedRef.current) {
          nativeWarnedRef.current = true;
          const mins = Math.floor(effectiveIdle / 60);
          const secs = effectiveIdle % 60;
          const idleLabel = mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
          logRdp("warn", `RDP idle ${idleLabel} — warning shown, disconnecting in ${effectiveWarningSeconds}s unless cancelled`);

          await executeBackendCommand("Hide-RDPClientWindow", {}).catch(() => {});
          try {
            const win = getCurrentWindow();
            await win.setAlwaysOnTop(true);
            alwaysOnTopRef.current = true;
            await win.unminimize();
            await win.setFocus();
          } catch { /* best effort */ }

          invoke("show_rdp_idle_warning_native", {
            idleTime: idleLabel,
            secondsLeft: effectiveWarningSeconds,
          }).catch((err) => {
            console.warn("[RdpIdle] custom warning notification failed", err);
          });
        }

        console.log(
          `[RdpIdle] effective=${effectiveIdle}s | idle=${idle}s | base=${baselineIdleRef.current}s | warn=${inWarning} | kill=${shouldKill} | threshold=${timeoutSeconds}s`
        );

        if (shouldKill && !killedRef.current) {
          killedRef.current = true;
          killInFlightRef.current = true; // keep the kill from being undone mid-flight
          setIsIdle(false);
          setWarning(false);
          setSeconds(0);
          baselineIdleRef.current = -1; // re-anchor once the next RDP session starts
          nativeWarnedRef.current = false;
          dropAlwaysOnTop();
          console.log("[RdpIdle] Threshold reached — forcing" + (dismountVaultsOnDisconnect ? " vault dismount then" : "") + " disconnect");

          try {
            if (dismountVaultsOnDisconnect) {
              // Disconnect-RDPClientIdle captures the remote host WHILE the RDP
              // connection is still alive, THEN kills mstsc, THEN dismounts —
              // awaited so the (now synchronous) remote dismount actually
              // completes. It must run BEFORE any local kill: killing mstsc first
              // drops the port-3389 connection that host-capture depends on.
              const res = await executeBackendCommand(
                "Disconnect-RDPClientIdle",
                { DismountServerVaults: true, _ts: Date.now() }
              );
              console.log("[RdpIdle] Disconnect-RDPClientIdle (kill + dismount) result:", JSON.stringify(res));
            } else {
              // No dismount — fast local kill via Rust taskkill.
              const killResult = await invoke<{ killed: boolean; code: number; msg: string }>("kill_mstsc_processes");
              console.log("[RdpIdle] kill_mstsc_processes result:", JSON.stringify(killResult));
            }
            const extras = [
              dismountVaultsOnDisconnect ? "remote vaults dismounted" : null,
              clearCacheOnDisconnect ? "RDP history cleared" : null,
              removeCredsOnDisconnect ? "saved credentials removed" : null,
            ].filter(Boolean).join(", ");
            logRdp("warn", `RDP session disconnected after ${timeoutSeconds}s idle${extras ? ` (${extras})` : ""}`);
          } catch (e) {
            console.error("[RdpIdle] idle disconnect FAILED:", e);
            logRdp("error", `RDP idle disconnect failed: ${e instanceof Error ? e.message : String(e)}`);
            killedRef.current = false; // let it retry next tick
          } finally {
            killInFlightRef.current = false;
          }

          if (clearCacheOnDisconnect) {
            executeBackendCommand(clearCommand("RDPHistory"), {}).catch(() => {});
          }
          if (removeCredsOnDisconnect || clearCacheOnDisconnect) {
            executeBackendCommand(clearCommand("RDPPasswords"), {}).catch(() => {});
          }
          if (saveLog) {
            console.info("[RdpIdle] Disconnect logged at", new Date().toISOString());
          }
        }
      } finally {
        tickBusyRef.current = false;
      }
    };

    const tickTimer = setInterval(tick, TICK_MS);
    tick();

    return () => {
      clearInterval(pollTimer);
      clearInterval(tickTimer);
    };
  }, [
    enabled,
    timeoutSeconds,
    effectiveWarningSeconds,
    clearCacheOnDisconnect,
    removeCredsOnDisconnect,
    saveLog,
    dismountVaultsOnDisconnect,
    disabledReason,
    dropAlwaysOnTop,
  ]);

  return {
    secondsSinceActivity: seconds,
    isIdle,
    warningActive,
    warningSecondsLeft: warningLeft,
    snooze,
  };
}
