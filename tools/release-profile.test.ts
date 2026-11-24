// SPDX-License-Identifier: AGPL-3.0-or-later
// Explicit release-profile selection: hybrid (default, fully gated) versus classical.
import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  DESKTOP_PQ_FEATURES, SERVICE_PQ_FEATURES, appendFeatures, assertClassicalEnvironment, assertNoExperimentalFeatures,
  desktopBuildCommand, proBuildArgs, releaseBuildEnvironment, releaseProfile, serviceBuildCommand, withoutReleaseProfileFlag,
} from "./pq-release-profile";

const experimentalIn = (command: readonly string[]) =>
  command.flatMap((arg, i) => (command[i - 1] === "--features" ? arg.split(",") : [])).filter((f) => f.startsWith("experimental-"));
const pro = (extra: readonly string[], profile?: "classical" | "hybrid", recoveryLab = false) =>
  proBuildArgs(extra, { release: true, recoveryLab, manifestPath: "m", targetDir: "t", profile });

/** Run a release tool with a controlled environment; only used for runs that must fail before building. */
function runTool(script: string, args: string[], extra: Record<string, string>) {
  const env: Record<string, string | undefined> = { ...process.env };
  for (const key of Object.keys(env)) if (key.startsWith("WINCMD_PQ_") || key === "WINCMD_RELEASE_PROFILE") delete env[key];
  return Bun.spawnSync([process.execPath, "run", script, ...args], { cwd: process.cwd(), env: { ...env, ...extra }, stdout: "pipe", stderr: "pipe" });
}

describe("explicit release profile selection", () => {
  test("unset, empty and hybrid all mean today's hybrid behaviour", () => {
    expect(releaseProfile({})).toBe("hybrid");
    expect(releaseProfile({ WINCMD_RELEASE_PROFILE: "" })).toBe("hybrid");
    expect(releaseProfile({ WINCMD_RELEASE_PROFILE: "hybrid" })).toBe("hybrid");
    expect(releaseProfile({}, ["--release-profile=hybrid"])).toBe("hybrid");
  });
  test("classical is chosen only by the exact env value or CLI flag", () => {
    expect(releaseProfile({ WINCMD_RELEASE_PROFILE: "classical" })).toBe("classical");
    expect(releaseProfile({}, ["--release", "--release-profile=classical"])).toBe("classical");
    expect(releaseProfile({ WINCMD_RELEASE_PROFILE: "classical" }, ["--release-profile=classical"])).toBe("classical");
  });
  test("any other value, spelling, duplicate or conflict fails closed", () => {
    for (const value of ["Classical", "CLASSICAL", " classical", "classical ", "pq", "none", "0", "true", "classic"]) {
      expect(() => releaseProfile({ WINCMD_RELEASE_PROFILE: value }), value).toThrow(/Unknown release profile/);
      expect(() => releaseProfile({}, [`--release-profile=${value}`]), value).toThrow(/Unknown release profile/);
    }
    expect(() => releaseProfile({}, ["--release-profile="])).toThrow();
    expect(() => releaseProfile({}, ["--release-profile", "classical"])).toThrow();
    expect(() => releaseProfile({}, ["--release-profile=classical", "--release-profile=classical"])).toThrow();
    expect(() => releaseProfile({ WINCMD_RELEASE_PROFILE: "hybrid" }, ["--release-profile=classical"])).toThrow(/conflicts/);
    expect(() => releaseProfile({ WINCMD_RELEASE_PROFILE: "classical" }, ["--release-profile=hybrid"])).toThrow(/conflicts/);
  });
  test("the profile flag is never forwarded to Cargo", () => {
    expect(withoutReleaseProfileFlag(["--release", "--release-profile=classical", "-v"])).toEqual(["--release", "-v"]);
  });
});

