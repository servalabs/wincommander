import type { BackendResponse } from "../hooks/useBackend";

export function privacyShieldStopError(
  response: BackendResponse<{ success: boolean; message?: string; error?: boolean | null }>,
): string | null {
  if (response.success && response.data?.success === true && (response.data.error === undefined || response.data.error === null || response.data.error === false)) return null;
  const reason = response.error ?? response.data?.message ?? "";
  if (/machine-state operation is busy|privacy shield policy is still changing|shield policy is transitioning/i.test(reason)) {
    return "Privacy Shield is updating. Wait a moment, then try stopping it again.";
  }
  if (/process identity|status could not be (read|verified)|stop could not be confirmed/i.test(reason)) {
    return "Privacy Shield status could not be verified. Refresh its status, then try again.";
  }
  return "Privacy Shield could not be stopped. Check its status and try again.";
}

export function privacyShieldStopVerificationError(
  response: BackendResponse<{ running?: boolean; error?: boolean | null }>,
): string | null {
  return response.success && response.data?.running === false && (response.data.error === undefined || response.data.error === null || response.data.error === false)
    ? null
    : "Privacy Shield stop could not be confirmed. Check its status and try again.";
}

export async function confirmPrivacyShieldStop(
  response: BackendResponse<{ success: boolean; message?: string; error?: boolean | null; running?: boolean; shieldLifecycle?: string }>,
  readFresh: () => Promise<BackendResponse<{ running?: boolean; error?: boolean | null }>>,
): Promise<string | null> {
  const failure = privacyShieldStopError(response);
  if (failure) return failure;
  if (response.data?.running === false && response.data.shieldLifecycle === "stopped") return null;
  return privacyShieldStopVerificationError(await readFresh());
}
