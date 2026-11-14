// SPDX-License-Identifier: AGPL-3.0-or-later
// Disposable cross-runtime keys only; never reads or produces production keys.
import { trustProofMessage, validatePqReleaseTrust } from "./pq-release-profile";
import { createHash } from "node:crypto";

const env: Record<string, string> = {};
for (const name of ["LIC", "FREE", "PRO"] as const) {
  const ed = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]);
  const pq = await crypto.subtle.generateKey("ML-DSA-65", true, ["sign", "verify"]);
  env[`WINCMD_PQ_${name}_ED_KEY`] = Buffer.from(await crypto.subtle.exportKey("raw", ed.publicKey)).toString("base64url");
  env[`WINCMD_PQ_${name}_MLDSA_KEY`] = Buffer.from(await crypto.subtle.exportKey("raw-public", pq.publicKey)).toString("base64url");
  env[`WINCMD_PQ_${name}_ED_ID`] = `${name}-ed`;
  env[`WINCMD_PQ_${name}_MLDSA_ID`] = `${name}-pq`;
  env[`WINCMD_PQ_${name}_EPOCH`] = "7";
  const message = trustProofMessage(name, env);
  env[`WINCMD_PQ_${name}_ED_PROOF`] = Buffer.from(await crypto.subtle.sign("Ed25519", ed.privateKey, message)).toString("base64url");
  env[`WINCMD_PQ_${name}_MLDSA_PROOF`] = Buffer.from(await crypto.subtle.sign("ML-DSA-65", pq.privateKey, message)).toString("base64url");
}
const profiles: Record<string, { ed_sha256: string; mldsa_sha256: string; ed_id: string; mldsa_id: string; epoch: number }> = {};
for (const p of ["LIC", "FREE", "PRO"]) profiles[p] = {
  ed_sha256: createHash("sha256").update(Buffer.from(env[`WINCMD_PQ_${p}_ED_KEY`], "base64url")).digest("hex"),
  mldsa_sha256: createHash("sha256").update(Buffer.from(env[`WINCMD_PQ_${p}_MLDSA_KEY`], "base64url")).digest("hex"),
  ed_id: env[`WINCMD_PQ_${p}_ED_ID`], mldsa_id: env[`WINCMD_PQ_${p}_MLDSA_ID`], epoch: 7,
};
validatePqReleaseTrust(env, new Set(), { version: 1, status: "approved", profiles });
const baseEnv = { ...process.env };
for (const key of Object.keys(baseEnv)) if (key.startsWith("WINCMD_PQ_")) delete baseEnv[key];
const run = (values: Record<string, string>) => Bun.spawnSync([
  "cargo", "run", "--quiet", "--locked", "--manifest-path", "src-tauri/Cargo.toml", "-p", "wincmd-pq-experiment", "--bin", "wincmd-pq-trust-check",
], { cwd: process.cwd(), env: { ...baseEnv, ...values, AWS_LC_SYS_PREBUILT_NASM: "1" }, stdout: "pipe", stderr: "pipe" });
const valid = run(env);
if (valid.exitCode !== 0) throw new Error(`Rust rejected disposable trust proofs: ${valid.stderr.toString()}`);
const signature = Buffer.from(env.WINCMD_PQ_LIC_MLDSA_PROOF, "base64url");
signature[0] ^= 1;
const damaged = run({ ...env, WINCMD_PQ_LIC_MLDSA_PROOF: signature.toString("base64url") });
if (damaged.exitCode === 0) throw new Error("Rust accepted a damaged ML-DSA proof");
const wrongPurpose = run({ ...env, WINCMD_PQ_FREE_ED_PROOF: env.WINCMD_PQ_LIC_ED_PROOF });
if (wrongPurpose.exitCode === 0) throw new Error("Rust accepted a proof from another purpose");
console.log("Disposable Bun/Rust release trust proof crosswire passed");
