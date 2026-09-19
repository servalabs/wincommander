// Typed client for the local Windows Print Service audit commands.
// Detailed print metadata remains on this device; it is not part of the
// aggregate Argus/Fleet signal client.

import { invoke } from "@tauri-apps/api/core";

export interface PrintAuditEntry {
  timeCreated: string;
  document?: string | null;
  pages: number;
  printer?: string | null;
  user?: string | null;
  jobStatus?: string | null;
}

export interface PrintAuditStatus {
  channelEnabled: boolean;
  channelPresent: boolean;
}

export const printAudit = {
  status: () => invoke<PrintAuditStatus>("get_print_audit_status"),
  recent: (limit = 50) => invoke<PrintAuditEntry[]>("get_print_audit_log", { limit }),
  setEnabled: (enabled: boolean) => invoke("set_print_audit_enabled", { enabled }),
};
