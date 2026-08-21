// Narrow dialog bridge for eager legacy dialogs. It intentionally contains
// only the controlled Blueprint-style surface still needed at startup.
import * as React from "react";
import * as RDialog from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { cn } from "../../lib/utils";
import { DURATION } from "../shared/motion";
import { Icon, type IconName } from "./icon";

export function CompatDialog({ isOpen, onClose, title, icon, canEscapeKeyClose = true, canOutsideClickClose = true, className, style, children, isCloseButtonShown = true, onOpened }: {
  isOpen?: boolean; onClose?: () => void; title?: React.ReactNode; icon?: IconName;
  canEscapeKeyClose?: boolean; canOutsideClickClose?: boolean; className?: string;
  style?: React.CSSProperties; children?: React.ReactNode; isCloseButtonShown?: boolean; onOpened?: () => void;
}) {
  return <RDialog.Root open={isOpen} onOpenChange={(open) => open ? onOpened?.() : onClose?.()}>
    <RDialog.Portal>
      <RDialog.Overlay className="fixed inset-0 z-[var(--z-modal)] bg-black/70 data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=closed]:animate-out data-[state=closed]:fade-out-0" style={{ animationDuration: `var(--dur-normal, ${DURATION.normal}ms)`, animationTimingFunction: "var(--ease)" }} />
      <RDialog.Content
        onEscapeKeyDown={(event) => { if (!canEscapeKeyClose) event.preventDefault(); }}
        onPointerDownOutside={(event) => { if (!canOutsideClickClose) event.preventDefault(); }}
        className={cn("gpu-clip fixed left-1/2 top-1/2 z-[var(--z-modal)] w-full max-h-[calc(100vh-2rem)] max-w-[calc(100vw-2rem)] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-[var(--r-lg)] border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-[var(--shadow)] outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95", className)}
        style={{ ...style, animationDuration: `var(--dur-normal, ${DURATION.normal}ms)`, animationTimingFunction: "var(--ease)" }}
      >
        {title != null ? <div className="flex items-center gap-2 px-5 py-3.5 border-b border-[var(--border)]">
          {icon != null && <Icon icon={icon} size={16} className="text-[var(--text-dim)]" />}
          <RDialog.Title className="min-w-0 flex-1 break-words font-[family-name:var(--font-display)] text-[15px] font-semibold leading-tight text-[var(--text)]">{title}</RDialog.Title>
          {isCloseButtonShown && <RDialog.Close aria-label="Close" className="grid size-7 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] hover:bg-[var(--surface-2)] hover:text-[var(--text)] transition-colors"><X size={16} /></RDialog.Close>}
        </div> : <RDialog.Title className="sr-only">Dialog</RDialog.Title>}
        {children}
      </RDialog.Content>
    </RDialog.Portal>
  </RDialog.Root>;
}

export function CompatDialogBody({ children, className }: { children?: React.ReactNode; className?: string }) {
  return <div className={cn("px-5 py-4 overflow-y-auto", className)}>{children}</div>;
}

export function CompatDialogFooter({ children, actions, className }: { children?: React.ReactNode; actions?: React.ReactNode; className?: string }) {
  return <div className={cn("flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--border)]", className)}>{children}{actions}</div>;
}
