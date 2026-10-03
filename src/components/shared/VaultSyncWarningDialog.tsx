// SPDX-License-Identifier: AGPL-3.0-or-later
import { useEffect, useState } from "react";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Button } from "../ui/button";
import { VAULT_SYNC_WARNING_EVENT, vaultSyncNotice, vaultSyncWarningMessage, type VaultSyncNotice } from "../../lib/vaultSyncWarning";

export default function VaultSyncWarningDialog() {
  const [notices, setNotices] = useState<VaultSyncNotice[]>([]);
  useEffect(() => {
    const receive = (event: Event) => {
      const detail: unknown = (event as CustomEvent<unknown>).detail;
      if (!detail || typeof detail !== "object") return;
      const value = detail as Partial<VaultSyncNotice>;
      const notice = vaultSyncNotice(value.drive, value.warning);
      if (notice) setNotices(current => current.some(item => item.drive === notice.drive && item.warning === notice.warning)
        ? current : [...current, notice]);
    };
    window.addEventListener(VAULT_SYNC_WARNING_EVENT, receive);
    return () => window.removeEventListener(VAULT_SYNC_WARNING_EVENT, receive);
  }, []);
  const notice = notices[0];
  const close = () => setNotices(current => current.slice(1));
  return (
    <Dialog open={Boolean(notice)} onOpenChange={open => { if (!open) close(); }}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Container mounted — Syncthing needs attention</DialogTitle>
          <DialogDescription className="break-words whitespace-normal">
            {notice ? vaultSyncWarningMessage(notice) : ""}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter><Button onClick={close}>Close</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
