// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from "bun:test";
import { createHash, randomBytes } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { verifyHybridFreeStage } from "./verify-hybrid-free-stage";

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

function preparedRoot() {
  const root = mkdtempSync(join(tmpdir(), "wincmd-free-stage-test-"));
  const source = resolve(import.meta.dirname, "..");
  for (const file of fixtures) {
    const target = join(root, file);
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(join(source, file), target);
  }
  const env: NodeJS.ProcessEnv = {};
  const profiles: Record<string, object> = {};
  const hash = (value: Buffer) => createHash("sha256").update(value).digest("hex");
  for (const purpose of ["LIC", "FREE", "PRO"]) {
    const ed = randomBytes(32), pq = randomBytes(1952);
    const prefix = `WINCMD_PQ_${purpose}_`;
    env[`${prefix}ED_KEY`] = ed.toString("base64url");
    env[`${prefix}MLDSA_KEY`] = pq.toString("base64url");
    env[`${prefix}ED_ID`] = `${purpose}-ed`;
    env[`${prefix}MLDSA_ID`] = `${purpose}-pq`;
    env[`${prefix}EPOCH`] = "7";
    env[`${prefix}ED_PROOF`] = randomBytes(64).toString("base64url");
    env[`${prefix}MLDSA_PROOF`] = randomBytes(3309).toString("base64url");
    profiles[purpose] = { ed_sha256: hash(ed), mldsa_sha256: hash(pq), ed_id: `${purpose}-ed`, mldsa_id: `${purpose}-pq`, epoch: 7 };
  }
  mkdirSync(join(root, "tools"));
  writeFileSync(join(root, "tools/pq-release-approval.json"), JSON.stringify({ version: 1, status: "approved", profiles }));
  return { root, env };
}

describe("hybrid Free stage verification", () => {
  test("hosted publication remains blocked before the first R2 upload", () => {
    const workflow = readFileSync(resolve(import.meta.dirname, "../.github/workflows/release.yml"), "utf8");
    const gate = workflow.indexOf("- name: Block incomplete hybrid Free publication");
    const publish = workflow.indexOf("- name: Publish signed Free release to R2");
    expect(gate).toBeGreaterThan(-1);
    expect(publish).toBeGreaterThan(gate);
    expect(workflow.slice(gate, publish)).toContain("run: throw 'Free publication is disabled");
  });
  test("uses approved verifier trust and forces the verifier offline", () => {
    const { root, env } = preparedRoot();
    let proofs = 0, checks = 0, trustPath = "";
    try {
      verifyHybridFreeStage(root, "release.json", "setup.exe", "3.7.0", "stable", env, {
        proofs(_root, passed) { proofs++; expect(passed.CARGO_NET_OFFLINE).toBe("true"); },
        verify(_root, passed, args) {
          checks++;
          expect(passed.CARGO_NET_OFFLINE).toBe("true");
          trustPath = args[2];
          expect(JSON.parse(readFileSync(trustPath, "utf8"))).toEqual({
            ed_key_id: env.WINCMD_PQ_FREE_ED_ID, ed_public: env.WINCMD_PQ_FREE_ED_KEY,
            pq_key_id: env.WINCMD_PQ_FREE_MLDSA_ID, pq_public: env.WINCMD_PQ_FREE_MLDSA_KEY,
            minimum_epoch: 7,
          });
          expect(args.slice(3)).toEqual(["3.7.0", "stable"]);
        },
      });
      expect(proofs).toBe(1);
      expect(checks).toBe(1);
      expect(() => readFileSync(trustPath)).toThrow();
    } finally { rmSync(root, { recursive: true, force: true }); }
  });

  test("unapproved trust and failed proofs stop before verification", () => {
    const { root, env } = preparedRoot();
    let proofs = 0, checks = 0;
    const dependencies = { proofs() { proofs++; }, verify() { checks++; } };
    try {
      const changed = { ...env, WINCMD_PQ_FREE_ED_KEY: randomBytes(32).toString("base64url") };
      expect(() => verifyHybridFreeStage(root, "r", "i", "3.7.0", "stable", changed, dependencies)).toThrow("Unapproved FREE");
      expect([proofs, checks]).toEqual([0, 0]);
      expect(() => verifyHybridFreeStage(root, "r", "i", "3.7.0", "stable", env, {
        proofs() { throw new Error("invalid possession proof"); }, verify() { checks++; },
      })).toThrow("invalid possession proof");
      expect(checks).toBe(0);
    } finally { rmSync(root, { recursive: true, force: true }); }
  });

  test("invalid arguments or a rejected record never become a successful stage", () => {
    const { root, env } = preparedRoot();
    let calls = 0;
    const dependencies = { proofs() {}, verify() { calls++; throw new Error("bad signature"); } };
    try {
      expect(() => verifyHybridFreeStage(root, "r", "i", "3.07.0", "stable", env, dependencies)).toThrow();
      expect(calls).toBe(0);
      expect(() => verifyHybridFreeStage(root, "r", "i", "3.7.0", "stable", env, dependencies)).toThrow("bad signature");
      expect(calls).toBe(1);
    } finally { rmSync(root, { recursive: true, force: true }); }
  });
});
