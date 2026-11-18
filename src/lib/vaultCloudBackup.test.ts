// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from "bun:test";
import { acceptsCloudBackupReceipt, cloudBackupError, cloudBackupStateLabel, validCloudBackupPath, validateCloudBackupDraft } from "./vaultCloudBackup";
import type { VaultCloudBackupConfiguration, VaultCloudBackupResult } from "@/types/generated/ipc";

const configuration: VaultCloudBackupConfiguration = { whole_vault: false, source_relative_paths: ["Documents", "Photos"], remote_folder_name: "Personal backup", automatic: true };
describe("Vault cloud backup setup", () => {
  test("rejects escaping, state, system and Windows alias paths", () => {
    for (const path of ["", "../Documents", "C:\\Documents", "\\Documents", ".wincommander\\cloud-backup", "$RECYCLE.BIN", "System Volume Information", "Documents\\..\\Secrets", "Documents.\\Secrets", "Documents ", "Documents//Photos"]) {
      expect(validCloudBackupPath(path)).toBe(false);
    }
    expect(validCloudBackupPath("Documents\\Reports")).toBe(true);
  });
  test("requires distinct bounded sources and a single safe remote folder name", () => {
    expect(validateCloudBackupDraft(configuration)).toBeNull();
    for (const paths of [[], ["Documents", "documents\\Reports"], ["Photos", "PHOTOS"], ["Photos/Raw", "Photos"], Array.from({ length: 33 }, (_, index) => `Folder${index}`)]) {
      expect(validateCloudBackupDraft({ ...configuration, source_relative_paths: paths })).not.toBeNull();
    }
    expect(validateCloudBackupDraft({ ...configuration, whole_vault: true, source_relative_paths: [] })).toBeNull();
    expect(validateCloudBackupDraft({ ...configuration, whole_vault: true })).not.toBeNull();
    expect(validateCloudBackupDraft({ ...configuration, remote_folder_name: "../Other" })).not.toBeNull();
  });
  test("requires the same mounted identity for subsequent operations", () => {
    expect(acceptsCloudBackupReceipt(null, "a".repeat(64))).toBe(true);
    expect(acceptsCloudBackupReceipt("a".repeat(64), "b".repeat(64))).toBe(false);
    expect(acceptsCloudBackupReceipt(null, null)).toBe(false);
    expect(acceptsCloudBackupReceipt(null, "not a receipt")).toBe(false);
  });
  test("does not echo backend secrets or pretend an unknown response succeeded", () => {
    expect(cloudBackupError("error: token=secret path=X:\\private")).not.toContain("secret");
    expect(cloudBackupError("cloud_backup_not_signed_in token=secret")).toContain("Reconnect Proton Drive");
    expect(cloudBackupError("cloud_backup_not_permitted")).toContain("Make sure it is still mounted");
    expect(cloudBackupError("cloud_backup_presentation_unavailable")).toBe("The mounted Vault is not accessible in this Windows session. Reopen it and try again.");
    const base: VaultCloudBackupResult = { mount_receipt: null, engine_ready: true, connected: true, configured: true,
      automatic: true, watching: true, running: false, can_configure: true, configuration, last_run: null,
      last_backup: null, last_verification: null, last_restore: null, reason_code: null };
    expect(cloudBackupStateLabel(base)).toContain("Watching for changes");
    expect(cloudBackupStateLabel({ ...base, running: true })).toBe("Backup operation running…");
    expect(cloudBackupStateLabel({ ...base, watching: false })).toBe("Automatic backup is paused");
    expect(cloudBackupStateLabel({ ...base, connected: false, watching: false, reason_code: "cloud_backup_sign_in_pending" })).toContain("Complete Proton sign-in in your browser");
  });
});
