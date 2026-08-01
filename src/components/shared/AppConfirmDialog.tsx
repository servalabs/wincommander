import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "../ui/alert-dialog";

export interface AppConfirmationRequest {
  title: string;
  description: string;
  confirmLabel?: string;
  cancelLabel?: string;
  destructive?: boolean;
}

type PendingConfirmation = AppConfirmationRequest & {
  resolve: (accepted: boolean) => void;
};

type RequestConfirmation = (request: AppConfirmationRequest) => Promise<boolean>;

const AppConfirmContext = createContext<RequestConfirmation | null>(null);

export function shouldQueueConfirmation(active: boolean, promotionScheduled: boolean, queuedCount: number): boolean {
  return active || promotionScheduled || queuedCount > 0;
}

export function AppConfirmProvider({ children }: { children: ReactNode }) {
  const [active, setActive] = useState<PendingConfirmation>();
  const activeRef = useRef<PendingConfirmation | undefined>(undefined);
  const queueRef = useRef<PendingConfirmation[]>([]);
  const promotionTimerRef = useRef<number | undefined>(undefined);

  const requestConfirmation = useCallback<RequestConfirmation>((request) => (
    new Promise<boolean>((resolve) => {
      const pending = { ...request, resolve };
      if (shouldQueueConfirmation(
        activeRef.current !== undefined,
        promotionTimerRef.current !== undefined,
        queueRef.current.length,
      )) {
        queueRef.current.push(pending);
        return;
      }
      activeRef.current = pending;
      setActive(pending);
    })
  ), []);

  const settle = useCallback((accepted: boolean) => {
    const current = activeRef.current;
    if (!current) return;
    current.resolve(accepted);
    activeRef.current = undefined;
    setActive(undefined);
    promotionTimerRef.current = window.setTimeout(() => {
      promotionTimerRef.current = undefined;
      if (activeRef.current) return;
      const next = queueRef.current.shift();
      if (!next) return;
      activeRef.current = next;
      setActive(next);
    }, 0);
  }, []);

  const choose = useCallback((accepted: boolean) => {
    settle(accepted);
  }, [settle]);

  useEffect(() => () => {
    if (promotionTimerRef.current !== undefined) {
      window.clearTimeout(promotionTimerRef.current);
    }
    activeRef.current?.resolve(false);
    queueRef.current.forEach((pending) => pending.resolve(false));
    queueRef.current = [];
    activeRef.current = undefined;
  }, []);

  return (
    <AppConfirmContext.Provider value={requestConfirmation}>
      {children}
      <AlertDialog open={active !== undefined} onOpenChange={(open) => {
        if (!open) settle(false);
      }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{active?.title}</AlertDialogTitle>
            <AlertDialogDescription className="whitespace-pre-line">{active?.description}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => choose(false)}>{active?.cancelLabel ?? "Back"}</AlertDialogCancel>
            <AlertDialogAction
              className={active?.destructive === false ? undefined : "bg-[var(--danger)] text-white hover:bg-[color-mix(in_srgb,var(--danger)_82%,black)]"}
              onClick={() => choose(true)}
            >
              {active?.confirmLabel ?? "Continue"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </AppConfirmContext.Provider>
  );
}

export function useAppConfirm(): RequestConfirmation {
  const requestConfirmation = useContext(AppConfirmContext);
  if (!requestConfirmation) throw new Error("useAppConfirm must be used inside AppConfirmProvider");
  return requestConfirmation;
}
