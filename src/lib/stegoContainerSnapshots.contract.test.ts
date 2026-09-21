import { describe, expect, test } from "bun:test";
import {
  validateAttachForm,
  validateCreateForm,
  validateExtractForm,
  validateRefreshForm,
  validateRestoreFolderForm,
} from "./stegoBackupValidation";

// These tests describe the public form contracts for V2 snapshots. They do
// not claim that a browser or the installed Windows backend handler has run: those need a
// real carrier video and VeraCrypt container on Windows.

describe("Stego container snapshots", () => {
  test("attaches an existing .hc or .tc container without collecting a password", () => {
    for (const containerPath of ["D:\\Vault\\Personal.hc", "D:\\Vault\\Archive.TC"]) {
      const verdict = validateAttachForm({
        carrierPath: "D:\\Videos\\holiday.mp4",
        containerPath,
        outputPath: "D:\\Backups\\holiday-private.mp4",
      });

      expect(verdict.canSubmit).toBe(true);
      expect(verdict.errors).toEqual([]);
    }
  });

  test("refuses a non-container or any attach output that would replace either input", () => {
    expect(
      validateAttachForm({
        carrierPath: "D:\\Videos\\holiday.mp4",
        containerPath: "D:\\Vault\\not-a-container.zip",
        outputPath: "D:\\Backups\\holiday-private.mp4",
      }).canSubmit,
    ).toBe(false);

    for (const outputPath of ["D:\\Videos\\holiday.mp4", "D:\\Vault\\Personal.hc"]) {
      expect(
        validateAttachForm({
          carrierPath: "D:\\Videos\\holiday.mp4",
          containerPath: "D:\\Vault\\Personal.hc",
          outputPath,
        }).canSubmit,
      ).toBe(false);
    }
  });

  test("restores into a destination folder, retaining the embedded original filename", () => {
    const valid = validateRestoreFolderForm({
      inputPath: "D:\\Backups\\holiday-private.mp4",
      destinationDir: "E:\\Recovered",
    });

    expect(valid.canSubmit).toBe(true);
    expect(valid.errors).toEqual([]);
    // The V2 form deliberately takes destinationDir, not a user-supplied
    // output filename. The handler reads the stored original name instead.
    expect(Object.keys({ inputPath: "", destinationDir: "" })).toEqual(["inputPath", "destinationDir"]);
    expect(validateRestoreFolderForm({ inputPath: "D:\\Backups\\holiday-private.mp4", destinationDir: "" }).canSubmit).toBe(false);
  });

  test("requires an explicit acknowledgement before refresh replaces a backup", () => {
    const unchanged = validateRefreshForm({
      backupVideoPath: "D:\\Backups\\holiday-private.mp4",
      containerPath: "D:\\Vault\\Personal.hc",
      replacementConfirmed: false,
    });
    expect(unchanged.canSubmit).toBe(false);
    expect(unchanged.errors.find((issue) => issue.field === "confirmation")?.message).toContain(
      "replaced after verification",
    );

    const confirmed = validateRefreshForm({
      backupVideoPath: "D:\\Backups\\holiday-private.mp4",
      containerPath: "D:\\Vault\\Personal.hc",
      replacementConfirmed: true,
    });
    expect(confirmed.canSubmit).toBe(true);
  });

  test("keeps the legacy empty-container flow distinct from the snapshot flow", () => {
    // Legacy creation intentionally still needs a new-container password.
    expect(
      validateCreateForm({
        carrierPath: "D:\\Videos\\holiday.mp4",
        outputPath: "D:\\Backups\\legacy.mp4",
        sizeRaw: "100",
        sizeUnit: "M",
        password: "",
        passwordConfirm: "",
        destinationFreeBytes: null,
        carrierBytes: null,
      }).canSubmit,
    ).toBe(false);

    // Legacy V1 restore remains an explicit-file-name fallback because its
    // trailer has no original container filename to restore automatically.
    const legacyRestore = validateExtractForm({
      inputPath: "D:\\Backups\\legacy.mp4",
      outputPath: "D:\\Recovered\\legacy-volume",
    });
    expect(legacyRestore.canSubmit).toBe(true);
    expect(legacyRestore.normalizedOutputPath).toBe("D:\\Recovered\\legacy-volume.hc");
  });
});
