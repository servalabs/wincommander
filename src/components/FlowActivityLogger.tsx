// FlowActivityLogger — real-time visibility for the flow/automation engine.
//
// Subscribes to monitor-detection and flow-engine events on the Tauri bus and
// prints a safe `[Flow]` summary to the DevTools console. It deliberately never
// mirrors event payloads: those can contain user-authored rule text, action
// details, clipboard labels, paths, or SSIDs.
//
// Mounted globally in App.tsx so it's active regardless of which panel is open.
// Frontend-only; no backend changes needed to see the trail (the events it reads
// are already emitted). Console output is also mirrored to the backend log file
// via the `[Flow]` prefix hook in logger.ts.

import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  clipboardActivitySummary,
  flowDecisionSummary,
  flowExecutionSummary,
  flowNotifySummary,
} from "./flowActivitySummary";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

function log(message: string) {
  console.log(`[Flow] ${message}`);
}

function monitorEvent(feature: string, action: string, errorCode?: string, severity: "info" | "warn" | "error" = "warn") {
  recordDiagnostic({
    feature, action, stage: "detection", lifecycle: "applied",
    outcome: errorCode ? "degraded" : "progress", errorCode, severity,
    retryability: "automatic", suggestedNextAction: "review_status",
    privacyClass: "local_sensitive", context: { reason_category: action },
  });
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
        log(clipboardActivitySummary(e.payload ?? {}));
        monitorEvent("clipboard", "policy_detection", "CLP.POLICY.DETECTED");
      }),
    );
    add(
      listen<{ lookingAway?: boolean }>("privacy-shield-look-state", (e) => {
        log(`DETECT privacy shield → ${e.payload?.lookingAway ? "looking AWAY" : "looking back"}`);
        monitorEvent("privacy_shield", "detector_transition", undefined, "info");
      }),
    );
    add(
      listen("privacy-shield-event", () => {
        log("DETECT Privacy Shield event");
        monitorEvent("privacy_shield", "detection", "PSH.DETECTION");
      }),
    );
    add(
      listen("decoy-accessed", () => {
        log("DETECT decoy tripwire event");
        monitorEvent("decoy", "tripwire", "DCY.TRIPWIRE.DETECTED");
      }),
    );
    add(
      listen("ransomware-detected", () => {
        log("DETECT ransomware activity");
        monitorEvent("ransomware", "detection", "RAN.DETECTION", "error");
      }),
    );
    add(
      listen("wifi-guard-detected", () => {
        log("DETECT Wi-Fi guard event");
        monitorEvent("network", "wifi_guard_detection", "NET.WIFI_GUARD.DETECTED");
      }),
    );

    // ── Flow engine decisions ───────────────────────────────────────────────
    add(
      listen<{ ruleId?: string; reason?: string; message?: string }>("flow-log", (e) => {
        log(flowDecisionSummary(e.payload ?? {}));
        const operationId = typeof e.payload?.ruleId === "string" && /^[A-Za-z0-9_-]{1,110}$/.test(e.payload.ruleId)
          ? `FLOW-${e.payload.ruleId}` : newDiagnosticOperationId("flow");
        const admitted = e.payload?.reason === "admit";
        recordDiagnostic({ operationId, feature: "flow", action: "rule_decision", stage: "admission",
          lifecycle: "acknowledged", outcome: admitted ? "succeeded" : "failed",
          errorCode: admitted ? undefined : "FLW.RULE.REFUSED", severity: admitted ? "info" : "warn",
          retryability: "manual", suggestedNextAction: admitted ? "await_action" : "review_rule",
          privacyClass: "local_sensitive", context: { reason_category: admitted ? "admitted" : "refused" } });
      }),
    );
    add(
      listen<{ flowId?: string; ruleId?: string; completed?: boolean; totalDurationMs?: number }>(
        "flow-executed",
        (e) => {
          log(flowExecutionSummary(e.payload ?? {}));
          const operationId = typeof e.payload?.flowId === "string" && /^[A-Za-z0-9_-]{1,110}$/.test(e.payload.flowId)
            ? `FLOW-${e.payload.flowId}` : newDiagnosticOperationId("flow");
          const completed = e.payload?.completed === true;
          recordDiagnostic({ operationId, feature: "flow", action: "execute", stage: "terminal",
            lifecycle: "applied", outcome: completed ? "succeeded" : "failed",
            errorCode: completed ? undefined : "FLW.ACTION.FAILED", severity: completed ? "info" : "error",
            retryability: completed ? "never" : "manual", suggestedNextAction: completed ? "none" : "review_rule",
            durationMs: typeof e.payload?.totalDurationMs === "number" ? e.payload.totalDurationMs : undefined,
            privacyClass: "local_sensitive", context: { state: completed ? "completed" : "failed" } });
        },
      ),
    );
    add(
      listen<{ message?: string; severity?: string }>("flow-notify", (e) => {
        log(flowNotifySummary(e.payload ?? {}));
        recordDiagnostic({ feature: "flow", action: "notification", stage: "projection", lifecycle: "applied",
          outcome: "progress", severity: "info", retryability: "never", suggestedNextAction: "none",
          privacyClass: "local_sensitive", context: { state: "emitted" } });
      }),
    );

    return () => unlisteners.forEach((u) => u());
  }, []);

  return null;
}
