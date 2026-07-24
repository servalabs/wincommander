// src/components/shared/ConfirmIrreversibleDialog.tsx
//
// ═══════════════════════════════════════════════════════════════════════
// IRREVERSIBLE-ACTION CONFIRMATION DIALOG
// ═══════════════════════════════════════════════════════════════════════
//
// The single confirmation surface for any feature flagged
// `irreversible: true` in the toggle / feature registry. Replaces the
// per-feature typed-phrase patterns ("type DESTROY to continue") with a
// uniform pattern: a description of what's about to happen, a Cancel
// button, and a labeled action button that's disabled until a countdown
// elapses.
//
// Why countdown-instead-of-typed-phrase: matches Obsidian / VS Code /
// 1Password — clear button label + a brief enforced pause is enough
// friction for accidental clicks while not insulting users who genuinely
// want to proceed. See ref/architecture.md (Open-Core Architecture — Risk model + Backend).
//
// Usage:
//   const [open, setOpen] = useState(false);
//   <ConfirmIrreversibleDialog
//     isOpen={open}
//     onClose={() => setOpen(false)}
//     onConfirm={async () => { await eraseShadowCopies(); }}
//     title="Erase Shadow Copies"
//     consequences={[
//       "All Volume Shadow Copies are permanently deleted",
//       "System Restore points become unavailable",
//       "Cannot be reversed without a backup",
//     ]}
//     actionLabel="Erase Shadow Copies"
//   />
//
// Audit log: TODO (Phase 5 — write to %APPDATA%\WinCommander\audit.log
// on confirm, signed append-only). For now the dialog logs to the
// console; the Tauri-side audit module will land alongside the CI
// invariants work.

import { Button, Dialog, DialogBody, DialogFooter, Icon } from "@/components/ui/bp";
import { useEffect, useRef, useState } from "react";

export interface ConfirmIrreversibleDialogProps {
  /** Whether the dialog is visible. */
  isOpen: boolean;

  /** Called when the user dismisses (Cancel / X / backdrop click). */
  onClose: () => void;

  /** Called when the user confirms. May be async — the dialog stays open
   *  with the action button in a loading state until the promise resolves,
   *  then closes automatically. Throw to keep the dialog open with the
   *  error surfaced via toast (caller's responsibility). */
  onConfirm: () => void | Promise<void>;

  /** Dialog title — short, action-name style ("Erase Shadow Copies"). */
  title: string;

  /** Bullet list of consequences shown above the buttons. Each string is
   *  rendered as a list item; copy should be plain English. */
  consequences: string[];

  /** Label of the destructive action button. Should match the panel CTA
   *  ("Erase", "Self Destruct", "Shred Volume", etc.). */
  actionLabel: string;

  /** Countdown duration in seconds before the action button enables.
   *  Default 3 (matches the standard pattern). Set to 0 for no countdown
   *  (used in tests). */
  countdownSeconds?: number;

  /** Optional additional intro line shown above the consequence list. */
  intro?: string;
}

export default function ConfirmIrreversibleDialog({
  isOpen,
  onClose,
  onConfirm,
  title,
  consequences,
  actionLabel,
  countdownSeconds = 3,
  intro,
}: ConfirmIrreversibleDialogProps) {
  const [remaining, setRemaining] = useState(countdownSeconds);
  const [busy, setBusy] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Reset + restart the countdown each time the dialog opens. We don't
  // want a stale "0" from a previous mount letting the user click immediately.
  useEffect(() => {
    if (!isOpen) return;
    setRemaining(countdownSeconds);
    setBusy(false);
    if (countdownSeconds <= 0) return;
    intervalRef.current = setInterval(() => {
      setRemaining((r) => Math.max(0, r - 1));
    }, 1000);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
      intervalRef.current = null;
    };
  }, [isOpen, countdownSeconds]);

  const canConfirm = !busy && remaining <= 0;

  const handleConfirm = async () => {
    if (!canConfirm) return;
    setBusy(true);
    try {
      // TODO (Phase 5): write an audit-log entry here, before invoking
      // the action — so even if the action crashes the app, the intent
      // is recorded. For now, console-log so the action shows up in dev.
      console.info(
        `[audit] irreversible action confirmed: title="${title}" action="${actionLabel}"`,
      );
      await onConfirm();
      onClose();
    } catch {
      // Surface failure handling lives at the call site (toasts, etc.)
      // We don't auto-close on error so the user can retry without
      // re-opening.
      setBusy(false);
    }
  };

  return (
    <Dialog
      isOpen={isOpen}
      onClose={busy ? () => {} : onClose}
      title={title}
      icon="warning-sign"
      style={{ width: 480 }}
      canEscapeKeyClose={!busy}
      canOutsideClickClose={!busy}
    >
      <DialogBody>
        {intro && (
          <p style={{ marginBottom: 12, color: "var(--color-text-secondary)" }}>
            {intro}
          </p>
        )}

        <p style={{ marginBottom: 12, fontSize: 12, color: "var(--color-text-secondary)" }}>
          This action <strong style={{ color: "var(--color-danger)" }}>cannot be undone.</strong>{" "}
          The following will happen:
        </p>

        <ul
          style={{
            margin: 0,
            paddingLeft: 20,
            fontSize: 12,
            color: "var(--color-text-primary)",
            lineHeight: 1.7,
          }}
        >
          {consequences.map((c) => (
            <li key={c}>{c}</li>
          ))}
        </ul>
      </DialogBody>

      <DialogFooter>
        <Button minimal className="wc-btn-ghost" onClick={onClose} disabled={busy}>
          CANCEL
        </Button>
        <Button
          className="wc-btn-danger"
          onClick={handleConfirm}
          disabled={!canConfirm}
          loading={busy}
          icon={<Icon icon="ban-circle" size={11} />}
        >
          {remaining > 0 ? `${actionLabel.toUpperCase()} (${remaining}s)` : actionLabel.toUpperCase()}
        </Button>
      </DialogFooter>
    </Dialog>
  );
}
