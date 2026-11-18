export type CameraPresence = "present" | "absent" | "host_absent" | "unknown";

export function isCameraPresence(value: unknown): value is CameraPresence {
  return value === "present" || value === "absent" || value === "host_absent" || value === "unknown";
}

export function hasNoDetectedCamera(value: CameraPresence | null): boolean {
  return value === "absent" || value === "host_absent";
}

/** Share reads within a generation; serialize new generations behind older reads. */
export function createCameraPresenceProbe(read: () => Promise<unknown>, timeoutMs = 20_000): (generation?: number) => Promise<CameraPresence> {
  const pending = new Map<number, Promise<CameraPresence>>();
  let tail: Promise<unknown> = Promise.resolve();
  return (generation = 0) => {
    const existing = pending.get(generation);
    if (existing) return existing;
    const query: Promise<CameraPresence> = tail.then(async () => {
      let timer: ReturnType<typeof setTimeout>;
      const deadline = new Promise<unknown>(resolve => { timer = setTimeout(() => resolve(null), timeoutMs); });
      try {
        const value = await Promise.race([Promise.resolve().then(read), deadline]);
        return isCameraPresence(value) ? value : "unknown";
      } catch {
        return "unknown";
      } finally {
        clearTimeout(timer!);
      }
    });
    pending.set(generation, query);
    tail = query;
    void query.finally(() => { if (pending.get(generation) === query) pending.delete(generation); });
    return query;
  };
}

export function cameraDisplayState(presence: CameraPresence, blocked: boolean | null): "absent" | "unknown" | "allowed" | "blocked" {
  if (hasNoDetectedCamera(presence)) return "absent";
  if (presence === "unknown" || blocked === null) return "unknown";
  return blocked ? "blocked" : "allowed";
}

export function cameraAccessState(data: { disabled?: unknown; app_access_mixed?: unknown; desktop_apps_allowed?: unknown } | undefined): boolean | null {
  if (typeof data?.disabled !== "boolean") return null;
  return data.disabled;
}

// Both views share only hardware evidence; permissions and Shield readiness are separate.
export function createCameraPresenceStore() {
  let value: CameraPresence | null = null;
  let epoch = 0;
  const listeners = new Set<(value: CameraPresence | null) => void>();
  const notify = () => listeners.forEach(listener => listener(value));
  return {
    get: () => value,
    epoch: () => epoch,
    publish(next: CameraPresence, generation: number) {
      if (generation !== epoch || (next === "unknown" && value !== null)) return;
      value = next;
      notify();
    },
    reset() { epoch++; value = null; notify(); },
    subscribe(listener: (value: CameraPresence | null) => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
  };
}
export const cameraPresenceStore = createCameraPresenceStore();
export const CAMERA_RECHECK_EVENT = "wincommander:camera-recheck";
export function requestCameraRecheck(): void {
  cameraPresenceStore.reset();
  window.dispatchEvent(new Event(CAMERA_RECHECK_EVENT));
}
