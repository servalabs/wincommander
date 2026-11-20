// Event-driven refresh for changes made to this device from outside the UI
// (a Fleet command applied by the Pro agent, a policy epoch adopted by the
// native loop). Every signal here is a refresh trigger only: it never carries a
// setting value, so the UI keeps showing state it has read back.

/** Native: emitted after any settings write, including Fleet hand-off merges. */
export const SETTINGS_CHANGED_EVENT = "settings-changed";
/** Pro agent: a command or policy epoch has finished changing this device. */
export const DEVICE_STATE_CHANGED_EVENT = "device-state-changed";
/** Window event that fans a device-state change out to status readers. */
export const DEVICE_STATE_REFRESH_EVENT = "wincommander:device-state-refresh";

/** Several events from one command (settings write, notice, epoch) become one read. */
export const DEVICE_STATE_DEBOUNCE_MS = 250;
export const BUSY_RETRY_MS = 1_000;
export const BUSY_RETRY_ATTEMPTS = 3;

type Clock = Pick<typeof globalThis, "setTimeout" | "clearTimeout">;

interface Options {
  /** Re-reads settings (native settings store and the query cache). */
  readSettings: () => Promise<void>;
  /** Tells status readers (panels, monitors) to re-read their device state. */
  announceDevice: () => void;
  delayMs?: number;
  clock?: Clock;
  onError?: (error: unknown) => void;
}

/**
 * Coalesces change events into one settings read. A settings-only change (any
 * write, including the UI's own) just re-reads settings. A device-state change
 * additionally announces after that read, so status readers never see settings
 * older than the change that woke them. Events that arrive during a read are
 * folded into exactly one follow-up read.
 */
export function createDeviceStateRefresher({
  readSettings,
  announceDevice,
  delayMs = DEVICE_STATE_DEBOUNCE_MS,
  clock = globalThis,
  onError = () => undefined,
}: Options) {
  let disposed = false;
  let running = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pendingDevice = false;
  let pending = false;

  const schedule = () => {
    if (disposed || running || timer !== undefined) return;
    timer = clock.setTimeout(() => {
      timer = undefined;
      void run();
    }, delayMs);
  };

  const run = async () => {
    if (disposed || !pending) return;
    const announce = pendingDevice;
    pending = false;
    pendingDevice = false;
    running = true;
    try {
      await readSettings();
    } catch (error) {
      onError(error);
    } finally {
      running = false;
    }
    if (disposed) return;
    // Announce even if the settings read failed: status readers do their own reads.
    if (announce) {
      try { announceDevice(); } catch (error) { onError(error); }
    }
    if (pending) schedule();
  };

  const request = (device: boolean) => {
    if (disposed) return;
    pending = true;
    pendingDevice ||= device;
    schedule();
  };

  return {
    settingsChanged: () => request(false),
    deviceStateChanged: () => request(true),
    dispose() {
      disposed = true;
      if (timer !== undefined) clock.clearTimeout(timer);
      timer = undefined;
    },
  };
}

type Outcome = "ran" | "skipped";

/**
 * A refresh already in flight started before the device changed, so a skipped
 * attempt would lose the change. Retry a few times until one actually runs.
 */
export async function refreshWhenIdle(
  attempt: () => Promise<Outcome>,
  wait: (ms: number) => Promise<void> = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  attempts = BUSY_RETRY_ATTEMPTS,
  delayMs = BUSY_RETRY_MS,
): Promise<Outcome> {
  for (let tried = 0; tried < attempts; tried++) {
    if (await attempt() === "ran") return "ran";
    if (tried + 1 < attempts) await wait(delayMs);
  }
  return "skipped";
}

/** Subscribe to device-state announcements. Returns the unsubscribe function. */
export function onDeviceStateRefresh(
  callback: () => void,
  target: EventTarget | undefined = typeof window === "undefined" ? undefined : window,
): () => void {
  if (!target) return () => undefined;
  const listener = () => callback();
  target.addEventListener(DEVICE_STATE_REFRESH_EVENT, listener);
  return () => target.removeEventListener(DEVICE_STATE_REFRESH_EVENT, listener);
}

export function announceDeviceStateRefresh(
  target: EventTarget | undefined = typeof window === "undefined" ? undefined : window,
): void {
  target?.dispatchEvent(new Event(DEVICE_STATE_REFRESH_EVENT));
}
