import type { ProInstallStatus, ProManifest } from "../hooks/useProInstall";
import { parseProReleaseVersion } from "./proReleaseCompatibility";

/** Hash differences identify different builds, not which build is newer. */
export function shouldAutomaticallyReplacePro(
  status: Pick<ProInstallStatus, "installed" | "local_version" | "local_sha256"> | null,
  manifest: Pick<ProManifest, "version" | "sha256"> | null,
): boolean {
  if (!status?.installed || !manifest?.sha256) return false;
  const installed = parseProReleaseVersion(status.local_version);
  const offered = parseProReleaseVersion(manifest.version);
  if (!installed || !offered) return false;
  if (status.local_sha256?.toLowerCase() === manifest.sha256.toLowerCase()) return false;
  for (let index = 0; index < 4; index++) {
    const currentPart = installed[index] ?? 0;
    const offeredPart = offered[index] ?? 0;
    if (offeredPart !== currentPart) return offeredPart > currentPart;
  }
  return false;
}
