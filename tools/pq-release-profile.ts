// SPDX-License-Identifier: AGPL-3.0-or-later
// Release trust is public, but must be supplied from an independently reviewed source.
import { createHash } from "node:crypto";

export const DESKTOP_PQ_FEATURES = [
  "experimental-pq-licence",
  "experimental-pq-free-release",
  "experimental-pq-pro-release",
] as const;
export const SERVICE_PQ_FEATURES = ["experimental-pq-licence"] as const;

export function serviceBuildCommand(): string[] {
  return ["cargo", "build", "--manifest-path", "src-tauri/Cargo.toml", "-p", "commander-svc", "--release", "--features", SERVICE_PQ_FEATURES.join(",")];
}

export function desktopBuildCommand(configPath: string): string[] {
  return ["bun", "x", "--package", "@tauri-apps/cli@2.12.4", "tauri", "build", "--config", configPath,
    "--features", DESKTOP_PQ_FEATURES.join(",")];
}

type Environment = Readonly<Record<string, string | undefined>>;
type Purpose = "LIC" | "FREE" | "PRO";
const purposes: readonly Purpose[] = ["LIC", "FREE", "PRO"];
type ApprovedKeys = { ed_sha256: string; mldsa_sha256: string; ed_id: string; mldsa_id: string; epoch: number };
type Approval = { version: number; status: string; profiles: Partial<Record<Purpose, ApprovedKeys & { old?: ApprovedKeys }>> };
const fingerprint = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

function matchesApproval(actual: ApprovedKeys, approved: ApprovedKeys | undefined): boolean {
  return !!approved && approved.ed_sha256 === actual.ed_sha256 && approved.mldsa_sha256 === actual.mldsa_sha256
    && approved.ed_id === actual.ed_id && approved.mldsa_id === actual.mldsa_id && approved.epoch === actual.epoch;
}

function decodedKey(env: Environment, name: string, size: number): Buffer {
  const text = env[name];
  if (!text || !/^[A-Za-z0-9_-]+$/.test(text)) throw new Error(`Invalid or missing ${name}`);
  const bytes = Buffer.from(text, "base64url");
  if (bytes.length !== size || bytes.toString("base64url") !== text || bytes.every((byte) => byte === 0))
    throw new Error(`Invalid or missing ${name}`);
  return bytes;
}

/** Public, deterministic bytes to be signed independently by each trust key. */
export function trustProofMessage(purpose: Purpose, env: Environment): Uint8Array {
  const p = `WINCMD_PQ_${purpose}_`;
  decodedKey(env, `${p}ED_KEY`, 32); decodedKey(env, `${p}MLDSA_KEY`, 1952);
  const edId = env[`${p}ED_ID`] ?? ""; const pqId = env[`${p}MLDSA_ID`] ?? ""; const epoch = env[`${p}EPOCH`] ?? "";
  if (!/^[A-Za-z0-9._-]{1,128}$/.test(edId) || !/^[A-Za-z0-9._-]{1,128}$/.test(pqId) || edId === pqId
      || !/^[1-9][0-9]*$/.test(epoch) || !Number.isSafeInteger(Number(epoch))) throw new Error(`Invalid ${purpose} trust`);
  return new TextEncoder().encode(`WinCommander release trust proof v1\npurpose=${purpose}\ned_id=${env[`${p}ED_ID`]}\npq_id=${env[`${p}MLDSA_ID`]}\nepoch=${env[`${p}EPOCH`]}\ned_key=${env[`${p}ED_KEY`]}\npq_key=${env[`${p}MLDSA_KEY`]}\n`);
}

