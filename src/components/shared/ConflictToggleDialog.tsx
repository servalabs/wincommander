// src/components/shared/ConflictToggleDialog.tsx
//
// Confirmation dialog shown when enabling a toggle would automatically
// disable one or more mutually-exclusive toggles (declared via
// `conflictsWith` in the toggle registry — see src/types/toggles.ts).
// Generic/data-driven: the caller (ToggleSection) resolves which toggles
// conflict and passes their labels; this component only renders the prompt.

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

export interface ConflictToggleDialogProps {
  /** Whether the dialog is visible. Controlled externally — there is no
   *  trigger element, since the toggle click itself opens this dialog. */
  isOpen: boolean;
  /** Label of the toggle the user is trying to turn on. */
  toggleLabel: string;
  /** Labels of the currently-active toggles that conflict with it. */
  conflictingLabels: string[];
  /** Called on Cancel / outside-click / Escape — the toggle stays as-is. */
  onCancel: () => void;
  /** Called on Confirm — caller enables this toggle and disables the
   *  conflicting one(s). */
  onConfirm: () => void;
}

export default function ConflictToggleDialog({
  isOpen,
  toggleLabel,
  conflictingLabels,
  onCancel,
  onConfirm,
}: ConflictToggleDialogProps) {
  const conflictList = conflictingLabels.join(", ");

  return (
    <AlertDialog open={isOpen} onOpenChange={(open) => { if (!open) onCancel(); }}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Conflicting settings</AlertDialogTitle>
          <AlertDialogDescription>
            Turning on <strong className="text-[var(--text)]">{toggleLabel}</strong> will
            automatically turn off <strong className="text-[var(--text)]">{conflictList}</strong>{" "}
            because both settings cannot be active at the same time.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          {/* Cancel's own dismissal already flows through onOpenChange below
              (Radix closes on click) — no separate onClick needed here. */}
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={onConfirm}>
            Turn on anyway
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
