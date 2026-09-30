import type { VaultSaveAccessDirectoryResponse } from "./accessControlTypes";
import { summarizeReconcileResults, type ReconcileOutcome } from "./accessControlPolicy";

/** The parent verifies the durable directory by readback. The service omits
 * unchanged outsider groups from reconciliation results; don't invent failures
 * for those. Any reported failure or malformed result remains an error. */
export function accessGroupSaveOutcome(saved: VaultSaveAccessDirectoryResponse): ReconcileOutcome {
  const unknown: ReconcileOutcome = {
    intent: "danger",
    message: "WinCommander could not confirm that all groups were saved. Refresh the saved groups before trying again.",
  };
  if (!saved || !Array.isArray(saved.directory?.groups) || !Array.isArray(saved.results)) return unknown;
  const names = new Set<string>();
  for (const result of saved.results) {
    if (!result || typeof result.local_group !== "string" || !["created", "updated", "unchanged", "failed"].includes(result.state)) return unknown;
    const name = result.local_group.toLowerCase();
    if (names.has(name)) return unknown;
    names.add(name);
  }
  const outcome = summarizeReconcileResults(saved.results);
  const changed = new Set(saved.results.filter(result => result.state === "created" || result.state === "updated").map(result => result.local_group.toLowerCase()));
  const affected = saved.directory.groups.filter(group => changed.has(group.local_group.toLowerCase()));
  const labels = affected.slice(0, 3).map(group => `"${group.name.replace(/[\r\n\t]/g, " ").slice(0, 60)}"`);
  const heading = affected.length === 1 ? `Group ${labels[0]} saved on this PC.`
    : affected.length > 1 ? `${affected.length} access groups saved on this PC: ${labels.join(", ")}${affected.length > 3 ? ", …" : ""}.`
      : "Saved group settings confirmed on this PC.";
  return outcome.intent === "danger" ? outcome : {
    intent: "success",
    message: `${heading} Saved groups are available to authorized Windows administrators and can be reused in Vault permissions.`,
  };
}
