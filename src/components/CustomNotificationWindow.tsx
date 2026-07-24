import { useCallback, useEffect, useRef, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon } from "@/components/ui/bp";

interface NotificationPayload {
  id: number;
  title: string;
  body: string;
  severity?: "info" | "warning" | "danger";
  source?: string;
}

const MAX_VISIBLE_NOTIFICATIONS = 3;
const DUPLICATE_SUPPRESS_MS = 8_000;
const DISPLAY_DURATION_MS = 8_000;
const READY_EVENT = "wc-custom-notification-ready";

function getDedupeKey(payload: NotificationPayload) {
  return [
    payload.source ?? "",
    payload.severity ?? "",
    payload.title,
    payload.body,
  ].join("\u0001");
}

export default function CustomNotificationWindow() {
  const [notifications, setNotifications] = useState<NotificationPayload[]>([]);
  const hideTimers = useRef(new Map<number, number>());
  const recentlyShown = useRef(new Map<string, number>());

  const removeNotification = useCallback((id: number) => {
    const timer = hideTimers.current.get(id);
    if (timer !== undefined) window.clearTimeout(timer);
    hideTimers.current.delete(id);
    setNotifications((current) => current.filter((item) => item.id !== id));
  }, []);

  const dismissAll = useCallback(() => {
    hideTimers.current.forEach((timer) => window.clearTimeout(timer));
    hideTimers.current.clear();
    setNotifications([]);
  }, []);

  useEffect(() => {
    const windowRef = getCurrentWindow();
    if (notifications.length === 0) {
      windowRef.setIgnoreCursorEvents(true).catch(() => {});
      windowRef.hide().catch(() => {});
    } else {
      windowRef.setIgnoreCursorEvents(false).catch(() => {});
    }
  }, [notifications.length]);

  useEffect(() => {
    const windowRef = getCurrentWindow();
    const timers = hideTimers.current;
    const shownNotifications = recentlyShown.current;
    const disposers: Array<() => void> = [];
    let isDisposed = false;

    const onNotification = (payload: NotificationPayload) => {
      const now = Date.now();
      const dedupeKey = getDedupeKey(payload);
      const lastShownAt = shownNotifications.get(dedupeKey) ?? 0;
      if (now - lastShownAt < DUPLICATE_SUPPRESS_MS) return;

      shownNotifications.set(dedupeKey, now);
      for (const [key, timestamp] of shownNotifications) {
        if (now - timestamp > DUPLICATE_SUPPRESS_MS * 3) {
          shownNotifications.delete(key);
        }
      }

      setNotifications((current) => {
        const withoutSameId = current.filter((item) => item.id !== payload.id);
        return [...withoutSameId, payload].slice(-MAX_VISIBLE_NOTIFICATIONS);
      });
      windowRef.setIgnoreCursorEvents(false)
        .then(() => windowRef.show())
        .catch((error) => {
          console.warn("[Notify] could not show notification window", error);
        });

      const existingTimer = timers.get(payload.id);
      if (existingTimer !== undefined) window.clearTimeout(existingTimer);
      timers.set(
        payload.id,
        window.setTimeout(() => removeNotification(payload.id), DISPLAY_DURATION_MS),
      );
    };

    const setup = async () => {
      const registered = await Promise.all([
        listen<NotificationPayload>("wc-custom-notification", (event) => {
          onNotification(event.payload);
        }),
        listen("wc-custom-notification-dismiss", dismissAll),
        listen<number>("wc-custom-notification-dismiss-id", (event) => {
          removeNotification(event.payload);
        }),
        listen("wc-custom-notification-ping", () => {
          emit(READY_EVENT).catch(() => {});
        }),
      ]);

      if (isDisposed) {
        registered.forEach((dispose) => dispose());
        return;
      }
      disposers.push(...registered);
      await emit(READY_EVENT);
    };

    setup().catch((error) => {
      console.warn("[Notify] notification window setup failed", error);
    });

    return () => {
      isDisposed = true;
      timers.forEach((timer) => window.clearTimeout(timer));
      timers.clear();
      disposers.forEach((dispose) => dispose());
    };
  }, [dismissAll, removeNotification]);

  return (
    <div className="wc-custom-notification-window" aria-live="polite">
      {notifications.map((payload) => (
        <div
          key={payload.id}
          className={`wc-custom-notification wc-custom-notification--${payload.severity ?? "info"}`}
          role="alert"
        >
          <div className="wc-custom-notification__icon" aria-hidden="true">
            <Icon icon="notifications" size={18} />
          </div>
          <div className="wc-custom-notification__body">
            <div className="wc-custom-notification__brand">
              <span>SERVALABS WINCOMMANDER</span>
              <span>{payload.source ?? "WinCommander"}</span>
            </div>
            <div className="wc-custom-notification__title">{payload.title}</div>
            <div className="wc-custom-notification__text">{payload.body}</div>
          </div>
          <button
            type="button"
            className="wc-custom-notification__close"
            aria-label={`Dismiss ${payload.title}`}
            onClick={() => removeNotification(payload.id)}
          >
            <Icon icon="cross" size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
