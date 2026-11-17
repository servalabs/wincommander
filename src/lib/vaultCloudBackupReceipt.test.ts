import { describe, expect, test } from "bun:test";
import { cloudBackupRecordedRuns, cloudBackupRunSummary, cloudBackupStateLabel } from "./vaultCloudBackup";
import type { VaultCloudBackupReceipt, VaultCloudBackupResult } from "@/types/generated/ipc";

const run: VaultCloudBackupReceipt = {
  action: "start", remote_folder_name: "Recorded backup", restore_relative_path: null,
  confirmation: "remote_sample_matched", checked_files: 1, run_id: "run-1", outcome: "complete",
  started_at: "2026-11-17T12:00:00Z", finished_at: "2026-11-17T12:01:00Z",
  transferred_items: 2, transferred_bytes: "5", skipped_items: 0, failed_items: 0,
};
const result: VaultCloudBackupResult = {
  mount_receipt: null, engine_ready: true, connected: true, configured: false,
  automatic: false, watching: false, running: false, can_configure: true,
  configuration: null, last_run: null, last_backup: null, last_verification: null,
  last_restore: null, reason_code: null,
};
describe("backup confirmation evidence", () => {
  test("uses recorded destinations and distinguishes sampled cloud checks from restored checks", () => {
    expect(cloudBackupRunSummary(run)).toMatchObject({ tone: "success",
      destination: "Proton Drive / WinCommander Vault Backups / Recorded backup",
      detail: "1 sampled file matches your Vault." });
    const restored = { ...run, action: "restore" as const, confirmation: "restored_sample_matched" as const,
      restore_relative_path: "Restored check" };
    expect(cloudBackupRunSummary(restored)).toMatchObject({ tone: "success",
      title: "Files restored and checked", destination: "This Vault / Restored check" });
  });
  test("does not turn incomplete, legacy, empty or inconsistent evidence into success", () => {
    for (const value of [
      { ...run, action: null }, { ...run, finished_at: null },
      { ...run, finished_at: "not-a-date" }, { ...run, failed_items: 1 },
      { ...run, outcome: "unknown" as const }, { ...run, outcome: "partial" as const },
      { ...run, confirmation: null, checked_files: 0 }, { ...run, checked_files: 0 },
      { ...run, remote_folder_name: "../Other" },
      { ...run, action: "restore" as const, confirmation: "remote_sample_matched" as const },
    ]) expect(cloudBackupRunSummary(value).tone).not.toBe("success");
  });
  test("later automatic uploads do not overwrite the recorded restore", () => {
    const restored = { ...run, run_id: "restore-1", action: "restore" as const,
      confirmation: "restored_sample_matched" as const, restore_relative_path: "Restored check" };
    const recorded = cloudBackupRecordedRuns({ ...result, last_run: { ...run, run_id: "later-backup",
      started_at: "2026-11-17T12:02:00Z" }, last_restore: restored });
    expect(recorded.backup?.run_id).toBe("later-backup");
    expect(recorded.restore?.run_id).toBe("restore-1");
    const pending = { ...restored, run_id: "restore-2", outcome: "unknown" as const,
      confirmation: null, checked_files: 0, finished_at: null, started_at: "2026-11-17T12:03:00Z" };
    expect(cloudBackupRecordedRuns({ ...result, last_run: pending, last_restore: restored }).restore?.run_id).toBe("restore-2");
  });
  test("names the operation actually running", () => {
    expect(cloudBackupStateLabel({ ...result, running: true, last_run: { ...run, action: "restore" } })).toBe("Restoring files…");
    expect(cloudBackupStateLabel({ ...result, running: true, last_run: { ...run, action: "verify" } })).toBe("Checking files in Proton Drive…");
  });
});
