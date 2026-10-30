import type { BackendResponse } from "../hooks/useBackend";

export function privacyShieldStopError(
  response: BackendResponse<{ success: boolean; message?: string; error?: boolean | null }>,
): string | null {
  if (response.success && response.data?.success === true && (response.data.error === undefined || response.data.error === null || response.data.error === false)) return null;
  const reason = response.error ?? response.data?.message ?? "";
  if (reason.includes("machine-state operation is busy")) {
    return "Privacy Shield is updating. Wait a moment, then try stopping it again.";
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
