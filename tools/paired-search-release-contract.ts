import { createHash } from "node:crypto";

// This helper only prepares an explicit experimental packaging plan. It does
// not establish that the supplied service was built with the paired Pro image.
export function pairedSearchTemplate(template: string): string {
  if (!template.includes('!include "{{installer_hooks}}"') ||
      template.includes("!define WC_EXPERIMENTAL_SEARCH_FENCE") ||
      template.includes("!define WC_EXPERIMENTAL_USER_STAGE_ONLY")) {
    throw new Error("Unexpected NSIS template; cannot bind maintenance hooks.");
  }
  return '!define WC_EXPERIMENTAL_USER_STAGE_ONLY\n!define WC_EXPERIMENTAL_SEARCH_FENCE\n' + template;
}

export function pairedServiceHash(service: Uint8Array, expected: string): string {
  if (!/^[0-9a-fA-F]{64}$/.test(expected) || /^0{64}$/.test(expected)) {
    throw new Error("A nonzero final service SHA-256 is required.");
  }
  const actual = createHash("sha256").update(service).digest("hex");
  if (actual !== expected.toLowerCase()) throw new Error("Final service bytes do not match the paired SHA-256.");
  return actual;
}
