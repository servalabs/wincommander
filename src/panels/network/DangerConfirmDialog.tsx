// Confirmation dialog for the destructive network-maintenance actions, built
// on the v2 alert-dialog so NetworkMaintenanceTools stays single-kit.
//
// Friction matches the house ConfirmIrreversibleDialog: a consequence list plus
// an enforced pause before the action button arms. WHY a pause and not a typed
// phrase: same reasoning as ConfirmIrreversibleDialog — a clear label plus a
// brief delay stops accidental clicks without insulting deliberate users.
import { useEffect, useRef, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../../components/ui/alert-dialog";
import { buttonVariants } from "../../components/ui/button";
import { Icon } from "../../components/ui/icon";

interface DangerConfirmDialogProps {
  open: boolean;
  onOpenChange: (next: boolean) => void;
  onConfirm: () => void;
  title: string;
  intro: string;
  consequences: string[];
  actionLabel: string;
  /** 0 disables the pause — used for reversible actions that still deserve a
   *  consequence list (enable/disable a firewall rule). */
  countdownSeconds?: number;
}

export function DangerConfirmDialog({
  open,
  onOpenChange,
  onConfirm,
  title,
  intro,
  consequences,
  actionLabel,
  countdownSeconds = 3,
}: DangerConfirmDialogProps) {
  const [remaining, setRemaining] = useState(countdownSeconds);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Restart the pause on every open — a stale 0 from a previous open would let
  // the next one be confirmed instantly.
  useEffect(() => {
    if (!open) return;
    setRemaining(countdownSeconds);
    if (countdownSeconds <= 0) return;
    timerRef.current = setInterval(() => setRemaining((r) => Math.max(0, r - 1)), 1000);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
      timerRef.current = null;
    };
  }, [open, countdownSeconds]);

  const armed = remaining <= 0;

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="flex items-center gap-2">
            <Icon icon="warning-sign" size={16} intent="danger" />
            {title}
          </AlertDialogTitle>
          <AlertDialogDescription>{intro}</AlertDialogDescription>
        </AlertDialogHeader>

        <ul className="mt-3 flex flex-col gap-1.5 rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] p-3 text-[12px] leading-relaxed text-[var(--text-dim)]">
          {consequences.map((line) => (
            <li key={line} className="flex gap-2">
              <span aria-hidden className="mt-[3px] size-1 shrink-0 rounded-full bg-[var(--text-mute)]" />
              <span>{line}</span>
            </li>
          ))}
        </ul>

        <AlertDialogFooter className="pt-4">
          <AlertDialogCancel>Back</AlertDialogCancel>
          <AlertDialogAction
            disabled={!armed}
            className={buttonVariants({ variant: "danger" })}
            onClick={onConfirm}
          >
            {armed ? actionLabel : `${actionLabel} (${remaining}s)`}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
