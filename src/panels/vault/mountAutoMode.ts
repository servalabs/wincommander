import type { BackendResponse, MountVolumeParams, MountVolumeResult } from "@/hooks/useBackend";

type MountAttempt = (params: MountVolumeParams) => Promise<BackendResponse<MountVolumeResult>>;

const hiddenFallbackIsSafe = (result: BackendResponse<MountVolumeResult>) =>
  !result.success && result.error?.includes("vault_engine_unlock_failed") === true;

/**
 * The native encrypted-volume engine needs a mount mode, while the normal
 * Secure Storage UI deliberately does not expose container internals. A
 * read-only standard attempt is harmless to a hidden volume; only a password
 * rejection falls back to hidden mode. All other failures keep their original
 * error instead of masking a driver, service, or drive-letter problem.
 */
export async function mountPasswordSelectedVolume(
  mount: MountAttempt,
  params: Omit<MountVolumeParams, "volumeKind" | "volumeRole">,
): Promise<BackendResponse<MountVolumeResult>> {
  const standard = await mount({
    ...params,
    volumeKind: "standard",
    volumeRole: "standard",
  });
  if (!hiddenFallbackIsSafe(standard)) return standard;

  return mount({
    ...params,
    volumeKind: "dual",
    volumeRole: "hidden",
  });
}
