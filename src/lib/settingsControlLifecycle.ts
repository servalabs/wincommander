export type ControlLifecycle =
  | "Desired"
  | "Applying"
  | "Applied"
  | "Blocked"
  | "Failed"
  | "Not supported";

export interface ControlAccountIdentity {
  name: string;
  displayName?: string;
  sid?: string;
}

export interface ControlLifecycleInput {
  applying?: boolean;
  /** A user-facing failure from the write or its read-back. */
  failureReason?: string | null;
  /** Explicitly gate a machine-only control until service routing exists. */
  needsElevation?: boolean;
  supported?: boolean;
  desired?: boolean | null;
  /** Undefined means no Windows read-back has completed yet. */
  observed?: boolean | null;
  /** Identity returned by the live Windows account/profile lookup. */
  account?: ControlAccountIdentity | null;
}

export interface ControlLifecycleState {
  state: ControlLifecycle;
  reason: string | null;
  account: ControlAccountIdentity | null;
}

export type ControlLifecycleTone = "neutral" | "success" | "warning" | "danger" | "info";

const BLOCKED_REASON = /decoy|access denied|permission denied|not authorized|(?:needs?|requires?)[\s-]*elevation|store[\s-]*(?:is[\s-]*)?read[\s-]*only|read[\s-]*only/i;

function stateFor(reason: string): "Blocked" | "Failed" {
  return BLOCKED_REASON.test(reason) ? "Blocked" : "Failed";
}

/**
 * Pure control model shared by settings writes and generic settings surfaces.
 * `Applied` requires a matching Windows read-back; a saved intent is `Desired`.
 */
export function getControlLifecycle({
  applying = false,
  failureReason = null,
  needsElevation = false,
  supported = true,
  desired = null,
  observed = null,
  account = null,
}: ControlLifecycleInput): ControlLifecycleState {
  const withContext = (state: ControlLifecycle, reason: string | null): ControlLifecycleState => ({
    state,
    reason,
    account,
  });

  if (!supported) return withContext("Not supported", null);
  if (needsElevation) return withContext("Blocked", "needs-elevation");
  if (applying) return withContext("Applying", null);
  if (failureReason) return withContext(stateFor(failureReason), failureReason);
  if (observed === null || desired === null || observed !== desired) return withContext("Desired", null);
  return withContext("Applied", null);
}

/** Presentation metadata so generic setting UI does not invent state styling. */
export function getControlLifecycleTone(state: ControlLifecycle): ControlLifecycleTone {
  switch (state) {
    case "Applied": return "success";
    case "Applying": return "info";
    case "Desired":
    case "Blocked": return "warning";
    case "Failed": return "danger";
    case "Not supported": return "neutral";
  }
}
