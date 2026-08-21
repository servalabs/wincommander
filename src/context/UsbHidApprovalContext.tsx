import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { listen } from "@tauri-apps/api/event";
import {
  pendingApprovals,
  type PendingUsbHidApproval,
  type UsbHidApprovalGateStatus,
  type UsbHidVisualChallenge,
  type UsbHidVisualChallengeDigitResult,
} from "../lib/usbHidApproval";
import { usbHidApprovalIpc } from "../hooks/usbHidApprovalIpc";

interface UsbHidApprovalContextValue {
  pending: PendingUsbHidApproval[];
  status: UsbHidApprovalGateStatus | null;
  refresh: () => Promise<void>;
  start: (approvalTtlSecs: number) => Promise<void>;
  stop: () => Promise<void>;
  keepBlocked: (deviceKey: string) => Promise<void>;
  beginVisualChallenge: (
    deviceKey: string,
    action: UsbHidVisualChallenge["action"],
  ) => Promise<UsbHidVisualChallenge>;
  submitVisualChallengeDigit: (
    deviceKey: string,
    challengeId: string,
    step: number,
    digit: string,
  ) => Promise<UsbHidVisualChallengeDigitResult>;
}

const UsbHidApprovalContext = createContext<UsbHidApprovalContextValue | null>(null);

export function UsbHidApprovalProvider({ children }: { children: ReactNode }) {
  const [pending, setPending] = useState<PendingUsbHidApproval[]>([]);
  const [status, setStatus] = useState<UsbHidApprovalGateStatus | null>(null);

  const refresh = useCallback(async () => {
    const [nextStatus, list] = await Promise.all([
      usbHidApprovalIpc.status(),
      usbHidApprovalIpc.list(),
    ]);
    const unresolved = pendingApprovals(list.items);
    setStatus(nextStatus);
    setPending(unresolved);
    // A device can already be pending when this process resumes from the tray;
    // reveal the existing main window in this user session as well as on a new
    // notification event. Native notification delivery remains backend-owned.
    if (unresolved.length > 0) {
      void usbHidApprovalIpc.revealSecurityAlert().catch(() => {});
    }
  }, []);

  const receiveUpdate = useCallback((item: PendingUsbHidApproval) => {
    setPending((current) => {
      const withoutItem = current.filter((candidate) => candidate.deviceKey !== item.deviceKey);
      return item.status === "pending" || item.status === "containmentFailed"
        ? [item, ...withoutItem]
        : withoutItem;
    });
    if (item.status === "pending" || item.status === "containmentFailed") {
      void usbHidApprovalIpc.revealSecurityAlert().catch(() => {});
    }
  }, []);

  useEffect(() => {
    let mounted = true;
    void refresh().catch(() => {
      if (mounted) {
        setPending([]);
        setStatus(null);
      }
    });
    const pendingListener = listen<PendingUsbHidApproval>("usb-hid-approval-pending", (event) => {
      receiveUpdate(event.payload);
      void refresh().catch(() => {});
    });
    const updateListener = listen<PendingUsbHidApproval>("usb-hid-approval-updated", (event) => {
      receiveUpdate(event.payload);
      void refresh().catch(() => {});
    });
    return () => {
      mounted = false;
      void pendingListener.then((unlisten) => unlisten());
      void updateListener.then((unlisten) => unlisten());
    };
  }, [receiveUpdate, refresh]);

  const start = useCallback(async (approvalTtlSecs: number) => {
    await usbHidApprovalIpc.start(approvalTtlSecs);
    await refresh();
  }, [refresh]);

  const stop = useCallback(async () => {
    await usbHidApprovalIpc.stop();
    await refresh();
  }, [refresh]);

  const keepBlocked = useCallback(async (deviceKey: string) => {
    await usbHidApprovalIpc.keepBlocked(deviceKey);
    setPending((current) => current.filter((item) => item.deviceKey !== deviceKey));
    await refresh();
  }, [refresh]);

  const beginVisualChallenge = useCallback(async (
    deviceKey: string,
    action: UsbHidVisualChallenge["action"],
  ) => usbHidApprovalIpc.beginChallenge(deviceKey, action), []);

  const submitVisualChallengeDigit = useCallback(async (
    deviceKey: string,
    challengeId: string,
    step: number,
    digit: string,
  ) => {
    const result = await usbHidApprovalIpc.submitChallengeDigit(
      deviceKey,
      challengeId,
      step,
      digit,
    );
    await refresh();
    return result;
  }, [refresh]);

  const value = useMemo(() => ({
    pending,
    status,
    refresh,
    start,
    stop,
    keepBlocked,
    beginVisualChallenge,
    submitVisualChallengeDigit,
  }), [
    pending,
    status,
    refresh,
    start,
    stop,
    keepBlocked,
    beginVisualChallenge,
    submitVisualChallengeDigit,
  ]);

  return (
    <UsbHidApprovalContext.Provider value={value}>
      {children}
    </UsbHidApprovalContext.Provider>
  );
}

export function useUsbHidApproval(): UsbHidApprovalContextValue {
  const value = useContext(UsbHidApprovalContext);
  if (!value) throw new Error("useUsbHidApproval must be used inside UsbHidApprovalProvider");
  return value;
}
