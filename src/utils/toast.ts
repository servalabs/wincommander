// src/utils/toast.ts
//
// Notification helpers for WinCommander. All four convenience helpers route
// to the title-bar bell (notificationStore) — no bottom-corner toasts.
// showToast() still exists for internal/legacy use with Sonner (e.g. if a
// caller explicitly wants a transient corner toast). dismissToast/dismissAll
// remain for any pending Sonner dismissals.

import { toast } from "sonner";
import { pushNotification, getNotificationsHidden, getPopupAlertsEnabled, getPopupAlertsSuppressed, type NotifKind } from "../lib/notificationStore";

// Optional per-call routing override. Operational warn/danger results (mount,
// paste, toggle, update) pass { kind: "notification" } so they land in the
// Notifications tab instead of the default System Alerts.
interface NotifyOpts { kind?: NotifKind }

type ToastIntent = "none" | "primary" | "success" | "warning" | "danger";

interface ToastOptions {
    message: string;
    intent?: ToastIntent;
    timeout?: number;
    /** If true (default for errors), shows a COPY action that copies the message text. */
    copyable?: boolean;
    action?: { text: string; onClick: () => void };
    key?: string;
}

function copyText(message: string) {
    try {
        navigator.clipboard.writeText(message).catch(() => {
            const el = document.createElement("textarea");
            el.value = message;
            document.body.appendChild(el);
            el.select();
            document.execCommand("copy");
            document.body.removeChild(el);
        });
    } catch {
        // Silently ignore clipboard errors
    }
}

export async function showToast(opts: ToastOptions): Promise<string> {
    const action = opts.action
        ? { label: opts.action.text, onClick: opts.action.onClick }
        : opts.copyable
            ? { label: "Copy", onClick: () => copyText(opts.message) }
            : undefined;

    // Dedup by explicit key, else by message (matches the previous behavior).
    const data = { duration: opts.timeout ?? 4000, action, id: opts.key ?? opts.message };

    let id: string | number;
    switch (opts.intent) {
        case "success": id = toast.success(opts.message, data); break;
        case "danger": id = toast.error(opts.message, data); break;
        case "warning": id = toast.warning(opts.message, data); break;
        case "primary": id = toast.info(opts.message, data); break;
        default: id = toast(opts.message, data); break;
    }
    return String(id);
}

// ── Convenience helpers ───────────────────────────────────────────
//
// All four always write to the bell AND fire a Sonner corner toast,
// unless notifications are hidden (concealment mode / universal toggle).

async function maybeToast(intent: ToastIntent, message: string, timeout: number): Promise<void> {
    if (getNotificationsHidden()) return;
    if (!getPopupAlertsEnabled() || getPopupAlertsSuppressed()) return;
    try { await showToast({ message, intent, timeout }); } catch { /* best-effort */ }
}

export async function showSuccess(message: string, timeout = 4000, opts?: NotifyOpts): Promise<string> {
    const id = pushNotification("info", message, undefined, opts?.kind);
    void maybeToast("success", message, timeout);
    return id;
}

export async function showError(message: string, timeout = 6000, opts?: NotifyOpts): Promise<string> {
    const id = pushNotification("danger", message, undefined, opts?.kind);
    void maybeToast("danger", message, timeout);
    return id;
}

export async function showWarning(message: string, timeout = 5000, opts?: NotifyOpts): Promise<string> {
    const id = pushNotification("warn", message, undefined, opts?.kind);
    void maybeToast("warning", message, timeout);
    return id;
}

export async function showInfo(message: string, timeout = 4000, opts?: NotifyOpts): Promise<string> {
    const id = pushNotification("info", message, undefined, opts?.kind);
    void maybeToast("primary", message, timeout);
    return id;
}

// ── Dismiss ───────────────────────────────────────────────────────

export async function dismissToast(key: string): Promise<void> {
    toast.dismiss(key);
}

export async function dismissAll(): Promise<void> {
    toast.dismiss();
}
