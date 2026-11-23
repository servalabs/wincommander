// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, test } from "bun:test";
import { cloudBackupDestinationPreview, cloudBackupDisplayPath, acceptsCloudBackupReceipt, cloudBackupError, cloudBackupStateLabel, validCloudBackupPath, validateCloudBackupDraft } from "./vaultCloudBackup";
import type { VaultCloudBackupConfiguration, VaultCloudBackupResult } from "@/types/generated/ipc";

const configuration: VaultCloudBackupConfiguration = { whole_vault: false, source_relative_paths: ["Documents", "Photos"], remote_folder_name: "Personal backup", automatic: true };
describe("Vault cloud backup setup", () => {
  test("previews new, renamed and legacy destinations without moving old backups", () => {
    expect(cloudBackupDestinationPreview("My chosen name", null, null)).toBe("My files / My chosen name");
    expect(cloudBackupDestinationPreview("Personal backup", configuration, "/my-files/WinCommander Vault Backups/Personal backup")).toBe("My files / WinCommander Vault Backups / Personal backup");
    expect(cloudBackupDestinationPreview("Changed", configuration, "/my-files/WinCommander Vault Backups/Personal backup")).toBe("My files / Changed");
    expect(cloudBackupDestinationPreview("Personal backup", configuration, null)).toBeNull();
    expect(cloudBackupDisplayPath("/other/private")).toBeNull();
  });
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
    expect(cloudBackupError("cloud_backup_start_timed_out")).toBe("The backup tool did not start in time. Try again.");
    expect(cloudBackupError("cloud_backup_start_exited")).toBe("The backup tool stopped before it could start. Try again.");
    expect(cloudBackupError("cloud_backup_engine_write_failed")).toContain("could not be saved into the Vault");
    expect(cloudBackupError("cloud_backup_engine_write_failed")).not.toContain("downloaded. Check the connection");
    expect(cloudBackupError("cloud_backup_engine_download_failed")).toContain("could not be downloaded");
    expect(cloudBackupError("cloud_backup_engine_write_failed:create:5")).toContain("Failed while creating the file in the Vault: Windows error 5 (access denied).");
    expect(cloudBackupError("cloud_backup_engine_write_failed:rename:32")).toContain("moving the file into place: Windows error 32 (another program is using the file)");
    expect(cloudBackupError("cloud_backup_engine_write_failed:write:9999")).toContain("Windows error 9999.");
    expect(cloudBackupError("cloud_backup_engine_write_failed:create:5 C:\private")).not.toContain("private");
    const base: VaultCloudBackupResult = { connection_check: "unknown", remote_check: "unknown", checked_at: null, remote_path: null, mount_receipt: null, engine_ready: true, connected: true, configured: true,
      automatic: true, watching: true, running: false, can_configure: true, configuration, last_run: null,
      last_backup: null, last_verification: null, last_restore: null, reason_code: null };
    expect(cloudBackupStateLabel(base)).toContain("Watching for changes");
    expect(cloudBackupStateLabel({ ...base, running: true })).toBe("Backup operation running…");
    expect(cloudBackupStateLabel({ ...base, watching: false })).toBe("Automatic backup is paused");
    expect(cloudBackupStateLabel({ ...base, connected: false, watching: false, reason_code: "cloud_backup_sign_in_pending" })).toContain("Complete Proton sign-in in your browser");
  });
});
