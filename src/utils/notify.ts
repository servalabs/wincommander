// src/utils/notify.ts
//
// Unified notification entry point.
// One import, one call, the right render target:
//   - success / info / warning / danger  → title-bar bell (notificationStore)
//   - operation (with steps)             → OperationOverlay multi-step task list
//     (which also drives the TaskStatusBar progress surface)
//
// Existing showSuccess/showError/runOperation remain valid; notify() is the
// single front door new code should reach for.

import { runOperation, type OperationStep, type RunOperationOptions } from "../context/OperationContext";
import { pushNotification, type NotifKind } from "../lib/notificationStore";

type NotifyType = "success" | "info" | "warning" | "danger" | "operation";

export interface NotifyArgs {
  type?: NotifyType;
  message: string;
  /** For type:"operation" — the multi-step task list rendered in the overlay. */
  steps?: OperationStep[];
  /** Toast auto-dismiss (ms). */
  timeout?: number;
  /** Bell-tab override for operational warn/danger results (else severity-derived). */
  kind?: NotifKind;
  /** Operation-overlay options (mode/accent/doneTitle/…). */
  operation?: RunOperationOptions;
}

export function notify(args: NotifyArgs) {
  const { type = "info", message, steps, operation, kind } = args;
  if (type === "operation") {
    return runOperation(message, steps ?? [], operation);
  }
  const severity = type === "danger" ? "danger" : type === "warning" ? "warn" : "info";
  return pushNotification(severity, message, undefined, kind);
}

export default notify;
