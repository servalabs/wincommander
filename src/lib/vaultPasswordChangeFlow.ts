import {
  EMPTY_HEADER_CREDENTIALS,
  POLICY_MANAGED,
  WRONG_DECOY_PASSWORD,
  WRONG_PASSWORD,
  currentPasswordProblem,
  isPolicyManagedError,
  newPasswordProblem,
  vaultPasswordChangeErrorMessage,
  type HeaderCredentials,
  type PasswordChangeStage,
  type VaultPasswordHeader,
} from "./vaultPasswordChange";

/** current: one password box; new: new password for the header it opened; decoy-ask/decoy: optional decoy follow-up after a real-password change. */
export type PasswordChangeStep = "current" | "new" | "decoy-ask" | "decoy" | "done" | "policy";

export interface PasswordChangeFlow {
  step: PasswordChangeStep;
  volumePath: string;
  /** Set by the probe; null until the current password opened a header. */
  header: VaultPasswordHeader | null;
  /** The password typed first, and its new value. */
  first: HeaderCredentials;
  /** The decoy password in the follow-up after a real-password change. */
  decoy: HeaderCredentials;
  /** The just-set real password, held only until the decoy follow-up ends so the engine can refuse a decoy equal to it. */
  provenReal: HeaderCredentials | null;
  busy: boolean;
  success: string;
  failure: string;
}

export type PasswordChangeEvent =
  | { type: "path"; volumePath: string }
  | { type: "edit"; part: "first" | "decoy"; field: keyof HeaderCredentials; value: string }
  | { type: "send" }
  | { type: "probed"; header: VaultPasswordHeader }
  | { type: "changed" }
  | { type: "failed"; error: unknown }
  | { type: "decoy-yes" }
  | { type: "decoy-no" }
  | { type: "notice"; failure: string }
  | { type: "reset" };

export const PASSWORD_CHANGED = "Password changed.";
export const REAL_PASSWORD_CHANGED = "Password changed. Your real password is now the new one; your decoy password still works.";
export const DECOY_QUESTION = "Also change the decoy password?";
export const DECOY_PASSWORD_CHANGED = "Decoy password changed. Your new real password still works.";

export function startPasswordChange(volumePath = ""): PasswordChangeFlow {
  return { step: "current", volumePath, header: null, first: EMPTY_HEADER_CREDENTIALS, decoy: EMPTY_HEADER_CREDENTIALS, provenReal: null, busy: false, success: "", failure: "" };
}

export function passwordChangeStage(step: PasswordChangeStep): PasswordChangeStage | null {
  return step === "current" ? "probe" : step === "new" ? "change" : step === "decoy" ? "decoy" : null;
}

/** Returns the first problem with the visible form, or null when it can be sent. */
export function passwordChangeProblem(flow: PasswordChangeFlow): string | null {
  if (flow.step === "current") return currentPasswordProblem(flow.volumePath, flow.first);
  if (flow.step === "new") return newPasswordProblem(flow.first);
  if (flow.step === "decoy") return newPasswordProblem(flow.decoy, "decoy ");
  return null;
}

const provenFrom = (header: HeaderCredentials): HeaderCredentials =>
  ({ ...EMPTY_HEADER_CREDENTIALS, oldPassword: header.newPassword, oldPim: header.newPim, oldKeyfile: header.newKeyfile });
const withoutNew = (header: HeaderCredentials) => ({ ...header, newPassword: "", confirmPassword: "" });
const withoutCurrent = (header: HeaderCredentials) => ({ ...header, oldPassword: "" });

function failed(flow: PasswordChangeFlow, error: unknown): PasswordChangeFlow {
  const stage = passwordChangeStage(flow.step) ?? "change";
  const settled = { ...flow, busy: false };
  if (isPolicyManagedError(error)) {
    return { ...settled, step: "policy", first: EMPTY_HEADER_CREDENTIALS, decoy: EMPTY_HEADER_CREDENTIALS, provenReal: null, success: "", failure: POLICY_MANAGED };
  }
  const failure = vaultPasswordChangeErrorMessage(error, stage);
  if (stage === "decoy") {
    return { ...settled, decoy: failure === WRONG_DECOY_PASSWORD ? withoutCurrent(flow.decoy) : flow.decoy, failure };
  }
  // A current password that stops opening the vault after the probe sends the person back to the first box.
  if (stage === "probe" || failure === WRONG_PASSWORD) {
    return { ...settled, step: "current", header: null, first: withoutCurrent(withoutNew(flow.first)), provenReal: null, failure };
  }
  return { ...settled, provenReal: null, failure };
}

function changed(flow: PasswordChangeFlow): PasswordChangeFlow {
  const settled = { ...flow, busy: false, failure: "" };
  if (flow.step === "decoy") return { ...settled, step: "done", decoy: EMPTY_HEADER_CREDENTIALS, provenReal: null, success: DECOY_PASSWORD_CHANGED };
  return flow.header === "hidden"
    ? { ...settled, step: "decoy-ask", first: EMPTY_HEADER_CREDENTIALS, success: REAL_PASSWORD_CHANGED }
    : { ...settled, step: "done", first: EMPTY_HEADER_CREDENTIALS, provenReal: null, success: PASSWORD_CHANGED };
}

export function passwordChangeReducer(flow: PasswordChangeFlow, event: PasswordChangeEvent): PasswordChangeFlow {
  switch (event.type) {
    case "path":
      return flow.step === "current" ? { ...flow, volumePath: event.volumePath, failure: "" } : flow;
    case "edit":
      return { ...flow, [event.part]: { ...flow[event.part], [event.field]: event.value }, failure: "" };
    case "send":
      // New passwords leave state once the request is built; the current one stays for the engine's re-check.
      return flow.step === "new" ? { ...flow, busy: true, failure: "", first: withoutNew(flow.first), provenReal: flow.header === "hidden" ? provenFrom(flow.first) : null }
        : flow.step === "decoy" ? { ...flow, busy: true, failure: "", decoy: withoutNew(flow.decoy) }
          : { ...flow, busy: true, failure: "" };
    case "probed":
      return flow.step === "current" ? { ...flow, busy: false, step: "new", header: event.header, failure: "" } : flow;
    case "changed":
      return flow.step === "new" || flow.step === "decoy" ? changed(flow) : flow;
    case "failed":
      return failed(flow, event.error);
    case "decoy-yes":
      return flow.step === "decoy-ask" ? { ...flow, step: "decoy", decoy: EMPTY_HEADER_CREDENTIALS } : flow;
    case "decoy-no":
      return flow.step === "decoy-ask" ? { ...flow, step: "done", provenReal: null } : flow;
    case "notice":
      return { ...flow, failure: event.failure };
    case "reset":
      return startPasswordChange();
  }
}
