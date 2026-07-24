import { useEffect } from "react";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import {
  currentMonitor,
  cursorPosition,
  monitorFromPoint,
  primaryMonitor,
  type Monitor,
} from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getNotificationWindowBounds } from "../lib/notificationWindowPosition";

interface NotificationPayload {
  id: number;
  title: string;
  body: string;
  severity?: "info" | "warning" | "danger";
  source?: string;
}

const NOTIFICATION_LABEL = "notification-alerts";
const NOTIFICATION_READY_EVENT = "wc-custom-notification-ready";
const NOTIFICATION_PING_EVENT = "wc-custom-notification-ping";
const NOTIFICATION_EVENT = "wc-custom-notification";
const WINDOW_READY_TIMEOUT_MS = 4_000;

let isNotificationWindowReady = false;
let notificationQueue: Promise<void> = Promise.resolve();
const readyWaiters = new Set<() => void>();

function markNotificationWindowReady() {
  isNotificationWindowReady = true;
  readyWaiters.forEach((resolve) => resolve());
  readyWaiters.clear();
}

function errorText(error: unknown) {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

async function waitForCreated(windowRef: WebviewWindow) {
  await new Promise<void>((resolve, reject) => {
    let isSettled = false;
    const settle = (callback: () => void) => {
      if (isSettled) return;
      isSettled = true;
      window.clearTimeout(timeout);
      callback();
    };
    const timeout = window.setTimeout(
      () => settle(() => reject(new Error("notification window creation timed out"))),
      WINDOW_READY_TIMEOUT_MS,
    );

    windowRef.once("tauri://created", () => settle(resolve)).catch(reject);
    windowRef.once("tauri://error", (event) => {
      settle(() => reject(new Error(errorText(event.payload))));
    }).catch(reject);
  });
}

async function waitForReady(windowRef: WebviewWindow) {
  if (isNotificationWindowReady) return;

  await new Promise<void>((resolve, reject) => {
    const finish = () => {
      window.clearTimeout(timeout);
      readyWaiters.delete(finish);
      resolve();
    };
    const timeout = window.setTimeout(() => {
      readyWaiters.delete(finish);
      reject(new Error("notification window did not become ready"));
    }, WINDOW_READY_TIMEOUT_MS);

    readyWaiters.add(finish);
    windowRef.emit(NOTIFICATION_PING_EVENT, {}).catch(() => {});
  });
}

async function getNotificationWindow() {
  const existing = await WebviewWindow.getByLabel(NOTIFICATION_LABEL);
  if (existing) {
    await waitForReady(existing);
    return existing;
  }

  isNotificationWindowReady = false;
  const created = new WebviewWindow(NOTIFICATION_LABEL, {
    url: "index.html",
    title: "WinCommander Alerts",
    decorations: false,
    transparent: true,
    shadow: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    resizable: false,
    width: 440,
    height: 520,
    visible: false,
    focus: false,
  });

  try {
    await waitForCreated(created);
  } catch (error) {
    if (!errorText(error).toLowerCase().includes("already exists")) throw error;
    const recovered = await WebviewWindow.getByLabel(NOTIFICATION_LABEL);
    if (!recovered) throw error;
    await waitForReady(recovered);
    return recovered;
  }

  await waitForReady(created);
  return created;
}

async function getRelevantMonitor(): Promise<Monitor | null> {
  try {
    const cursor = await cursorPosition();
    const cursorMonitor = await monitorFromPoint(cursor.x, cursor.y);
    if (cursorMonitor) return cursorMonitor;
  } catch {
    // The main-window and primary-monitor fallbacks still give deterministic placement.
  }
  return (await currentMonitor().catch(() => null))
    ?? (await primaryMonitor().catch(() => null));
}

async function positionBottomRight(windowRef: WebviewWindow) {
  const monitor = await getRelevantMonitor();
  if (!monitor) return;

  const bounds = getNotificationWindowBounds(monitor.workArea, monitor.scaleFactor);
  await windowRef.setSize(new PhysicalSize(bounds.width, bounds.height));
  await windowRef.setPosition(new PhysicalPosition(bounds.x, bounds.y));
}

async function showExternalNotification(payload: NotificationPayload) {
  const windowRef = await getNotificationWindow();
  await positionBottomRight(windowRef);
  await windowRef.setAlwaysOnTop(true);
  await windowRef.emit(NOTIFICATION_EVENT, payload);
}

export default function ExternalNotificationBridge() {
  useEffect(() => {
    const disposers: Array<() => void> = [];
    let isDisposed = false;

    const setup = async () => {
      const disposeReady = await listen(NOTIFICATION_READY_EVENT, markNotificationWindowReady);
      if (isDisposed) {
        disposeReady();
        return;
      }
      disposers.push(disposeReady);

      const disposeNotification = await listen<NotificationPayload>(
        "wc-native-notification",
        (event) => {
          notificationQueue = notificationQueue
            .catch(() => {})
            .then(() => showExternalNotification(event.payload));
          notificationQueue.catch((error) => {
            isNotificationWindowReady = false;
            console.warn("[Notify] external notification failed", error);
          });
        },
      );
      if (isDisposed) {
        disposeNotification();
        return;
      }
      disposers.push(disposeNotification);

      const disposeAll = await listen("wc-native-notification-dismiss", () => {
        WebviewWindow.getByLabel(NOTIFICATION_LABEL)
          .then((windowRef) => windowRef?.emit("wc-custom-notification-dismiss", {}))
          .catch(() => {});
      });
      if (isDisposed) {
        disposeAll();
        return;
      }
      disposers.push(disposeAll);

      const disposeOne = await listen<number>("wc-native-notification-dismiss-id", (event) => {
        WebviewWindow.getByLabel(NOTIFICATION_LABEL)
          .then((windowRef) => windowRef?.emit("wc-custom-notification-dismiss-id", event.payload))
          .catch(() => {});
      });
      if (isDisposed) {
        disposeOne();
        return;
      }
      disposers.push(disposeOne);
    };

    setup().catch((error) => {
      console.warn("[Notify] notification bridge setup failed", error);
    });

    return () => {
      isDisposed = true;
      disposers.forEach((dispose) => dispose());
    };
  }, []);

  return null;
}