describe("hybrid remains the default build profile", () => {
  test("default service and desktop commands carry exactly today's PQ features", () => {
    expect(serviceBuildCommand()).toEqual(serviceBuildCommand("hybrid"));
    expect(experimentalIn(serviceBuildCommand())).toEqual([...SERVICE_PQ_FEATURES]);
    expect(experimentalIn(desktopBuildCommand("c.json"))).toEqual([...DESKTOP_PQ_FEATURES]);
    expect(desktopBuildCommand("c.json").slice(-2)).toEqual(["--features", DESKTOP_PQ_FEATURES.join(",")]);
  });
  test("Pro release build keeps its historical arguments in the default profile", () => {
    expect(pro(["--release"])).toEqual(["build", "-p", "commander-pro", "--release", "--features", "experimental-pq-licence", "--manifest-path", "m", "--target-dir", "t"]);
    expect(proBuildArgs([], { release: false, recoveryLab: false, manifestPath: "m", targetDir: "t" })).not.toContain("--features");
  });
  test("default hybrid release still fails closed while trust approval is unconfigured", () => {
    for (const extra of [{}, { WINCMD_RELEASE_PROFILE: "hybrid" }, { WINCMD_RELEASE_PROFILE: "" }]) {
      const result = runTool("tools/build-tauri-release.ts", [], extra);
      expect(result.exitCode, JSON.stringify(extra)).not.toBe(0);
      expect(result.stderr.toString()).toMatch(/Release trust approval is not configured|Invalid or missing WINCMD_PQ_LIC_ED_KEY/);
    }
  });
});

