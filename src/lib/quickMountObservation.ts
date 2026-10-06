import type { EncryptionStatus } from "@/hooks/useBackend";

export function canOpenQuickMountDrive(
  status: EncryptionStatus | null,
  statusError: string | null,
  drive: string | null,
  containerPath: string | undefined,
): boolean {
  if (statusError || !drive || !containerPath) return false;
  const normalize = (value: string) => value.replace(/^\\\\\?\\/, "").replace(/\//g, "\\").toLowerCase();
  return status?.volumes?.some(volume => volume.accessible === true
    && volume.letter.replace(/:$/, "").toLowerCase() === drive.replace(/:$/, "").toLowerCase()
    && !!volume.path && normalize(volume.path) === normalize(containerPath)) === true;
}
