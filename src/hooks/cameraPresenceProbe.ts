export type CameraPresence = "present" | "absent" | "unknown";

/** Share the in-progress query across dashboard remounts and focus events. */
export function createCameraPresenceProbe(read: () => Promise<unknown>, timeoutMs = 20_000): () => Promise<CameraPresence> {
  let pending: Promise<CameraPresence> | null = null;
  return () => {
    if (!pending) {
      let timer: ReturnType<typeof setTimeout>;
      const deadline = new Promise<unknown>(resolve => { timer = setTimeout(() => resolve(null), timeoutMs); });
      pending = Promise.race([Promise.resolve().then(read), deadline]).then(
        value => value === "present" || value === "absent" ? value : "unknown",
        () => "unknown" as const,
      ).finally(() => { clearTimeout(timer); pending = null; });
    }
    return pending;
  };
}

export function cameraDisplayState(presence: CameraPresence, blocked: boolean | null): "absent" | "unknown" | "allowed" | "blocked" {
  if (presence === "absent") return "absent";
  if (blocked === null) return "unknown";
  return blocked ? "blocked" : "allowed";
}

export function cameraAccessState(data: { disabled?: unknown; app_access_mixed?: unknown; desktop_apps_allowed?: unknown } | undefined): boolean | null {
  if (typeof data?.disabled !== "boolean" || data.app_access_mixed === true) return null;
  if (typeof data.desktop_apps_allowed === "boolean" && data.desktop_apps_allowed === data.disabled) return null;
  return data.disabled;
}
