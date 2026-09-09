import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Rule } from "../types/generated/fleet";

export interface ClipboardGuardPolicy {
  policy_version: number;
  rules: Rule[];
}

export type ClipboardGuardRuleLoadState = "loading" | "ready" | "degraded";

export function localClipboardGuardPolicy(rules: Rule[]): ClipboardGuardPolicy {
  return { policy_version: 0, rules };
}

/** The backend validates, atomically activates, then persists this policy. */
export async function saveLocalClipboardRules(
  rules: Rule[],
  save: (policy: ClipboardGuardPolicy) => Promise<void>,
): Promise<void> {
  await save(localClipboardGuardPolicy(rules));
}

/**
 * Keeps the two policy sources separate. Local rules come from encrypted
 * per-user storage; Fleet rules are read-only runtime state. A failed read
 * retains the last known UI state rather than suggesting protection vanished.
 */
export default function useClipboardGuardRules() {
  const [localRules, setLocalRules] = useState<Rule[]>([]);
  const [fleetRules, setFleetRules] = useState<Rule[]>([]);
  const [localRuleLoadState, setLocalRuleLoadState] = useState<ClipboardGuardRuleLoadState>("loading");

  const saveLocalRules = useCallback(async (rules: Rule[]) => {
    await saveLocalClipboardRules(
      rules,
      (policy) => invoke<void>("save_local_clipboard_guard_rules", { policy }),
    );
    setLocalRules(rules);
    setLocalRuleLoadState("ready");
  }, []);

  useEffect(() => {
    let active = true;
    invoke<ClipboardGuardPolicy>("load_local_clipboard_guard_rules")
      .then((policy) => {
        if (active) {
          setLocalRules(policy.rules);
          setLocalRuleLoadState("ready");
        }
      })
      .catch(() => {
        // Do not show an unavailable encrypted store as an empty rules list.
        // Built-in checks remain separate from this custom-rule state.
        if (active) setLocalRuleLoadState("degraded");
      });
    invoke<ClipboardGuardPolicy>("get_managed_clipboard_guard_rules")
      .then((policy) => {
        if (active) setFleetRules(policy.rules);
      })
      .catch(() => {
        // The service retains the last valid Fleet policy. Keep this UI's
        // last successful value too; an unavailable read must not weaken it.
      });
    return () => { active = false; };
  }, []);

  return { localRules, fleetRules, localRuleLoadState, saveLocalRules };
}
