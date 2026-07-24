// Pure helpers for the selective crypto-erase picker. No IPC, no React —
// unit-tested in cryptoEraseTargets.test.ts. Mirrors the server-side OS
// derivation in selective_erase.rs (OS = system drive) so the UI and the
// backend agree on which targets need the nuclear ceremony.
import type { BitLockerVolume } from "../hooks/useBackend";

export interface EncryptedTarget {
  id: string;
  kind: "veracrypt" | "bitlocker";
  label: string;
  path?: string;
  mountLetter?: string;
  mountPoint?: string;
  isOsVolume: boolean;
  escrowRisk?: boolean;
  protectionOn?: boolean;
  eligible: boolean;
  reason?: string;
}

type VeraVolume = { letter: string; path: string | null; type: string };

const normDrive = (s: string): string => s.trim().replace(/\\+$/, "").toUpperCase();

/** Mirrors the server (selective_erase.rs is_system_device_path): a raw device
 *  namespace path (\\.\... or \\?\...) is an OS/system-class VeraCrypt target
 *  that requires the typed nuclear ack. A plain file container is data. */
export const isVeraCryptDevicePath = (p: string | null | undefined): boolean =>
  !!p && /^\\\\[.?]\\/.test(p.trim());

/** A BitLocker recovery key that could survive protector removal (escrowed). */
export function escrowRiskOf(v: BitLockerVolume): boolean {
  return v.recoveryPasswordPresent || v.backupUsed;
}

/** OS/system volumes require the heavy "won't boot again" ceremony. */
export function requiresNuclear(t: EncryptedTarget): boolean {
  return t.isOsVolume;
}

export function buildTargets(
  veracrypt: VeraVolume[],
  bitlocker: BitLockerVolume[],
  systemDrive: string,
): EncryptedTarget[] {
  const sys = normDrive(systemDrive || "C:");
  const out: EncryptedTarget[] = [];

  for (const v of veracrypt) {
    const hasPath = !!(v.path && v.path.trim() && v.path !== "Encrypted Volume");
    out.push({
      id: `vc:${v.letter}`,
      kind: "veracrypt",
      label: `VeraCrypt ${v.letter}`,
      path: hasPath ? (v.path as string) : undefined,
      mountLetter: v.letter,
      isOsVolume: isVeraCryptDevicePath(v.path),
      eligible: hasPath,
      reason: hasPath ? undefined : "Container path unknown — remount from its file to erase it.",
    });
  }

  for (const b of bitlocker) {
    const isOs = b.volumeType === "OperatingSystem" || normDrive(b.mountPoint) === sys;
    out.push({
      id: `bl:${b.mountPoint}`,
      kind: "bitlocker",
      label: `BitLocker ${normDrive(b.mountPoint)}`,
      mountPoint: b.mountPoint,
      isOsVolume: isOs,
      escrowRisk: escrowRiskOf(b),
      protectionOn: b.volumeStatus !== "FullyDecrypted",
      eligible: true,
    });
  }

  return out;
}
