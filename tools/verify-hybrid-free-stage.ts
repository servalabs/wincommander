// SPDX-License-Identifier: AGPL-3.0-or-later
// Verify a locally prepared record against the approved release trust configuration.
// This does not attest which keys were embedded in a previously built installer.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { validateReleaseTrust, verifyReleaseTrustProofs } from "./pq-release-preflight";

type Env = NodeJS.ProcessEnv;
type Dependencies = {
  proofs: (root: string, env: Env) => void;
  verify: (root: string, env: Env, args: string[]) => void;
};

const realDependencies: Dependencies = {
  proofs: verifyReleaseTrustProofs,
  verify(root, env, args) {
    const result = Bun.spawnSync([
      "cargo", "run", "--locked", "--offline", "--manifest-path", "src-tauri/Cargo.toml",
      "-p", "wincmd-pq-experiment", "--features", "experimental-offline-verifier",
      "--bin", "wincmd-pq-free-release-check", "--", ...args,
    ], { cwd: root, env, stdout: "inherit", stderr: "inherit" });
    if (result.exitCode !== 0) throw new Error("Hybrid Free release verification failed");
  },
};

export function verifyHybridFreeStage(
  root: string, record: string, installer: string, version: string, channel: string,
  env: Env, dependencies: Dependencies = realDependencies,
): void {
  if (!record || !installer || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)
      || !["stable", "beta"].includes(channel)) throw new Error("Invalid Free stage arguments");
  // Approval and possession checks precede the verifier and all network-capable publishing.
  validateReleaseTrust(root, env);
  const offlineEnv = { ...env, CARGO_NET_OFFLINE: "true" };
  dependencies.proofs(root, offlineEnv);
  const trust = {
    ed_key_id: env.WINCMD_PQ_FREE_ED_ID,
    ed_public: env.WINCMD_PQ_FREE_ED_KEY,
    pq_key_id: env.WINCMD_PQ_FREE_MLDSA_ID,
    pq_public: env.WINCMD_PQ_FREE_MLDSA_KEY,
    minimum_epoch: Number(env.WINCMD_PQ_FREE_EPOCH),
  };
  const directory = mkdtempSync(join(tmpdir(), "wincmd-free-stage-trust-"));
  try {
    const trustFile = join(directory, "trust.json");
    writeFileSync(trustFile, JSON.stringify(trust), { flag: "wx", mode: 0o600 });
    dependencies.verify(root, offlineEnv, [resolve(record), resolve(installer), trustFile, version, channel]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  try {
    const [record, installer, version, channel, ...extra] = process.argv.slice(2);
    if (extra.length || !record || !installer || !version || !channel) throw new Error("Invalid Free stage arguments");
    verifyHybridFreeStage(resolve(import.meta.dirname, ".."), record, installer, version, channel, process.env);
    console.log("Hybrid Free record matches approved verifier trust and the staged installer.");
  } catch (error) {
    console.error(error instanceof Error ? error.message : "Hybrid Free stage verification failed");
    process.exitCode = 1;
  }
}
