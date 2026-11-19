// SPDX-License-Identifier: AGPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from "react";
import useBackend from "./useBackend";
import type { VaultCloudBackupAction, VaultCloudBackupConfiguration, VaultCloudBackupRequest, VaultCloudBackupResult } from "@/types/generated/ipc";
import { acceptsCloudBackupReceipt, cloudBackupError } from "@/lib/vaultCloudBackup";

export default function useVaultCloudBackup(isOpen: boolean, personal: boolean, internalDrive: number) {
  const backend = useBackend();
  const latest = useRef(backend); latest.current = backend;
  const generation = useRef(0);
  const inFlight = useRef(false);
  const activeOperation = useRef<VaultCloudBackupAction | null>(null);
  const receipt = useRef<string | null>(null);
  const invalidated = useRef(false);
  const [result, setResult] = useState<VaultCloudBackupResult | null>(null);
  const latestResult = useRef(result); latestResult.current = result;
  const lastPollAt = useRef(0);
  const [busy, setBusy] = useState(false);
  const [action, setAction] = useState<VaultCloudBackupAction | null>(null);
  const [error, setError] = useState("");
  const invalidate = useCallback(() => { generation.current++; inFlight.current = false; }, []);
  const observe = useCallback((incoming: VaultCloudBackupResult) => {
    if (!acceptsCloudBackupReceipt(receipt.current, incoming.mount_receipt)) {
      invalidated.current = true; setResult(null); throw new Error("cloud_backup_mount_changed");
    }
    receipt.current = incoming.mount_receipt;
    setResult(incoming);
  }, []);
  const perform = useCallback(async (operation: VaultCloudBackupAction, configuration?: VaultCloudBackupConfiguration, restoreRelativePath?: string) => {
    if (inFlight.current) {
      if (activeOperation.current !== "status" || operation === "status") return false;
    }
    if (invalidated.current) { setError(cloudBackupError("cloud_backup_mount_changed")); return false; }
    if (operation !== "status" && !receipt.current) {
      setError(cloudBackupError("cloud_backup_mount_changed")); return false;
    }
    // Ignore a superseded status response without cancelling the user's action.
    if (inFlight.current) generation.current++;
    const epoch = generation.current;
    inFlight.current = true; activeOperation.current = operation;
    if (operation === "status") lastPollAt.current = Date.now();
    setBusy(operation !== "status" || !latestResult.current); setAction(operation); setError("");
    const request: VaultCloudBackupRequest = {
      personal, internal_drive: internalDrive, action: operation, expected_mount_receipt: receipt.current,
      configuration: configuration ?? null, restore_relative_path: restoreRelativePath ?? null,
    };
    try {
      let response = await (["prepare", "connect", "configure"].includes(operation)
        ? latest.current.setupVaultCloudBackup(request) : latest.current.manageVaultCloudBackup(request));
      if (epoch !== generation.current) return false;
      const operationReason = response.reason_code;
      if (operation !== "status") {
        // Mutations return state too. Only Stop returns an acknowledgement.
        if (operation !== "stop") observe(response);
        activeOperation.current = "status"; setBusy(false); setAction("status");
        response = await latest.current.manageVaultCloudBackup({ ...request, action: "status", configuration: null, restore_relative_path: null });
        if (epoch !== generation.current) return false;
      }
      observe(response);
      const reason = operationReason ?? response.reason_code;
      if (reason && reason !== "cloud_backup_sign_in_pending") setError(cloudBackupError(reason));
      return !reason;
    } catch (cause) {
      if (epoch === generation.current) {
        // A timeout may leave an operation running. Require a fresh observation.
        setResult(null); setError(cloudBackupError(cause));
      }
      return false;
    } finally {
      if (epoch === generation.current) { inFlight.current = false; activeOperation.current = null; setBusy(false); setAction(null); }
    }
  }, [internalDrive, personal, observe]);
  useEffect(() => {
    generation.current++; receipt.current = null; invalidated.current = false; inFlight.current = false;
    // The new mount has no usable observation yet. Reset the synchronous ref
    // too, so its first pending status shows loading rather than inheriting
    // the previous Vault's nonblocking-refresh state.
    latestResult.current = null;
    setResult(null); setBusy(false); setAction(null); setError("");
    if (!isOpen) return;
    void perform("status");
    const timer = window.setInterval(() => {
      const current = latestResult.current;
      // Do not repeatedly interrupt setup while waiting for the user to connect.
      if (!inFlight.current && current && (current.connected || current.reason_code === "cloud_backup_sign_in_pending")
        && (current.running || current.reason_code === "cloud_backup_sign_in_pending" || Date.now() - lastPollAt.current >= 30000)) void perform("status");
    }, 3000);
    return () => { invalidate(); window.clearInterval(timer); };
  }, [isOpen, personal, internalDrive, perform, invalidate]);
  return { result, busy, action, error, perform };
}
