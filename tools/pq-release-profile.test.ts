// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DESKTOP_PQ_FEATURES, SERVICE_PQ_FEATURES, desktopBuildCommand, normalizedPqReleaseEnvironment, serviceBuildCommand, trustProofMessage, validatePqReleaseTrust } from "./pq-release-profile";

async function trust(): Promise<Record<string, string>> {
  const env: Record<string, string> = {};
  for (const name of ["LIC", "FREE", "PRO"] as const) {
    const ed = await crypto.subtle.generateKey("Ed25519", true, ["sign", "verify"]);
    const pq = await crypto.subtle.generateKey("ML-DSA-65", true, ["sign", "verify"]);
    env[`WINCMD_PQ_${name}_ED_KEY`] = Buffer.from(await crypto.subtle.exportKey("raw", ed.publicKey)).toString("base64url");
    env[`WINCMD_PQ_${name}_MLDSA_KEY`] = Buffer.from(await crypto.subtle.exportKey("raw-public", pq.publicKey)).toString("base64url");
    env[`WINCMD_PQ_${name}_ED_ID`] = `${name}-ed`;
    env[`WINCMD_PQ_${name}_MLDSA_ID`] = `${name}-pq`;
    env[`WINCMD_PQ_${name}_EPOCH`] = "1";
    const message = trustProofMessage(name, env);
    env[`WINCMD_PQ_${name}_ED_PROOF`] = Buffer.from(await crypto.subtle.sign("Ed25519", ed.privateKey, message)).toString("base64url");
    env[`WINCMD_PQ_${name}_MLDSA_PROOF`] = Buffer.from(await crypto.subtle.sign("ML-DSA-65", pq.privateKey, message)).toString("base64url");
  }
  return env;
}

function approval(env: Record<string, string>) {
  type Keys = { ed_sha256: string; mldsa_sha256: string; ed_id: string; mldsa_id: string; epoch: number };
  const profiles: Record<string, Keys & { old?: Keys }> = {};
  for (const p of ["LIC", "FREE", "PRO"]) profiles[p] = {
    ed_sha256: createHash("sha256").update(Buffer.from(env[`WINCMD_PQ_${p}_ED_KEY`], "base64url")).digest("hex"),
    mldsa_sha256: createHash("sha256").update(Buffer.from(env[`WINCMD_PQ_${p}_MLDSA_KEY`], "base64url")).digest("hex"),
    ed_id: env[`WINCMD_PQ_${p}_ED_ID`], mldsa_id: env[`WINCMD_PQ_${p}_MLDSA_ID`], epoch: Number(env[`WINCMD_PQ_${p}_EPOCH`]),
  };
  return { version: 1, status: "approved", profiles };
}

