import { invoke } from "@tauri-apps/api/core";

export type SharedShortcutClaimKind = "calculator" | "hide";
export type SharedShortcutManaged = { desktop: boolean; start: boolean };
type ClaimResult = { status: string; warnings?: string[]; managed?: Partial<SharedShortcutManaged> };
export type SharedShortcutOutcome = { warning: string | null; managed: SharedShortcutManaged };
const MANAGED_WARNING = "Shared WinCommander shortcuts need service reconciliation.";

export function shortcutWarningsAfterReconciliation(warnings: string[], sharedWarning: string | null): string[] {
  return sharedWarning ? warnings : warnings.filter((warning) => warning !== MANAGED_WARNING);
}

/** Collapse only the exact known service rejection; retain combined or unknown warnings. */
export function summarizeStartupShortcutWarnings(warnings: Array<string | null | undefined>): string[] {
  const denial = /^Shared shortcut changes could not be confirmed: (?:Error: )?service rejected request: forbidden \(session-helper peer binary path is not on the allow-list\)(?: \[operation [0-9]+\])?$/;
  const explanation = "Shared shortcuts could not be synchronized because this app is not approved by the Windows service. Use the installed app or the protected development setup.";
  return [...new Set(warnings.filter((warning): warning is string => Boolean(warning)).map((warning) => denial.test(warning) ? explanation : warning))];
}

export async function syncSharedShortcutClaim(
  kind: SharedShortcutClaimKind,
  expectedActive?: boolean,
  send: (kind: SharedShortcutClaimKind) => Promise<ClaimResult> = (claimKind) =>
    invoke("reconcile_shared_shortcut_claim", { kind: claimKind, expectedActive: expectedActive ?? null }),
): Promise<SharedShortcutOutcome> {
  const unavailable = { desktop: false, start: false };
  try {
    const result = await send(kind);
    const managed = result.status === "hidden" || result.status === "partial"
      ? { desktop: result.managed?.desktop === true, start: result.managed?.start === true }
      : unavailable;
    if (result.warnings?.length) return { warning: result.warnings.join(" "), managed };
    if (result.status === "partial") return { warning: "Shared shortcut changes could not be confirmed.", managed };
    if (result.status !== "hidden" && result.status !== "restored") {
      return { warning: "Shared shortcut changes could not be confirmed.", managed: unavailable };
    }
    if (expectedActive && result.status !== "hidden") return { warning: "Shared shortcuts are still visible.", managed: unavailable };
    return { warning: null, managed };
  } catch (error) {
    return { warning: `Shared shortcut changes could not be confirmed: ${String(error)}`, managed: unavailable };
  }
}
