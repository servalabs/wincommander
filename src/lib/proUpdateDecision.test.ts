import { describe, expect, test } from "bun:test";
import { shouldAutomaticallyReplacePro } from "./proUpdateDecision";

describe("automatic Pro replacement", () => {
  const installed = { installed: true, local_version: "3.6.4", local_sha256: "local-paired-build" };

  test("never downgrades a newer paired build to the published Pro", () => {
    expect(shouldAutomaticallyReplacePro(installed, { version: "3.6.3", sha256: "published" })).toBe(false);
  });

  test("does not overwrite a same-version local build just because its hash differs", () => {
    for (const version of ["3.6.4", "3.6.4.0", "v3.6.4"]) {
      expect(shouldAutomaticallyReplacePro(installed, { version, sha256: "published" })).toBe(false);
    }
  });

  test("upgrades an installed older version, including legacy metadata without a hash", () => {
    const manifest = { version: "3.6.5", sha256: "published" };
    expect(shouldAutomaticallyReplacePro(installed, manifest)).toBe(true);
    expect(shouldAutomaticallyReplacePro({ ...installed, local_sha256: null }, manifest)).toBe(true);
    expect(shouldAutomaticallyReplacePro(installed, { ...manifest, version: "3.10.0" })).toBe(true);
  });

  test("does not reinstall identical content even when a manifest claims a newer version", () => {
    expect(shouldAutomaticallyReplacePro(installed, { version: "3.6.5", sha256: "LOCAL-PAIRED-BUILD" })).toBe(false);
  });

  test("leaves unknown or ambiguous versions for explicit manual recovery", () => {
    for (const version of [null, undefined, "", "unknown", "3.6.4-beta", "broken 3.6.4", "3.6.4.0.1"]) {
      expect(shouldAutomaticallyReplacePro({ ...installed, local_version: version }, { version: "3.6.5", sha256: "published" })).toBe(false);
      expect(shouldAutomaticallyReplacePro(installed, { version: version ?? "", sha256: "published" })).toBe(false);
    }
  });

  test("does not initiate first install or act on missing probes", () => {
    const manifest = { version: "3.6.5", sha256: "published" };
    expect(shouldAutomaticallyReplacePro({ ...installed, installed: false }, manifest)).toBe(false);
    expect(shouldAutomaticallyReplacePro(null, manifest)).toBe(false);
    expect(shouldAutomaticallyReplacePro(installed, null)).toBe(false);
    expect(shouldAutomaticallyReplacePro(installed, { ...manifest, sha256: "" })).toBe(false);
  });
});
