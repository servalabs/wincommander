// src/hooks/useVerifyUsbBoot.ts
//
// Typed wrapper around the F6 USB-boot self-test IPC commands invoked from
// src/panels/privacy/VerifyUsbBootDialog.tsx. See that file's header for the
// full safety contract this mirrors — arming writes a REAL, validly-signed
// wipe token; there is no "test mode" in the wire format.
//
// Exposed as a plain module-level object (not a React hook with per-render
// identity) since every entry is a stateless IPC call with no captured
// props/state — same shape as src/hooks/useArgus.ts.

import { invoke } from "@tauri-apps/api/core";

export interface VerifyUsbBootArmResult {
  usbRoot: string;
  bootEntryId: string;
  nonceHex: string;
  expiresAtUnix: number;
  warning: string;
}

export interface VerifyUsbBootCheckResult {
  consumed: boolean;
  reason?: string;
  bootNextCleared?: boolean;
}

export interface VerifyUsbBootStatusResult {
  armed: boolean;
  usbRoot?: string;
  bootEntryId?: string;
  nonceHex?: string;
  expiresAtUnix?: number;
}

export const verifyUsbBoot = {
  status: () => invoke<VerifyUsbBootStatusResult>("f6_verify_usb_boot_status"),
  arm: () => invoke<VerifyUsbBootArmResult>("f6_verify_usb_boot_arm"),
  disarm: () => invoke<void>("f6_verify_usb_boot_disarm"),
  check: (usbRoot: string, nonceHex: string) =>
    invoke<VerifyUsbBootCheckResult>("f6_verify_usb_boot_check", { usbRoot, nonceHex }),
};

export default verifyUsbBoot;
