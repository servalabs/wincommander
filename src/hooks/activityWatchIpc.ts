// Typed IPC for ActivityWatch's loopback proxy. Raw invoke() lives only in
// src/hooks/** (see eslint no-restricted-imports). The Free-side command is
// commander-free/src/activity_watch_autostart.rs::activity_watch_request —
// path-constrained to /api/0/*, never a general HTTP proxy.

import { invoke } from "@tauri-apps/api/core";

/** Fetch JSON from ActivityWatch via the native process (avoids CORS in WebView). */
export async function activityWatchRequest<T>(path: string): Promise<T> {
  return invoke<T>("activity_watch_request", { path });
}

/** Ask the single native supervisor to reuse a healthy tracker or start it. */
export async function ensureActivityWatchStarted(): Promise<void> {
  await invoke("activity_watch_ensure_started");
}
