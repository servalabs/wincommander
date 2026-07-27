// State + IPC for the firewall audit card. Extracted out of the component so
// the card stays presentation-only, and so the two things the previous inline
// version dropped — the per-rule `errors[]` reasons and the `.wfw` backup path —
// have somewhere to live.
import { useCallback, useMemo, useRef, useState } from "react";
import { useBackend, type FirewallAudit, type FirewallRule } from "../../hooks/useBackend";
import { classifyFirewallError } from "../../lib/firewallAuditCopy";
import {
  buildRemediationMessage,
  type FirewallAction,
  type FirewallOutcome,
} from "../../lib/firewallAuditView";
import type { ErrorAdvice } from "../../lib/arpDiagnostics";
import { useNetworkSessionState } from "./networkSessionState";

export interface RemediationReport {
  action: FirewallAction;
  outcome: FirewallOutcome;
  /** One line per rule the backend refused or failed to change. Previously only
   *  counted, never shown, so a user could not find out which rule failed. */
  failures: string[];
  /** Path of the .wfw firewall export taken before the change. */
  backupPath: string | null;
  cancelled: boolean;
}

export function useFirewallAudit() {
  const backend = useBackend();
  const backendRef = useRef(backend);
  backendRef.current = backend;

  // audit/selected live in the cross-tab/cross-navigation session-state map
  // (not useState): switching away from the Diagnostics tab unmounts this
  // hook, and a plain useState would force a re-audit every time the user
  // came back — see networkSessionState.ts.
  const [audit, setAudit] = useNetworkSessionState<FirewallAudit | undefined>("network.firewall.audit", undefined);
  const [selected, setSelected] = useNetworkSessionState<Set<string>>("network.firewall.selected", new Set());
  const [auditing, setAuditing] = useState(false);
  const [applying, setApplying] = useState<FirewallAction | null>(null);
  const [error, setError] = useState<ErrorAdvice>();
  const [report, setReport] = useState<RemediationReport>();

  const rules = useMemo<FirewallRule[]>(() => audit?.rules ?? [], [audit]);

  const inspect = useCallback(async () => {
    setAuditing(true);
    setError(undefined);
    setReport(undefined);
    try {
      const next = await backendRef.current.firewallAuditPreview();
      setAudit(next);
      setSelected(new Set());
      // A scan that succeeded at the IPC level can still carry an error string
      // (unsupported locale, netsh failure) — that is a real error, not a result.
      if (next.error) setError(classifyFirewallError(next.error));
    } catch (cause) {
      setError(classifyFirewallError(String(cause)));
      setAudit(undefined);
    } finally {
      setAuditing(false);
    }
  }, []);

  const cancel = useCallback(async () => {
    await backendRef.current.firewallAuditCancel().catch(() => {});
  }, []);

  const remediate = useCallback(
    async (action: FirewallAction) => {
      if (selected.size === 0) return;
      setApplying(action);
      setError(undefined);
      setReport(undefined);
      try {
        const result = await backendRef.current.firewallAuditRemediate([...selected], action);
        setReport({
          action,
          outcome: buildRemediationMessage(action, result),
          failures: result.errors,
          backupPath: result.backupPath,
          cancelled: result.cancelled,
        });
        // Re-preview so the list reflects what actually changed. A failure here
        // must not wipe the report the user needs to read.
        const next = await backendRef.current.firewallAuditPreview().catch(() => undefined);
        if (next) {
          setAudit(next);
          setSelected(new Set());
        }
      } catch (cause) {
        setError(classifyFirewallError(String(cause)));
      } finally {
        setApplying(null);
      }
    },
    [selected],
  );

  const toggle = useCallback((id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const setSelection = useCallback((ids: string[]) => setSelected(new Set(ids)), []);

  const selectedRules = useMemo<FirewallRule[]>(
    () => rules.filter((rule) => selected.has(rule.id)),
    [rules, selected],
  );

  return {
    audit,
    rules,
    selected,
    selectedRules,
    auditing,
    applying,
    busy: auditing || applying !== null,
    error,
    report,
    inspect,
    cancel,
    remediate,
    toggle,
    setSelection,
  };
}
