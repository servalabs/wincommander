import { createHash } from "node:crypto";

export function pairedServiceHash(service: Uint8Array, expected: string): string {
  if (!/^[0-9a-fA-F]{64}$/.test(expected) || /^0{64}$/.test(expected)) {
    throw new Error("A nonzero final service SHA-256 is required.");
  }
  const actual = createHash("sha256").update(service).digest("hex");
  if (actual !== expected.toLowerCase()) throw new Error("Final service bytes do not match the paired SHA-256.");
  return actual;
}

export function pairedSearchTemplate(template: string, settingsLab = false): string {
  if (!template.includes('!include "{{installer_hooks}}"') || template.includes('!define WC_PAIRED_SEARCH_RELEASE')) {
    throw new Error("Unexpected NSIS template; paired writer checks cannot be bound.");
  }
  return '!define WC_PAIRED_SEARCH_RELEASE\n' + (settingsLab ? '!define WC_PAIRED_SETTINGS_LAB\n' : '') + template;
}
