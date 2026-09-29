export const MINIMUM_PRO_RELEASE = "3.6.4";

export function parseProReleaseVersion(value: string | null | undefined): number[] | null {
  const normalized = typeof value === "string" ? value.trim().replace(/^v/i, "") : "";
  if (!/^\d+\.\d+\.\d+(?:\.\d+)?$/.test(normalized)) return null;
  const parts = normalized.split(".").map(Number);
  while (parts.length < 4) parts.push(0);
  return parts.every(Number.isSafeInteger) ? parts : null;
}

function compare(left: number[], right: number[]): number {
  for (let index = 0; index < 4; index++) {
    if (left[index] !== right[index]) return left[index] > right[index] ? 1 : -1;
  }
  return 0;
}

export function proReleaseCompatibilityError(
  proVersion: string | null | undefined,
  freeVersion: string | null | undefined,
): string | null {
  const pro = parseProReleaseVersion(proVersion);
  const free = parseProReleaseVersion(freeVersion);
  if (!pro || !free) {
    return "The WinCommander / Pro release version could not be verified. Nothing was installed. Use the matching WinCommander + Pro setup.";
  }
  if (compare(pro, [3, 6, 4, 0]) < 0) {
    return `Compatible Pro ${MINIMUM_PRO_RELEASE} or later is not available from this update source. Nothing was installed. Use the matching WinCommander + Pro setup; reinstalling the older Pro will not fix Vault mounting.`;
  }
  if (compare(pro, free) > 0) {
    return "This Pro release requires a newer WinCommander. Update WinCommander first, then retry. Nothing was installed.";
  }
  return null;
}