/** Fail before building if any component would lack its required trust. */
export function validatePqReleaseTrust(env: Environment, forbiddenPublicKeys: ReadonlySet<string>, approval: Approval): void {
  if (!approval || approval.version !== 1 || approval.status !== "approved" || !approval.profiles)
    throw new Error("Release trust approval is not configured");
  const seenEd = new Set<string>();
  const seenPq = new Set<string>();
  for (const purpose of purposes) {
    const prefix = `WINCMD_PQ_${purpose}_`;
    const ed = decodedKey(env, `${prefix}ED_KEY`, 32).toString("base64url");
    const pq = decodedKey(env, `${prefix}MLDSA_KEY`, 1952).toString("base64url");
    if (forbiddenPublicKeys.has(ed) || forbiddenPublicKeys.has(pq)) throw new Error(`Disposable ${purpose} trust cannot be used in a release`);
    if (seenEd.has(ed) || seenPq.has(pq)) throw new Error("Release trust keys must be distinct across purposes");
    seenEd.add(ed); seenPq.add(pq);
    const edId = env[`${prefix}ED_ID`] ?? "";
    const pqId = env[`${prefix}MLDSA_ID`] ?? "";
    if (!/^[A-Za-z0-9._-]{1,128}$/.test(edId) || !/^[A-Za-z0-9._-]{1,128}$/.test(pqId) || edId === pqId)
      throw new Error(`Invalid ${purpose} key identifiers`);
    const epoch = env[`${prefix}EPOCH`] ?? "";
    if (!/^[1-9][0-9]*$/.test(epoch) || !Number.isSafeInteger(Number(epoch)))
      throw new Error(`Invalid ${purpose} trust epoch`);
    const approved = approval.profiles[purpose];
    if (!matchesApproval({ ed_sha256: fingerprint(Buffer.from(ed, "base64url")), mldsa_sha256: fingerprint(Buffer.from(pq, "base64url")),
      ed_id: edId, mldsa_id: pqId, epoch: Number(epoch) }, approved)) throw new Error(`Unapproved ${purpose} release trust`);
    decodedKey(env, `${prefix}ED_PROOF`, 64);
    decodedKey(env, `${prefix}MLDSA_PROOF`, 3309);
    const oldFields = ["ED_KEY", "MLDSA_KEY", "ED_ID", "MLDSA_ID", "MAX_EPOCH"].map((field) => `${prefix}OLD_${field}`);
    if (oldFields.some((name) => !!env[name])) {
      const oldEd = decodedKey(env, oldFields[0], 32).toString("base64url");
      const oldPq = decodedKey(env, oldFields[1], 1952).toString("base64url");
      const oldEdId = env[oldFields[2]] ?? ""; const oldPqId = env[oldFields[3]] ?? "";
      const max = env[oldFields[4]] ?? "";
      if (forbiddenPublicKeys.has(oldEd) || forbiddenPublicKeys.has(oldPq)
          || (oldEd === ed && oldPq === pq)
          || !/^[A-Za-z0-9._-]{1,128}$/.test(oldEdId) || !/^[A-Za-z0-9._-]{1,128}$/.test(oldPqId) || oldEdId === oldPqId
          || !/^[1-9][0-9]*$/.test(max) || !Number.isSafeInteger(Number(max)) || Number(max) >= Number(epoch))
        throw new Error(`Invalid historical ${purpose} trust`);
      if (!matchesApproval({ ed_sha256: fingerprint(Buffer.from(oldEd, "base64url")), mldsa_sha256: fingerprint(Buffer.from(oldPq, "base64url")),
        ed_id: oldEdId, mldsa_id: oldPqId, epoch: Number(max) }, approved?.old)) throw new Error(`Unapproved historical ${purpose} trust`);
    } else if (approved?.old) {
      throw new Error(`Missing historical ${purpose} trust`);
    }
  }
}

/** Empty optional GitHub variables must not become present empty Rust trust values. */
export function normalizedPqReleaseEnvironment(env: Environment): Record<string, string | undefined> {
  const copy = { ...env };
  for (const purpose of purposes) for (const field of ["ED_KEY", "MLDSA_KEY", "ED_ID", "MLDSA_ID", "MAX_EPOCH"]) {
    const name = `WINCMD_PQ_${purpose}_OLD_${field}`;
    if (copy[name] === "" || copy[name] === undefined) delete copy[name];
  }
  return copy;
}
