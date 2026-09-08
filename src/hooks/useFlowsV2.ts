// useFlowsV2 — state + IPC for the v2 (Pro-backed) flows engine.
//
// All mutations go through the Free `flow_bridge` Tauri commands, which persist
// to `settings.app.proFlows` and re-sync the whole set to the Pro engine. Live
// activity (`flow-executed`, `flow-log`, `flow-notify`) streams in via events.

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Rule } from "../panels/flows/rules";
import { newDiagnosticOperationId, recordDiagnostic } from "../lib/diagnostics";

export interface FlowLogEntry {
  id: number;
  at: string;
  kind: "executed" | "admit" | "refused" | "notify";
  ruleId?: string;
  message: string;
}

export interface FlowsState {
  rules: Rule[];
  commands: string[];
  loading: boolean;
  error: string | null;
  log: FlowLogEntry[];
}

const MAX_LOG = 60;

// Rust's own dispatch_paid_command bounds a single Pro round-trip (handshake
// + request, times up to 2 retry attempts on transport error) at ~5 minutes
// worst case — but that's still a hard bound, not a hang. This is the
// frontend's independent safety net: without it, RuleEditor's `saving` state
// (set true, cleared only in a catch) has no way out if the awaited invoke()
// legitimately never settles (e.g. Pro deadlocked, an orphaned pipe that
// never errors), which read to users as infinite/stuck "Saving…".
// Promise.race can't cancel the underlying invoke — Tauri IPC has no abort
// hook here — it just stops the UI from waiting on it forever.
const RULE_MUTATION_TIMEOUT_MS = 90_000;

