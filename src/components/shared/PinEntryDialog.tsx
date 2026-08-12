// src/components/shared/PinEntryDialog.tsx
//
// Generic PIN-collection dialog for toggles flagged `requiresPinOnEnable`
// in the toggle registry (see src/types/toggles.ts). Renders on top of the
// existing shadcn Dialog primitives — no new design system. The toggle
// itself stays a fully-controlled switch (state = appSettings), so
// cancelling here just closes the dialog; nothing was ever applied.
//
// Copy stays calm/informational per the persona rule — no "security risk"
// scare language, just what the PIN is for and the format constraint.

import { useEffect, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";

const DEFAULT_PIN_PATTERN = /^[0-9]{6,20}$/;

/** Mirrors the backend guard (Set-BitLockerTpmPin: `^[0-9]{6,20}$`) so the
 *  dialog rejects the same inputs the PowerShell module would. Exported so
 *  it can be unit-tested and reused without mounting the dialog. */
export function validatePin(pin: string, pattern: RegExp = DEFAULT_PIN_PATTERN): string | null {
  if (!pin) return "Enter a PIN";
  if (!/^[0-9]+$/.test(pin)) return "PIN must contain only digits";
  if (!pattern.test(pin)) return "PIN must be 6-20 digits";
  return null;
}

export interface PinEntryDialogProps {
  isOpen: boolean;
  /** Toggle label, used in the dialog title ("Set a PIN for <label>"). */
  toggleLabel: string;
  /** Short explanation of what the PIN is for. */
  description: string;
  pinPattern?: RegExp;
  /** Called with the validated PIN. May be async — the dialog stays open
   *  with the confirm button in a loading state until it resolves, then
   *  closes. Throw (or let the promise reject) to keep the dialog open;
   *  the caller is responsible for surfacing the error (e.g. via toast). */
  onConfirm: (pin: string) => void | Promise<void>;
  /** Called on Cancel / outside-click / Escape — the toggle stays off. */
  onCancel: () => void;
}

interface PinEntryDialogBodyProps {
  toggleLabel: string;
  description: string;
  pin: string;
  validationError: string | null;
  busy: boolean;
  inputId: string;
  errorId: string;
  onPinChange: (value: string) => void;
  onBlur: () => void;
  onEnter: () => void;
  onCancel: () => void;
  onConfirm: () => void;
}

// Pure/stateless — no Radix Portal, so it can be exercised directly with
// react-dom/server (Radix Dialog's portal renders empty on the server,
// which is why this markup lives outside <DialogContent>'s children prop
// as its own testable piece rather than only inside the live dialog).
export function PinEntryDialogBody({
  toggleLabel,
  description,
  pin,
  validationError,
  busy,
  inputId,
  errorId,
  onPinChange,
  onBlur,
  onEnter,
  onCancel,
  onConfirm,
}: PinEntryDialogBodyProps) {
  return (
    <>
      <DialogHeader>
        <DialogTitle>Set a PIN for {toggleLabel}</DialogTitle>
        <DialogDescription>{description}</DialogDescription>
      </DialogHeader>

      <label htmlFor={inputId} className="flex flex-col gap-1.5">
        <span className="text-[13px] font-medium text-[var(--text)]">PIN</span>
        <Input
          id={inputId}
          type="password"
          inputMode="numeric"
          autoComplete="off"
          placeholder="6-20 digits"
          value={pin}
          maxLength={20}
          aria-invalid={!!validationError}
          aria-describedby={validationError ? errorId : undefined}
          onChange={(e) => onPinChange(e.target.value.replace(/\D/g, "").slice(0, 20))}
          onBlur={onBlur}
          onKeyDown={(e) => { if (e.key === "Enter") onEnter(); }}
          disabled={busy}
          autoFocus
        />
        {validationError && (
          <span id={errorId} className="text-[12px] text-[var(--danger,#e5484d)]">
            {validationError}
          </span>
        )}
      </label>

      <DialogFooter>
        <Button variant="outline" onClick={onCancel} disabled={busy}>
          Cancel
        </Button>
        <Button onClick={onConfirm} disabled={busy || !!validationError}>
          {busy ? "Turning on…" : "Turn on"}
        </Button>
      </DialogFooter>
    </>
  );
}

export default function PinEntryDialog({
  isOpen,
  toggleLabel,
  description,
  pinPattern = DEFAULT_PIN_PATTERN,
  onConfirm,
  onCancel,
}: PinEntryDialogProps) {
  const [pin, setPin] = useState("");
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const inputId = useId();
  const errorId = useId();

  // Reset on every open so a previous attempt's PIN/error never leaks in.
  useEffect(() => {
    if (isOpen) {
      setPin("");
      setTouched(false);
      setBusy(false);
    }
  }, [isOpen]);

  const validationError = touched ? validatePin(pin, pinPattern) : null;

  const handleConfirm = async () => {
    setTouched(true);
    const err = validatePin(pin, pinPattern);
    if (err) return;
    setBusy(true);
    try {
      await onConfirm(pin);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => { if (!open && !busy) onCancel(); }}>
      <DialogContent>
        <PinEntryDialogBody
          toggleLabel={toggleLabel}
          description={description}
          pin={pin}
          validationError={validationError}
          busy={busy}
          inputId={inputId}
          errorId={errorId}
          onPinChange={setPin}
          onBlur={() => setTouched(true)}
          onEnter={handleConfirm}
          onCancel={onCancel}
          onConfirm={handleConfirm}
        />
      </DialogContent>
    </Dialog>
  );
}
