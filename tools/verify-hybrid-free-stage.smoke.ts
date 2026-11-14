// SPDX-License-Identifier: AGPL-3.0-or-later
// Disposable real-key smoke test. Pass the directory with the compiled Rust binaries.
import { strict as assert } from "node:assert";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { trustProofMessage } from "./pq-release-profile";
import { verifyHybridFreeStage } from "./verify-hybrid-free-stage";

const binaries = process.argv[2];
if (!binaries || process.argv.length !== 3) throw new Error("Specify the directory containing the compiled Rust tools");
const exe = (name: string) => join(resolve(binaries), `${name}${process.platform === "win32" ? ".exe" : ""}`);
const source = resolve(import.meta.dirname, "..");
const temp = mkdtempSync(join(tmpdir(), "wincmd-free-real-stage-"));
const root = join(temp, "root");
const p = (name: string) => join(temp, name);
const run = (program: string, args: string[], env: NodeJS.ProcessEnv = process.env) => {
  const result = Bun.spawnSync([exe(program), ...args], { env: { ...env, CARGO_NET_OFFLINE: "true" }, stdout: "pipe", stderr: "pipe" });
  if (result.exitCode !== 0) throw new Error(`${program} rejected input`);
};
try {
  const fixtures = [
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
  for (const file of fixtures) {
    const target = join(root, file);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(source, file), target);
  }
  const env: NodeJS.ProcessEnv = { ...process.env };
  for (const key of Object.keys(env)) if (key.startsWith("WINCMD_PQ_")) delete env[key];
  const profiles: Record<string, object> = {};
  let freeEd: CryptoKey | undefined, freePq: CryptoKey | undefined;
  const hash = (value: string) => createHash("sha256").update(Buffer.from(value, "base64url")).digest("hex");
  for (const purpose of ["LIC", "FREE", "PRO"]) {
    const ed = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]);
    const pq = await crypto.subtle.generateKey("ML-DSA-65", true, ["sign", "verify"]);
    const prefix = `WINCMD_PQ_${purpose}_`;
    env[`${prefix}ED_KEY`] = Buffer.from(await crypto.subtle.exportKey("raw", ed.publicKey)).toString("base64url");
    env[`${prefix}MLDSA_KEY`] = Buffer.from(await crypto.subtle.exportKey("raw-public", pq.publicKey)).toString("base64url");
    env[`${prefix}ED_ID`] = `${purpose}-ed`; env[`${prefix}MLDSA_ID`] = `${purpose}-pq`; env[`${prefix}EPOCH`] = "7";
    const message = trustProofMessage(purpose as "LIC" | "FREE" | "PRO", env);
    env[`${prefix}ED_PROOF`] = Buffer.from(await crypto.subtle.sign("Ed25519", ed.privateKey, message)).toString("base64url");
    env[`${prefix}MLDSA_PROOF`] = Buffer.from(await crypto.subtle.sign("ML-DSA-65", pq.privateKey, message)).toString("base64url");
    profiles[purpose] = { ed_sha256: hash(env[`${prefix}ED_KEY`]!), mldsa_sha256: hash(env[`${prefix}MLDSA_KEY`]!),
      ed_id: env[`${prefix}ED_ID`], mldsa_id: env[`${prefix}MLDSA_ID`], epoch: 7 };
    if (purpose === "FREE") { freeEd = ed.privateKey; freePq = pq.privateKey; }
  }
  if (!freeEd || !freePq) throw new Error("Free keys missing");
  mkdirSync(join(root, "tools"));
  writeFileSync(join(root, "tools/pq-release-approval.json"), JSON.stringify({ version: 1, status: "approved", profiles }));
  const fixture = JSON.parse(readFileSync(join(source, "src-tauri/wincmd-pq-experiment/tests/fixtures/free-tauri-disposable.json"), "utf8"));
  const version = fixture.version as string;
  writeFileSync(p("setup.exe"), fixture.installer);
  writeFileSync(p("tauri.json"), JSON.stringify({ plugins: { updater: { pubkey: fixture.publicKey } } }));
  writeFileSync(p("latest.json"), JSON.stringify({ version, platforms: { "windows-x86_64": { signature: fixture.signature,
    url: `https://winupdates.servalabs.com/free/v${version}/WinCommander_${version}_x64-setup.exe` } } }));
  const trust = { ed_key_id: env.WINCMD_PQ_FREE_ED_ID, ed_public: env.WINCMD_PQ_FREE_ED_KEY,
    pq_key_id: env.WINCMD_PQ_FREE_MLDSA_ID, pq_public: env.WINCMD_PQ_FREE_MLDSA_KEY, minimum_epoch: 7 };
  writeFileSync(p("trust.json"), JSON.stringify(trust));
  const prepare = "wincmd-pq-free-release-prepare";
  run(prepare, ["stage", p("latest.json"), p("setup.exe"), p("tauri.json"), version, "stable", p("payload.json")]);
  run(prepare, ["prepare", p("payload.json"), p("setup.exe"), p("trust.json"), version, "stable", "7", p("ed.msg"), p("pq.msg")]);
  writeFileSync(p("ed.sig"), Buffer.from(await crypto.subtle.sign("Ed25519", freeEd, readFileSync(p("ed.msg")))));
  writeFileSync(p("pq.sig"), Buffer.from(await crypto.subtle.sign("ML-DSA-65", freePq, readFileSync(p("pq.msg")))));
  run(prepare, ["finish", p("payload.json"), p("setup.exe"), p("trust.json"), version, "stable", "7", p("ed.sig"), p("pq.sig"), p("signed.json")]);
  let verifications = 0;
  const dependencies = {
    proofs(_root: string, passed: NodeJS.ProcessEnv) { run("wincmd-pq-trust-check", [], passed); },
    verify(_root: string, passed: NodeJS.ProcessEnv, args: string[]) { verifications++; run("wincmd-pq-free-release-check", args, passed); },
  };
  verifyHybridFreeStage(root, p("signed.json"), p("setup.exe"), version, "stable", env, dependencies);
  const tampered = JSON.parse(readFileSync(p("signed.json"), "utf8"));
  const signature = tampered.signatures[0].signature as string;
  tampered.signatures[0].signature = (signature[0] === "A" ? "B" : "A") + signature.slice(1);
  writeFileSync(p("tampered.json"), JSON.stringify(tampered));
  assert.throws(() => verifyHybridFreeStage(root, p("tampered.json"), p("setup.exe"), version, "stable", env, dependencies));
  writeFileSync(p("setup.exe"), "different installer");
  assert.throws(() => verifyHybridFreeStage(root, p("signed.json"), p("setup.exe"), version, "stable", env, dependencies));
  const unapproved = { ...env, WINCMD_PQ_FREE_ED_KEY: env.WINCMD_PQ_LIC_ED_KEY };
  const before = verifications;
  assert.throws(() => verifyHybridFreeStage(root, p("signed.json"), p("setup.exe"), version, "stable", unapproved, dependencies));
  assert.equal(verifications, before);
  const badProof = { ...env, WINCMD_PQ_FREE_ED_PROOF: Buffer.alloc(64, 1).toString("base64url") };
  assert.throws(() => verifyHybridFreeStage(root, p("signed.json"), p("setup.exe"), version, "stable", badProof, dependencies));
  assert.equal(verifications, before);
  console.log("Disposable real-key Free stage verification and negative cases passed.");
} finally { rmSync(temp, { recursive: true, force: true }); }
