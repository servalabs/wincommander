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
  /** BitLocker only — false once the volume is FullyDecrypted, at which point
   *  there are no keys left to destroy. Drives eligibility, not just display. */
  isEncrypted?: boolean;
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
    const isEncrypted = b.volumeStatus !== "FullyDecrypted";
    out.push({
      id: `bl:${b.mountPoint}`,
      kind: "bitlocker",
      label: `BitLocker ${normDrive(b.mountPoint)}`,
      mountPoint: b.mountPoint,
      isOsVolume: isOs,
      escrowRisk: escrowRiskOf(b),
      isEncrypted,
      // A fully decrypted volume holds no keys, so a crypto-erase would be a
      // destructive no-op. Refuse it in the UI rather than firing the Pro call.
      eligible: isEncrypted,
      reason: isEncrypted ? undefined : "Not encrypted — there are no keys to destroy.",
    });
  }

  return out;
}

/** The real Windows system drive, derived from the BitLocker volume list.
 *  WHY: the server reads %SystemDrive% (selective_erase.rs system_drive) and
 *  builds the nuclear ack token from it. The UI used to hardcode "C:", which is
 *  wrong on a machine that boots from another letter — the user would be told
 *  to type a token the server rejects. The OperatingSystem volume's mount point
 *  IS the system drive, so prefer it and only fall back to "C:". */
export function deriveSystemDrive(bitlocker: BitLockerVolume[]): string {
  const os = bitlocker.find((b) => b.volumeType === "OperatingSystem");
  return os ? normDrive(os.mountPoint) : "C:";
}

/** The exact token the server will compare the typed ack against — mirrors the
 *  `resolved` computation in selective_erase.rs erase_encrypted_container. */
export function expectedAckToken(target: EncryptedTarget, systemDrive: string): string {
  const sys = normDrive(systemDrive || "C:");
  if (target.kind === "bitlocker") return normDrive(target.mountPoint || sys);
  return sys;
}

/** What is physically on screen as "the thing being destroyed" — a container
 *  path for VeraCrypt, a mount point for BitLocker. Shown next to the ack token
 *  so the user is never asked to type a letter with no visible relationship to
 *  the target. */
export function targetSubject(target: EncryptedTarget): string {
  return target.path || target.mountPoint || target.mountLetter || target.label;
}

/** The method that will actually be applied, shown before the user commits
 *  (same idea as DriveWipeDialog's wipeMethodLabel). */
export function eraseMethodLabel(target: EncryptedTarget): string {
  return target.kind === "veracrypt"
    ? "VeraCrypt · overwrite the primary + backup header with random bytes"
    : "BitLocker · remove every key protector, then lock the volume to evict its key";
}

/** The limitation the user deserves to know BEFORE committing — same register
 *  as DriveWipeDialog's "Best-effort only" callout. Each string is grounded in
 *  a documented gap in selective_erase.rs, not invented reassurance. */
export function eraseLimitation(target: EncryptedTarget): string {
  if (target.kind === "veracrypt") {
    return "The overwrite is issued and flushed to disk, but WinCommander does not re-mount the container afterwards to prove the password stops working. On a large volume the backup header near the end of the file is not independently re-read either.";
  }
  if (target.isOsVolume) {
    return "The volume Windows is running from cannot be locked, so its key stays in memory until this machine powers off. The data becomes unrecoverable once this session ends — not the instant you confirm.";
  }
  return "If Windows cannot lock the volume afterwards, its key stays in memory and the volume stays readable until you reboot or dismount it. The receipt states which of the two happened.";
}

/** Consequence bullets for the confirmation ceremony. */
export function eraseConsequences(target: EncryptedTarget): string[] {
  const subject = targetSubject(target);
  const shared = [
    target.kind === "veracrypt"
      ? `The header of ${subject} is overwritten, so the password can never open it again`
      : `Every BitLocker key protector on ${subject} is removed`,
    "The data stays physically on the disk but becomes permanently unreadable",
    "A backup of the container or a copy of the password does not help — the keys are gone",
  ];
  if (target.isOsVolume) {
    shared.push("This is the volume Windows boots from — the machine will not start after a restart");
  }
  if (target.escrowRisk) {
    shared.push(
      "A recovery key for this volume is escrowed (Entra/AD or a Microsoft account) — until you revoke it there, the volume is still recoverable",
    );
  }
  return shared;
}