function withTimeout<T>(promise: Promise<T>, ms: number, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${Math.round(ms / 1000)}s`)),
      ms,
    );
    promise.then(
      (v) => {
        clearTimeout(timer);
        resolve(v);
      },
      (e) => {
        clearTimeout(timer);
        reject(e);
      },
    );
  });
}

export function useFlowsV2(hasPaid: boolean) {
  const [state, setState] = useState<FlowsState>({
    rules: [],
    commands: [],
    loading: true,
    error: null,
    log: [],
  });
  const logSeq = useRef(0);

  const pushLog = useCallback((entry: Omit<FlowLogEntry, "id" | "at">) => {
    logSeq.current += 1;
    const full: FlowLogEntry = { ...entry, id: logSeq.current, at: new Date().toISOString() };
    setState((s) => ({ ...s, log: [full, ...s.log].slice(0, MAX_LOG) }));
  }, []);

  const reload = useCallback(async () => {
    if (!hasPaid) {
      setState((s) => ({ ...s, loading: false }));
      return;
    }
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const [rules, commands] = await Promise.all([
        invoke<Rule[]>("flow_list_rules"),
        invoke<string[]>("list_backend_commands").catch(() => [] as string[]),
      ]);
      setState((s) => ({ ...s, rules, commands, loading: false }));
    } catch (e) {
      setState((s) => ({ ...s, loading: false, error: String(e) }));
    }
  }, [hasPaid]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Live activity stream.
  useEffect(() => {
    if (!hasPaid) return;
    const unlisteners: UnlistenFn[] = [];
    void (async () => {
      unlisteners.push(
        await listen<{ ruleId?: string; actionCount?: number }>("flow-executed", (e) => {
          pushLog({ kind: "executed", ruleId: e.payload?.ruleId, message: `ran ${e.payload?.actionCount ?? 0} action(s)` });
        }),
      );
      unlisteners.push(
        await listen<{ ruleId?: string; reason?: string; message?: string }>("flow-log", (e) => {
          const reason = e.payload?.reason ?? "";
          const kind = reason === "admit" ? "admit" : "refused";
          pushLog({ kind, ruleId: e.payload?.ruleId, message: e.payload?.message ?? reason });
        }),
      );
      unlisteners.push(
        await listen<{ message?: string }>("flow-notify", (e) => {
          pushLog({ kind: "notify", message: e.payload?.message ?? "" });
        }),
      );
    })();
    return () => unlisteners.forEach((u) => u());
  }, [hasPaid, pushLog]);

  const saveRule = useCallback(
    async (rule: Rule) => {
      const operationId = newDiagnosticOperationId("flow");
      recordDiagnostic({ operationId, feature: "flow", action: "save_rule", stage: "request", lifecycle: "requested", outcome: "started", severity: "info", retryability: "automatic", suggestedNextAction: "await_result", privacyClass: "local_sensitive", context: { state: "requested" } });
      try {
        await withTimeout(invoke("flow_save_rule", { rule }), RULE_MUTATION_TIMEOUT_MS, "Saving flow");
        recordDiagnostic({ operationId, feature: "flow", action: "save_rule", stage: "persistence", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive", context: { state: "saved" } });
        await reload();
      } catch (error) {
        recordDiagnostic({ operationId, feature: "flow", action: "save_rule", stage: "persistence", lifecycle: "applied", outcome: "failed", errorCode: "FLW.RULE.SAVE_FAILED", severity: "error", retryability: "manual", suggestedNextAction: "retry", privacyClass: "local_sensitive", context: { reason_category: "persistence" } });
        throw error;
      }
    },
    [reload],
  );

  const deleteRule = useCallback(
    async (ruleId: string) => {
      const operationId = newDiagnosticOperationId("flow");
      recordDiagnostic({ operationId, feature: "flow", action: "delete_rule", stage: "request", lifecycle: "requested", outcome: "started", severity: "info", retryability: "automatic", suggestedNextAction: "await_result", privacyClass: "local_sensitive", context: { state: "requested" } });
      try {
        await withTimeout(invoke("flow_delete_rule", { ruleId }), RULE_MUTATION_TIMEOUT_MS, "Deleting flow");
        recordDiagnostic({ operationId, feature: "flow", action: "delete_rule", stage: "persistence", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive", context: { state: "deleted" } });
        await reload();
      } catch (error) {
        recordDiagnostic({ operationId, feature: "flow", action: "delete_rule", stage: "persistence", lifecycle: "applied", outcome: "failed", errorCode: "FLW.RULE.DELETE_FAILED", severity: "error", retryability: "manual", suggestedNextAction: "retry", privacyClass: "local_sensitive", context: { reason_category: "persistence" } });
        throw error;
      }
    },
    [reload],
  );

  const setEnabled = useCallback(
    async (ruleId: string, enabled: boolean) => {
      const operationId = newDiagnosticOperationId("flow");
      recordDiagnostic({ operationId, feature: "flow", action: "set_enabled", stage: "request", lifecycle: "requested", outcome: "started", severity: "info", retryability: "automatic", suggestedNextAction: "await_result", privacyClass: "local_sensitive", context: { state: enabled ? "enabled" : "disabled" } });
      try {
        await withTimeout(invoke("flow_set_enabled", { ruleId, enabled }), RULE_MUTATION_TIMEOUT_MS, "Updating flow");
        recordDiagnostic({ operationId, feature: "flow", action: "set_enabled", stage: "persistence", lifecycle: "applied", outcome: "succeeded", severity: "info", retryability: "never", suggestedNextAction: "none", privacyClass: "local_sensitive", context: { state: enabled ? "enabled" : "disabled" } });
        await reload();
      } catch (error) {
        recordDiagnostic({ operationId, feature: "flow", action: "set_enabled", stage: "persistence", lifecycle: "applied", outcome: "failed", errorCode: "FLW.RULE.UPDATE_FAILED", severity: "error", retryability: "manual", suggestedNextAction: "retry", privacyClass: "local_sensitive", context: { reason_category: "persistence" } });
        throw error;
      }
    },
    [reload],
  );

  const fireNow = useCallback(async (ruleId: string) => {
    const operationId = newDiagnosticOperationId("flow");
    recordDiagnostic({ operationId, feature: "flow", action: "execute", stage: "request", lifecycle: "requested", outcome: "started", severity: "info", retryability: "automatic", suggestedNextAction: "await_result", privacyClass: "local_sensitive", context: { state: "requested" } });
    try {
      await invoke("flow_fire_now", { ruleId });
      recordDiagnostic({ operationId, feature: "flow", action: "execute", stage: "delivery", lifecycle: "delivered", outcome: "progress", severity: "info", retryability: "automatic", suggestedNextAction: "await_result", privacyClass: "local_sensitive", context: { state: "delivered" } });
    } catch (error) {
      recordDiagnostic({ operationId, feature: "flow", action: "execute", stage: "delivery", lifecycle: "delivered", outcome: "failed", errorCode: "FLW.ACTION.DISPATCH_FAILED", severity: "error", retryability: "manual", suggestedNextAction: "retry", privacyClass: "local_sensitive", context: { reason_category: "dispatch" } });
      throw error;
    }
  }, []);

  return { ...state, reload, saveRule, deleteRule, setEnabled, fireNow };
}