describe("classical release profile", () => {
  test("service and desktop builds carry no features at all, so no experimental-* feature", () => {
    for (const command of [serviceBuildCommand("classical"), desktopBuildCommand("c.json", "classical")]) {
      expect(command).not.toContain("--features");
      expect(command.join(" ")).not.toContain("experimental-");
      expect(() => assertNoExperimentalFeatures(command, "test")).not.toThrow();
    }
    expect(serviceBuildCommand("classical").slice(0, 3)).toEqual(["cargo", "build", "--manifest-path"]);
    expect(desktopBuildCommand("c.json", "classical").slice(0, 4)).toEqual(["bun", "x", "--package", "@tauri-apps/cli@2.12.4"]);
  });
  test("Pro release build adds no experimental-pq-licence and keeps the non-experimental lab feature", () => {
    const args = pro(["--release"], "classical");
    expect(args).toEqual(["build", "-p", "commander-pro", "--release", "--manifest-path", "m", "--target-dir", "t"]);
    expect(pro(["--release"], "classical", true)).toContain("fleet-recovery-lab");
  });
  test("an experimental feature in a classical build throws, however it is spelled", () => {
    for (const args of [
      ["--features", "experimental-pq-licence"], ["--features=experimental-encrypted-search"], ["-F", "a,experimental-x"],
      ["-Fexperimental-x"], ["--features", "commander-pro/experimental-pq-licence"], ["--all-features"],
    ]) {
      expect(() => assertNoExperimentalFeatures(["cargo", "build", ...args], "test"), args.join(" ")).toThrow(/Classical release profile refuses/);
      expect(() => pro(["--release", ...args], "classical"), args.join(" ")).toThrow();
    }
    expect(() => assertNoExperimentalFeatures(["cargo", "--features", "fleet-recovery-lab,serde"], "test")).not.toThrow();
  });
  test("any WINCMD_PQ_* value throws, but unset GitHub variables (empty strings) are tolerated and stripped", () => {
    expect(() => assertClassicalEnvironment({ WINCMD_PQ_LIC_ED_KEY: "x" })).toThrow(/WINCMD_PQ_LIC_ED_KEY/);
    expect(() => releaseBuildEnvironment({ WINCMD_PQ_PRO_OLD_MAX_EPOCH: "1" }, "classical")).toThrow();
    expect(() => assertClassicalEnvironment({ WINCMD_PQ_LIC_ED_KEY: "" })).not.toThrow();
    const env = releaseBuildEnvironment({ WINCMD_PQ_LIC_ED_KEY: "", WINCMD_PQ_FREE_EPOCH: "", PATH: "p" }, "classical");
    expect(Object.keys(env).filter((k) => k.startsWith("WINCMD_PQ_"))).toEqual([]);
    expect(env.PATH).toBe("p");
    expect(env.WINCMD_RELEASE_PROFILE).toBe("classical");
  });
  test("the desktop tool fails before building on hybrid trust, unknown values, labs or conflicts", () => {
    const pq = runTool("tools/build-tauri-release.ts", [], { WINCMD_RELEASE_PROFILE: "classical", WINCMD_PQ_LIC_ED_KEY: "x" });
    expect(pq.exitCode).not.toBe(0);
    expect(pq.stderr.toString()).toMatch(/refuses hybrid trust configuration: WINCMD_PQ_LIC_ED_KEY/);
    const unknown = runTool("tools/build-tauri-release.ts", [], { WINCMD_RELEASE_PROFILE: "Classical" });
    expect(unknown.exitCode).not.toBe(0);
    expect(unknown.stderr.toString()).toMatch(/Unknown release profile/);
    const lab = runTool("tools/build-tauri-release.ts", ["--release-profile=classical", "--experimental-paired-search-lab"], {});
    expect(lab.exitCode).not.toBe(0);
    expect(lab.stderr.toString()).toMatch(/refuses the experimental paired search lab/);
    const conflict = runTool("tools/build-tauri-release.ts", ["--release-profile=classical"], { WINCMD_RELEASE_PROFILE: "hybrid" });
    expect(conflict.exitCode).not.toBe(0);
    expect(conflict.stderr.toString()).toMatch(/conflicts/);
  });
  test("build-pro rejects the same misconfigurations before invoking Cargo", () => {
    const fakePro = mkdtempSync(join(tmpdir(), "wincmd-profile-test-"));
    try {
      writeFileSync(join(fakePro, "Cargo.toml"), "");
      const base = { WINCOMMANDER_PRO_WORKSPACE: fakePro };
      const pq = runTool("tools/build-pro.ts", ["--release", "--release-profile=classical"], { ...base, WINCMD_PQ_PRO_ED_KEY: "x" });
      expect(pq.exitCode).not.toBe(0);
      expect(pq.stderr.toString()).toMatch(/refuses hybrid trust configuration/);
      const feature = runTool("tools/build-pro.ts", ["--release", "--features", "experimental-pq-licence"], { ...base, WINCMD_RELEASE_PROFILE: "classical" });
      expect(feature.exitCode).not.toBe(0);
      expect(feature.stderr.toString()).toMatch(/Classical release profile refuses the experimental-pq-licence/);
      const unknown = runTool("tools/build-pro.ts", ["--release"], { ...base, WINCMD_RELEASE_PROFILE: "pq" });
      expect(unknown.exitCode).not.toBe(0);
      expect(unknown.stderr.toString()).toMatch(/Unknown release profile/);
    } finally {
      rmSync(fakePro, { recursive: true, force: true });
    }
  });
});

describe("feature list handling is robust", () => {
  test("appendFeatures creates or extends --features without touching other arguments", () => {
    expect(appendFeatures(["tauri", "build"], "a")).toEqual(["tauri", "build", "--features", "a"]);
    expect(appendFeatures(["tauri", "--features", "a", "--x"], ["b", "c"])).toEqual(["tauri", "--features", "a,b,c", "--x"]);
    expect(appendFeatures(["tauri", "build"], [])).toEqual(["tauri", "build"]);
    expect(() => appendFeatures(["tauri", "--features"], "a")).toThrow(/Malformed/);
    expect(() => appendFeatures(["tauri", "--features", "--x"], "a")).toThrow(/Malformed/);
  });
  test("the release tools no longer locate --features with indexOf(...) + 1 or hard-wire the PQ feature", () => {
    const source = readFileSync("tools/build-tauri-release.ts", "utf8");
    expect(source).not.toMatch(/indexOf\(\s*"--features"\s*\)\s*\+\s*1/);
    expect(source).toContain("appendFeatures(");
    const proTool = readFileSync("tools/build-pro.ts", "utf8");
    expect(proTool).not.toContain("experimental-pq-licence");
    expect(proTool).toContain("proBuildArgs(");
  });
});
