// SPDX-License-Identifier: AGPL-3.0-or-later
import { useEffect, useRef, useState } from "react";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Button } from "../ui/button";
import { VAULT_SYNC_WARNING_EVENT, vaultSyncNotice, vaultSyncWarningMessage, type VaultSyncNotice } from "../../lib/vaultSyncWarning";
import { vaultSyncRecoveryChoice } from "../../lib/vaultSyncRecovery";
import VaultSyncRecoveryPanel from "./VaultSyncRecoveryPanel";
import useBackend from "../../hooks/useBackend";
import { inspectPostMountSync } from "../../lib/vaultSyncPostMount";
import { personalVaultSyncError } from "../../lib/personalVaultSyncFeedback";
import { pushNotification, getNotificationsHidden, getPopupAlertsEnabled, getPopupAlertsSuppressed } from "../../lib/notificationStore";
import { showInfo, showToast } from "../../utils/toast";
import { canOpenQuickMountDrive, type QuickMountExplorerTarget } from "../../lib/quickMountObservation";

type PreparedNotice = VaultSyncNotice & { message?: string; target?: QuickMountExplorerTarget };

export default function VaultSyncWarningDialog() {
  const backend = useBackend();
  const latest = useRef(backend); latest.current = backend;
  const checking = useRef(new Set<string>());
  const reviewing = useRef(new Set<string>());
  const [notices, setNotices] = useState<PreparedNotice[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    const present = (notice: PreparedNotice) => setNotices(current => current.some(item => item.drive === notice.drive && item.warning === notice.warning)
      ? current : [...current, notice]);
    const review = async (notice: PreparedNotice) => {
      if (!active || reviewing.current.has(notice.drive)) return;
      reviewing.current.add(notice.drive);
      try {
        if (notice.target) {
          const state = await latest.current.getEncryptedVolumeStatus();
          if (!active) return;
          if (!state.success || !canOpenQuickMountDrive(state.data ?? null, null, notice.drive, notice.target.containerPath, notice.target)) {
            void showInfo(`${notice.drive} The mounted Vault has changed. Refresh Secure Storage and open the intended Vault’s sync settings.`, undefined, { kind: "notification" });
            return;
          }
        }
        if (active) present(notice);
      } catch {
        if (active) void showInfo(`${notice.drive} Its current mount could not be confirmed. Refresh Secure Storage before reviewing sync recovery.`, undefined, { kind: "notification" });
      } finally { reviewing.current.delete(notice.drive); }
    };
    const notify = (notice: PreparedNotice) => {
      if (!active) return;
      const message = notice.message ?? vaultSyncWarningMessage(notice);
      pushNotification("warn", `${message} Open this Vault’s sync settings to review.`, undefined, "notification");
      if (!getNotificationsHidden() && getPopupAlertsEnabled() && !getPopupAlertsSuppressed()) {
        void showToast({ message, intent: "warning", timeout: 10000, key: `vault-sync-${notice.drive}`,
          action: { text: "Review", onClick: () => { void review(notice); } } });
      }
    };
    const check = async (notice: VaultSyncNotice) => {
      if (checking.current.has(notice.drive)) return;
      checking.current.add(notice.drive);
      try {
        const result = await inspectPostMountSync(notice, {
          readStatus: () => latest.current.getEncryptedVolumeStatus(),
          list: slot => latest.current.managePersonalVaultSync(slot, "list"),
          inspect: (slot, receipt) => latest.current.enablePersonalVaultSync(slot, "Sync", "inspect", undefined, receipt),
        });
        if (!active || result.kind === "stale") return;
        if (result.kind === "recovery") notify({ ...result.notice, target: result.target });
        else void showInfo(`${notice.drive} Sync check finished. No folder recovery decisions are needed.`, undefined, { kind: "notification" });
      } catch (failure) {
        notify({ ...notice, warning: "unavailable", message: `${notice.drive} The mount completed, but its sync check could not finish. ${personalVaultSyncError(failure, "status")}` });
      } finally { checking.current.delete(notice.drive); }
    };
    const receive = (event: Event) => {
      const detail: unknown = (event as CustomEvent<unknown>).detail;
      if (!detail || typeof detail !== "object") return;
      const value = detail as Partial<VaultSyncNotice>;
      const notice = vaultSyncNotice(value.drive, value.warning);
      if (notice && value.recovery) {
        const recovery = vaultSyncRecoveryChoice(value.recovery.internalDrive, value.recovery.roots);
        if (recovery) notice.recovery = recovery;
      }
      if (!notice) return;
      if (notice.recovery) present(notice);
      else if (notice.warning === "recovery_required") void check(notice);
      else notify(notice);
    };
    window.addEventListener(VAULT_SYNC_WARNING_EVENT, receive);
    return () => { active = false; window.removeEventListener(VAULT_SYNC_WARNING_EVENT, receive); };
  }, []);
  const notice = notices[0];
  const close = () => { if (!busy) setNotices(current => current.slice(1)); };
  return (
    <Dialog open={Boolean(notice)} onOpenChange={open => { if (!open) close(); }}>
      <DialogContent className="max-w-lg flex flex-col">
        <DialogHeader>
          <DialogTitle>Container mounted — Syncthing needs attention</DialogTitle>
          <DialogDescription className="break-words whitespace-normal">
            {notice ? notice.message ?? vaultSyncWarningMessage(notice) : ""}
          </DialogDescription>
        </DialogHeader>
        {notice?.warning === "recovery_required" && <div className="min-h-0 overflow-y-auto" style={{ maxHeight: "min(60vh, 32rem)" }}>
          <VaultSyncRecoveryPanel key={`${notice.drive}:${notice.recovery?.roots[0]?.token ?? "inspect"}`} notice={notice} onBusyChange={setBusy} />
        </div>}
        <DialogFooter><Button disabled={busy} onClick={close}>{notice?.warning === "recovery_required" ? "Done" : "Close"}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
