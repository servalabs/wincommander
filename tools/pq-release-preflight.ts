// SPDX-License-Identifier: AGPL-3.0-or-later
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { normalizedPqReleaseEnvironment, validatePqReleaseTrust } from "./pq-release-profile";

const disposableTrustFiles = [
  "src-tauri/commander-free/tests/fixtures/hybrid-pro-disposable.json",
  "src-tauri/commander-svc/src/fixtures/hybrid-licence-test.json",
  "src-tauri/commander-svc/src/fixtures/hybrid-licence-invalid-test.json",
  "src-tauri/commander-svc/src/fixtures/workers-hybrid-licence.json",
  "src-tauri/commander-svc/src/fixtures/workers-enrolment-issuer.json",
  "src-tauri/commander-svc/src/fixtures/workers-enrolment-receipt.json",
  "src-tauri/commander-svc/src/fixtures/workers-device-renewal.json",
  "src-tauri/wincmd-pq-experiment/tests/cloudflare-jws-v1.json",
  "src-tauri/wincmd-pq-experiment/tests/cloudflare-jws-signer-v1.json",
];

export function validateReleaseTrust(root: string, env: NodeJS.ProcessEnv): void {
  const disposableKeys = new Set<string>();
  for (const file of disposableTrustFiles) {
    const fixture = JSON.parse(readFileSync(resolve(root, file), "utf8")) as Record<string, unknown>;
    for (const key of [fixture.publicEd, fixture.publicPq]) if (typeof key === "string") disposableKeys.add(key);
  }
  const approval = JSON.parse(readFileSync(resolve(root, "tools/pq-release-approval.json"), "utf8"));
  validatePqReleaseTrust(env, disposableKeys, approval);
}

export function verifyReleaseTrustProofs(root: string, env: NodeJS.ProcessEnv): void {
  const result = Bun.spawnSync(["cargo", "run", "--locked", "--manifest-path", "src-tauri/Cargo.toml",
    "-p", "wincmd-pq-experiment", "--bin", "wincmd-pq-trust-check"], {
    cwd: root,
    env: { ...normalizedPqReleaseEnvironment(env), AWS_LC_SYS_PREBUILT_NASM: env.AWS_LC_SYS_PREBUILT_NASM ?? "1" },
    stdout: "inherit", stderr: "inherit",
  });
  if (result.exitCode !== 0) throw new Error(`Independent release trust proof verification failed with exit code ${result.exitCode}`);
}

if (import.meta.main) {
  const root = resolve(import.meta.dirname, "..");
  validateReleaseTrust(root, process.env);
  verifyReleaseTrustProofs(root, process.env);
}