describe("paired post-quantum release configuration", () => {
  test("requires distinct, complete trust for desktop and service", async () => {
    const env = await trust();
    const approved = approval(env);
    expect(() => validatePqReleaseTrust(env, new Set(), approved)).not.toThrow();
    expect(DESKTOP_PQ_FEATURES).toContain("experimental-pq-licence");
    expect(DESKTOP_PQ_FEATURES).toContain("experimental-pq-free-release");
    expect(DESKTOP_PQ_FEATURES).toContain("experimental-pq-pro-release");
    expect(SERVICE_PQ_FEATURES).toEqual(["experimental-pq-licence"]);
    const service = serviceBuildCommand();
    const desktop = desktopBuildCommand("config.json");
    expect(service[service.indexOf("--features") + 1]).toBe("experimental-pq-licence");
    expect(desktop[desktop.indexOf("--features") + 1].split(",")).toEqual([...DESKTOP_PQ_FEATURES]);
    for (const field of Object.keys(env)) {
      const missing = { ...env }; delete missing[field];
      expect(() => validatePqReleaseTrust(missing, new Set(), approved), field).toThrow();
    }
  });
  test("rejects malformed, reused and disposable trust", async () => {
    const env = await trust();
    const approved = approval(env);
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_ED_KEY: "AA" }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_ED_KEY: `${env.WINCMD_PQ_LIC_ED_KEY}=` }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_EPOCH: "0" }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_EPOCH: "9007199254740992" }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_FREE_ED_KEY: env.WINCMD_PQ_LIC_ED_KEY }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_MLDSA_ID: env.WINCMD_PQ_LIC_ED_ID }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_ED_ID: "ed:key" }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust(env, new Set([env.WINCMD_PQ_LIC_ED_KEY]), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_OLD_ED_KEY: env.WINCMD_PQ_LIC_ED_KEY }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust(env, new Set(), { ...approved, status: "unconfigured" })).toThrow();
    const mismatched = approval(env); mismatched.profiles.LIC.ed_sha256 = "0".repeat(64);
    expect(() => validatePqReleaseTrust(env, new Set(), mismatched)).toThrow();
    expect(normalizedPqReleaseEnvironment({ ...env, WINCMD_PQ_LIC_OLD_ED_KEY: "" })).not.toHaveProperty("WINCMD_PQ_LIC_OLD_ED_KEY");
  });
  test("a release without configured trust fails before the build starts", () => {
    const env = { ...process.env };
    for (const key of Object.keys(env)) if (key.startsWith("WINCMD_PQ_")) delete env[key];
    const result = Bun.spawnSync([process.execPath, "run", "tools/build-tauri-release.ts"], {
      cwd: process.cwd(), env, stdout: "pipe", stderr: "pipe",
    });
    expect(result.exitCode).not.toBe(0);
    expect(result.stderr.toString()).toMatch(/Release trust approval is not configured|Invalid or missing WINCMD_PQ_LIC_ED_KEY/);
    const wrapper = readFileSync("tools/build-tauri-release.ts", "utf8");
    expect(wrapper.indexOf("validateReleaseTrust(root, process.env)")).toBeLessThan(wrapper.indexOf("assertSharedAssets(root)"));
    expect(wrapper).toContain("serviceBuildCommand()");
    expect(wrapper).toContain("desktopBuildCommand(generatedConfigPath)");
  });
  test("Pro release build refuses missing approval before invoking Cargo", () => {
    const env = { ...process.env };
    for (const key of Object.keys(env)) if (key.startsWith("WINCMD_PQ_")) delete env[key];
    const fakePro = mkdtempSync(join(tmpdir(), "wincmd-pq-test-"));
    try {
      writeFileSync(join(fakePro, "Cargo.toml"), "");
      env.WINCOMMANDER_PRO_WORKSPACE = fakePro;
      const result = Bun.spawnSync([process.execPath, "run", "tools/build-pro.ts", "--release"], {
        cwd: process.cwd(), env, stdout: "pipe", stderr: "pipe",
      });
      expect(result.exitCode).not.toBe(0);
      expect(result.stderr.toString()).toMatch(/Release trust approval is not configured|Invalid or missing WINCMD_PQ_LIC_ED_KEY/);
    } finally {
      rmSync(fakePro, { recursive: true, force: true });
    }
  });
  test("requires every approved historical key and rejects an incomplete rotation", async () => {
    const current = await trust();
    const previous = await trust();
    current.WINCMD_PQ_LIC_EPOCH = "2";
    const env = { ...current, WINCMD_PQ_LIC_OLD_ED_KEY: previous.WINCMD_PQ_LIC_ED_KEY,
      WINCMD_PQ_LIC_OLD_MLDSA_KEY: previous.WINCMD_PQ_LIC_MLDSA_KEY,
      WINCMD_PQ_LIC_OLD_ED_ID: "old-ed", WINCMD_PQ_LIC_OLD_MLDSA_ID: "old-pq", WINCMD_PQ_LIC_OLD_MAX_EPOCH: "1" };
    const approved = approval(current);
    const old = { ...approval(previous).profiles.LIC, ed_id: "old-ed", mldsa_id: "old-pq", epoch: 1 };
    approved.profiles.LIC = { ...approved.profiles.LIC, old };
    expect(() => validatePqReleaseTrust(env, new Set(), approved)).not.toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_OLD_ED_KEY: "" }, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust(current, new Set(), approved)).toThrow();
    expect(() => validatePqReleaseTrust({ ...env, WINCMD_PQ_LIC_OLD_MAX_EPOCH: "2" }, new Set(), approved)).toThrow();
  });
});
