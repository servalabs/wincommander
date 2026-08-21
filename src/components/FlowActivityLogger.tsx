// FlowActivityLogger — real-time visibility for the flow/automation engine.
//
// Subscribes to every monitor-detection and flow-engine event on the Tauri bus
// and prints a clear `[Flow]` line to the DevTools console for each. This is the
// human-visible companion to the encrypted backend log: open DevTools → Console
// and you see, live, whether a detector fired, whether a rule matched, whether
// it was admitted or refused, and exactly which command ran.
//
// Mounted globally in App.tsx so it's active regardless of which panel is open.
// Frontend-only; no backend changes needed to see the trail (the events it reads
// are already emitted). Console output is also mirrored to the backend log file
// via the `[Flow]` prefix hook in logger.ts.

import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

function log(message: string, data?: unknown) {
  if (data === undefined) {
    console.log(`[Flow] ${message}`);
  } else {
    console.log(`[Flow] ${message}`, data);
  }
}

export default function FlowActivityLogger() {
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    const add = (p: Promise<UnlistenFn>) =>
      p.then((fn) => unlisteners.push(fn)).catch(() => {});

    log("activity logger armed — watching detectors + flow engine");

    // ── Detectors (the "is it checking / did it fire" signals) ──────────────
    add(
      listen<{ pattern?: string; severity?: string }>("paste-monitor-detected", (e) => {
        log(`DETECT clipboard secret → "${e.payload?.pattern ?? "?"}" (${e.payload?.severity ?? "?"})`);
      }),
    );
    add(
      listen<{ lookingAway?: boolean }>("privacy-shield-look-state", (e) => {
        log(`DETECT privacy shield → ${e.payload?.lookingAway ? "looking AWAY" : "looking back"}`);
      }),
    );
    add(
      listen<{ kind?: string }>("privacy-shield-event", (e) => {
        log(`DETECT gaze event → kind="${e.payload?.kind ?? "?"}" (forwarded to flow engine)`);
      }),
    );
    add(
      listen<{ path?: string }>("decoy-accessed", (e) => {
        log(`DETECT decoy file accessed → ${e.payload?.path ?? "?"}`);
      }),
    );
    add(
      listen("ransomware-detected", () => log("DETECT ransomware activity")),
    );
    add(
      listen<{ ssid?: string }>("wifi-guard-detected", (e) => {
        log(`DETECT rogue Wi-Fi → ${e.payload?.ssid ?? "?"}`);
      }),
    );

    // ── Flow engine decisions ───────────────────────────────────────────────
    add(
      listen<{ ruleId?: string; reason?: string; message?: string }>("flow-log", (e) => {
        const reason = e.payload?.reason ?? "";
        const verb = reason === "admit" ? "ADMIT" : "SKIP";
        log(`${verb} rule ${e.payload?.ruleId ?? "?"} — ${e.payload?.message ?? reason}`);
      }),
    );
    add(
      listen<{ ruleId?: string; ruleName?: string; actionCount?: number; actions?: unknown[] }>(
        "flow-executed",
        (e) => {
          const p = e.payload ?? {};
          log(
            `EXECUTED rule "${p.ruleName ?? p.ruleId ?? "?"}" — ${p.actionCount ?? 0} action(s):`,
            p.actions,
          );
        },
      ),
    );
    add(
      listen<{ message?: string; severity?: string }>("flow-notify", (e) => {
        log(`NOTIFY (${e.payload?.severity ?? "info"}) "${e.payload?.message ?? ""}"`);
      }),
    );

    return () => unlisteners.forEach((u) => u());
  }, []);

  return null;
}
