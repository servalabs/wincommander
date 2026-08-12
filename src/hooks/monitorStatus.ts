// src/hooks/monitorStatus.ts
//
// Typed wrappers for the two extra monitor-status IPC calls the privacy
// panel polls to count armed monitors (auth-anomaly + session-assurance).
// Raw `invoke` lives only in src/hooks/** per the IPC-layering guard; the
// response shapes are hand-typed because the Free wrappers forward opaque
// JSON from the Pro sidecar (no ts-rs binding for these calls).

import { invoke } from "@tauri-apps/api/core";

const MONITOR_SUBJECT_ID = "self";

export interface MonitorRunningStatus {
  running: boolean;
}

/** Auth-anomaly (impossible-travel / new-device) monitor status. */
export const authAnomalyStatus = (): Promise<MonitorRunningStatus> =>
  invoke<MonitorRunningStatus>("auth_anomaly_status");

/** Session-assurance (presence) monitor status for the canonical self subject. */
export const sessionMonitorStatus = (): Promise<MonitorRunningStatus> =>
  invoke<MonitorRunningStatus>("session_monitor_status", {
    subjectId: MONITOR_SUBJECT_ID,
    orgId: "self",
  });

/** Start the Session Assurance collector with the same safe defaults used by
 * the former standalone control. */
export const startSessionMonitor = (): Promise<void> =>
  Promise.race([
    invoke("start_session_monitor", {
      subjectId: MONITOR_SUBJECT_ID,
      orgId: "self",
      deviceId: typeof navigator !== "undefined" ? navigator.userAgent : "self-device",
      modelLevel: "balanced",
      checkGaze: true,
      checkFaces: true,
      checkSecondaryDevice: false,
      silentMode: false,
    }),
    new Promise<never>((_, reject) => {
      setTimeout(() => reject(new Error("Session monitor start timed out — the backend did not respond in time.")), 10_000);
    }),
  ]).then(() => undefined);

export const stopSessionMonitor = (): Promise<void> =>
  invoke("stop_session_monitor", {
    subjectId: MONITOR_SUBJECT_ID,
    orgId: "self",
  });

export const startAuthAnomalyMonitor = (): Promise<void> =>
  invoke("start_auth_anomaly_monitor");

export const stopAuthAnomalyMonitor = (): Promise<void> =>
  invoke("stop_auth_anomaly_monitor");
