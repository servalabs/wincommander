// src/components/BackgroundPollers.tsx
//
// BackgroundPollers — low-frequency background event listeners/tasks.
// Renders nothing (returns null). Mounted once inside AppContent.
//
// REFACTORED: All polling intervals have been moved to useActivePanelPoller.
// This component now only handles:
//   1. Tauri event listeners (shred-requested, tray-shield-toggle-requested)
//   2. DOM event listeners (apps-install-missing)
//   3. Auto-start ActivityWatch when a complete local install is detected
//   4. Quiet-mode supervisor tick for ActivityWatch headless components
//
// REMOVED (moved to useActivePanelPoller + Rust sysinfo):
//   - systemPollInterval (3s refreshSystem)  → Rust get_live_metrics + active-panel poll
//   - shieldInterval (60s pollShield)        → event-driven (tray label updated at start/stop)
//   - broad productivity polling → quiet-mode-only maintenance below

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import useBackend, { executeBackendCommand } from "../hooks/useBackend";
import { useAppState } from "../context/AppContext";
import useEntitlements from "../hooks/useEntitlements";
import { isModuleEnabled } from "../types/modules";
import { showWarning, showError, showSuccess } from "../utils/toast";
import { recordEvidence } from "../lib/evidence";
import { savedRamDiskMountRequest } from "../lib/ramDisk";
import useAutoHeal from "../hooks/useAutoHeal";
import useAdoptCurrentState from "../hooks/useAdoptCurrentState";
import { privacyShieldBlurTriggers, resolvePrivacyShieldMode } from "../lib/privacyShieldMode";
import { resolveFleetPrivacyShieldControl } from "../lib/fleetPrivacyShieldControl";
import type { PanelId } from "../types/panels";

interface PasteMonitorDetected {
  pattern: string;
  /** "warning" for credential leaks, "danger" for malicious commands. */
  severity?: string;
  detected_at: string;
}

function formatPasteDangerMessage(pattern: string): string {
  const isPowerShell = /powershell|encodedcommand|executionpolicy|pwsh/i.test(pattern);
  if (isPowerShell) {
    return `Dangerous PowerShell command copied (${pattern}). Do not paste it into Win+R, Terminal, or PowerShell. Clear the clipboard unless you wrote this command yourself.`;
  }
  return `Suspicious clipboard content (${pattern}). Do not paste this into Win+R, PowerShell, or any terminal. This looks like a ClickFix/pastejacking trick.`;
}

interface DecoyAccessed {
  path: string;
  /** "modified" | "removed" | "renamed" — basename for the toast. */
  kind: string;
  detected_at: string;
}

interface RansomwareDetected {
  count: number;
  window_seconds: number;
  sample_paths: string[];
  detected_at: string;
  // v2 (Pro ETW) attribution — absent on the notify fallback path.
  pid?: number;
  image_name?: string;
  image_path?: string;
  action_taken?: string;
}

interface CoercionPhraseFired {
  label: string;
  detected_at: string;
}

interface RemoteAccessDetected {
  tool: string;
  confidence?: "info" | "high";
  reason?: string;
  port?: number;
  peer?: string;
  logHint?: string;
  detectedAt: string;
}

interface ScreenCaptureDetected {
  tool: string;
  processName: string;
  confidence: string;
  detectedAt: string;
}

interface DriverProblemDetected {
  name: string;
  class: string;
  problemCode: number | null;
  problemText: string;
  severity: string;
  instanceId: string;
  detectedAt: string;
}

interface BackgroundPollersProps {
  /** Called when a shred event arrives — provides the file path(s) to shred */
  onShredRequest: (payload: string | string[]) => void;
  /** Called to navigate to a different panel (e.g. apps-install-missing) */
  onPanelChange: (panel: PanelId) => void;
}

export default function BackgroundPollers({
  onShredRequest,
  onPanelChange,
}: BackgroundPollersProps) {
  useAutoHeal();
  useAdoptCurrentState();
  const { startPrivacyShield, invokeProductivityEngineMaintenance, testRamDiskInstalled, getRamDiskStatus, createRamDisk, getAvailableDriveLetters, getAIDependenciesStatus } = useBackend();
  const { appSettings } = useAppState();
  const { hasPaid } = useEntitlements();
  const modules = appSettings?.app?.modules;
  const productivityQuietManaged = appSettings?.ideal?.identity?.hideBackendAppsList?.includes("productivityEngine") === true;
  // RAM disk autostart config — user opt-in toggle plus per-disk spec.
  // Off by default; when on, the autostart effect below creates the
  // disk on splash-complete if it isn't already mounted at that letter.
  // Lives in app.vault (alongside defaultMountLetter / recentPaths),
  // not ideal/current — it's a preference, not a system-state mirror.
  const ramdiskAutostart = appSettings?.app?.vault?.ramdiskAutostart;
  const privacyShieldAutostart = appSettings?.ideal?.privacy?.privacyShield?.autostart === true;
  const fleetPrivacyShield = appSettings?.app?.fleet?.enabled === true
    && (appSettings?.ideal?.privacy?.privacyShield?.fleetManaged === true
      || appSettings?.app?.fleet?.privacyShieldSessionOwned === true);
  // Keep the supervisor alive for every enrolled Fleet device, even after a
  // policy turns the Shield flags off. That transition is what must stop a
  // previously Fleet-owned process and send its final lifecycle event.
  const fleetPrivacySupervisor = appSettings?.app?.fleet?.enabled === true;
  const fleetPrivacyMonitoringEnabled = fleetPrivacyShield
    && appSettings?.ideal?.privacy?.privacyShield?.fleetMonitoringEnabled === true;

  // Keep the changing values in refs so the listener-registration effect below
  // can read them WITHOUT depending on appSettings — otherwise every settings
  // write tore down + re-registered all 11 Tauri listeners (UI-freeze contributor)
  // and opened the async-unlisten race that throws the "reading 'handlerId'"
  // rejection. The effect now registers once.
  const modulesRef = useRef(modules);
  const productivityQuietManagedRef = useRef(productivityQuietManaged);
  const ramdiskAutostartRef = useRef(ramdiskAutostart);
  const privacyShieldAutostartRef = useRef(privacyShieldAutostart);
  const hasPaidRef = useRef(hasPaid);
  const appSettingsRef = useRef(appSettings);
  const fleetShieldReportedStateRef = useRef<string | null>(null);
  const fleetShieldReceivedStateRef = useRef<string | null>(null);
  // A device-scoped Fleet Start/Stop carries a durable command id. Every
  // media-free lifecycle receipt echoes only that id, never a camera detail.
  const fleetShieldCommandIdRef = useRef<string | null>(null);
  useEffect(() => {
    modulesRef.current = modules;
    productivityQuietManagedRef.current = productivityQuietManaged;
    ramdiskAutostartRef.current = ramdiskAutostart;
    privacyShieldAutostartRef.current = privacyShieldAutostart;
    hasPaidRef.current = hasPaid;
    appSettingsRef.current = appSettings;
  }, [modules, productivityQuietManaged, ramdiskAutostart, privacyShieldAutostart, hasPaid, appSettings]);

  // Fleet-managed Privacy Shield supervisor. Privacy Shield remains the sole
  // local camera client; Fleet supplies signed policy and receives status/
  // incident metadata only. If Meet, a browser, or another app owns the
  // camera, the local start simply fails, is reported as `camera_busy`, and
  // is retried on the next low-frequency tick without changing Windows camera
  // permissions or uploading a frame.
  useEffect(() => {
    if (!fleetPrivacySupervisor) return;
    let cancelled = false;
    let starting = false;
    const report = async (status: string, detail?: string) => {
      const commandId = fleetShieldCommandIdRef.current;
      const key = `${commandId ?? ""}:${status}:${detail ?? ""}`;
      if (fleetShieldReportedStateRef.current === key) return;
      // Only deduplicate a state after Pro accepts it. Marking it before the
      // IPC call made a transient sidecar failure suppress that lifecycle
      // transition forever, leaving Fleet with stale Shield state.
      try {
        await invoke("fleet_report_privacy_shield_status", { status, detail, commandId });
        fleetShieldReportedStateRef.current = key;
        return true;
      } catch {
        // Keep the prior key so the next supervisor tick retries this state.
        return false;
      }
    };
    const tick = async () => {
      if (cancelled || starting) return;
      const settings = appSettingsRef.current;
      const ps = settings?.ideal?.privacy?.privacyShield;
      if (!settings?.app?.fleet?.enabled) return;
      const fleetOwnsSession = settings.app.fleet.privacyShieldSessionOwned === true;
      // Pull the latest admin desired-state (separate from policy epochs).
      // Use this returned value in THIS tick too: patching settings alone only
      // takes effect after React re-renders, which used to make a freshly
      // delivered remote start look like an instruction that did nothing.
      let desiredState = settings.app.fleet.shieldDesiredState ?? null;
      try {
        desiredState = await invoke<typeof desiredState>("fleet_sync_shield_state");
      } catch { /* a later tick retries the authenticated state pull */ }
      fleetShieldCommandIdRef.current = desiredState?.commandId ?? null;
      const shieldControl = resolveFleetPrivacyShieldControl({
        fleetEnabled: settings.app.fleet.enabled === true,
        legacyManaged: ps?.fleetManaged === true,
        legacyMonitoringEnabled: ps?.fleetMonitoringEnabled === true,
        desiredState,
      });
      // These are durable, closed lifecycle receipts in the Pro-side protected
      // state. They let Fleet distinguish delivery from actual application;
      // no camera, application, or screen information is included.
      const receiptKey = desiredState?.updatedAt ?? `${ps?.fleetManaged === true}:${ps?.fleetMonitoringEnabled === true}`;
      if (shieldControl.managed && fleetShieldReceivedStateRef.current !== receiptKey) {
        if (await report("received")) fleetShieldReceivedStateRef.current = receiptKey;
      }
      try {
        const status = await executeBackendCommand<{ running?: boolean; cameraAvailable?: boolean; cameraMessage?: string; isWindowsServer?: boolean }>("Get-PrivacyShieldStatus");
        const running = status.success && status.data?.running === true;
        const stopOwnedSession = async () => {
          if (running) {
            const stopped = await executeBackendCommand<{ success?: boolean; message?: string }>("Stop-PrivacyShield", {});
            // Do not clear ownership or emit a false stop event on failure.
            // Keeping it set makes the next supervisor tick retry the command.
            if (!stopped.success || stopped.data?.success !== true) return false;
          }
          await invoke("patch_settings_cmd", { patch: { app: { fleet: { privacyShieldSessionOwned: false } } } }).catch(() => {});
          await invoke("update_tray_shield_label", { running: false }).catch(() => {});
          return true;
        };
        // Local/manual sessions are observable by Fleet whenever the endpoint
        // is enrolled. They remain locally stoppable unless Fleet owns them.
        // Reporting both transitions keeps the console's latest capability
        // snapshot truthful even when an employee starts or stops the Shield.
        if (!shieldControl.managed || !shieldControl.enabled) {
          if (fleetOwnsSession) {
            if (await stopOwnedSession()) await report("disabled_by_policy");
          } else {
            await report(running ? "running_local_session" : "stopped");
          }
          return;
        }
        if (running) {
          // A session already running when the supervisor first sees a
          // fleet mandate (started locally, or before this device's policy
          // sync completed) must be claimed as fleet-owned immediately, not
          // left "employee-owned" until the NEXT restart. Previously this
          // branch only reported the session and never set
          // privacyShieldSessionOwned, so PrivacyShieldCard's Stop button
          // (which used to gate solely on that flag) stayed unlocked
          // indefinitely for any session the supervisor didn't itself start.
          // The card's lock now derives directly from policy
          // (fleetShieldMandatesOn) so this claim is for reporting/quota
          // consistency, not the sole lock mechanism — but it must still
          // happen so quota charging and "who owns this session" state stay
          // correct going forward.
          if (!fleetOwnsSession) {
            await invoke("patch_settings_cmd", { patch: { app: { fleet: { privacyShieldSessionOwned: true } } } }).catch(() => {});
            await report("running_fleet_session");
            return;
          }
          // Fleet service is paid, but the local Free Privacy Shield quota is
          // intentionally still enforced. Stop and report exhaustion rather
          // than silently allowing unlimited Fleet-run camera monitoring.
          const quota = await invoke<{ is_unlimited: boolean; minutes_remaining: number }>("consume_shield_minutes", { minutes: 1.0 });
          if (!quota.is_unlimited && quota.minutes_remaining <= 0) {
            await executeBackendCommand("Stop-PrivacyShield", {});
            await report("quota_exhausted", "Daily Privacy Shield quota exhausted.");
          } else {
            await report("running_fleet_session");
          }
          return;
        }
        if (status.success && status.data?.cameraAvailable === false) {
          // Fixed, content-free capability classification: no camera name,
          // installed application, or image leaves the endpoint.
          await report(
            status.data.isWindowsServer === true
              ? "windows_server_camera_unavailable"
              : "camera_unavailable",
            status.data.cameraMessage ?? "Camera protection is unavailable.",
          );
          return;
        }
        await report("applying");
        starting = true;
        const mode = resolvePrivacyShieldMode({
          fleetManaged: shieldControl.managed,
          fleetMode: shieldControl.mode
            ?? (ps?.gazeDetectionEnabled === false
              && ps?.antiPeepingEnabled === false
              && ps?.cameraHunterEnabled === false
              ? "notify_only"
              : undefined),
          localMode: ps?.notifyMode,
        });
        const blurTriggers = privacyShieldBlurTriggers(mode, {
          gaze: ps?.gazeDetectionEnabled === true,
          faces: ps?.antiPeepingEnabled === true,
          device: ps?.cameraHunterEnabled === true,
        });
        const result = await startPrivacyShield(
          0,
          // Always collect all attention classes for Fleet. The last three
          // arguments keep visual blur bound to the signed/local card toggles.
          true, true, true,
          false, false,
          ps?.modelSize ?? "medium",
          ps?.confidenceThreshold ?? 0.5,
          ps?.blurOpacity ?? 200,
          ps?.wakeDelaySeconds ?? 150,
          ps?.deviceWakeMultiplier ?? 5,
          ps?.multiFaceWakeMultiplier ?? 5,
          ps?.detectionBufferFrames ?? 2,
          ps?.captureSpeed ?? 1,
          blurTriggers.gaze,
          blurTriggers.faces,
          blurTriggers.device,
        );
        if (result.success) {
          await invoke("patch_settings_cmd", { patch: { app: { fleet: { privacyShieldSessionOwned: true } } } }).catch(() => {});
          await invoke("update_tray_shield_label", { running: true }).catch(() => {});
          await report("running_fleet_session");
        } else {
          const detail = result.error ?? "Privacy Shield did not start.";
          await report(/camera|webcam|videocapture|in use/i.test(detail) ? "camera_busy" : "start_failed", detail);
        }
      } catch {
        // The next tick retries. Never surface a noisy local toast for an
        // administrator-managed background service.
      } finally {
        starting = false;
      }
    };
    void tick();
    const interval = setInterval(() => { void tick(); }, 60_000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [fleetPrivacySupervisor, fleetPrivacyShield, fleetPrivacyMonitoringEnabled, startPrivacyShield]);

  // Buffers for batching rapid-fire shred-requested events. When a user
  // shift/ctrl-selects N files in Explorer and picks "Shred with
  // WinCommander", the shell launches the exe once per file, so we
  // receive N separate events in <250ms. Without buffering, each event
  // would re-open the dialog and only the last path would survive. The
  // buffer collects all paths arriving within the debounce window and
  // hands the full array to the dialog in one shot.
  const shredBuffer = useRef<string[]>([]);
  const shredTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    // ── Event: navigate to Apps panel ────────────────────────────────────
    const handleAppsInstallMissing = () => onPanelChange("apps");
    window.addEventListener("apps-install-missing", handleAppsInstallMissing as EventListener);
    const handleOpenPackageUpdates = () => {
      window.__pendingAppsPackageUpdates = true;
      onPanelChange("apps");
      requestAnimationFrame(() => window.dispatchEvent(new Event("apps-package-updates-ready")));
    };
    window.addEventListener("apps-open-package-updates", handleOpenPackageUpdates);

    // ── Tauri event: shred file from context menu ─────────────────────────
    // Gate: only act if the cleanup module is enabled.
    // Multi-select: Explorer fires N rapid events for N selected files
    // (one process launch per file via the registry "%1" command). We
    // batch them into a single onShredRequest call so the dialog opens
    // once with all paths pre-populated.
    const unlistenShred = listen<string | string[]>("shred-requested", (event) => {
      if (!isModuleEnabled(modulesRef.current, 'cleanup')) return;
      const incoming = Array.isArray(event.payload) ? event.payload : [event.payload];
      // De-dupe as we go — Explorer occasionally emits the same path twice
      // when the same file is selected via different gestures.
      for (const p of incoming) {
        if (p && !shredBuffer.current.includes(p)) shredBuffer.current.push(p);
      }
      if (shredTimer.current) clearTimeout(shredTimer.current);
      shredTimer.current = setTimeout(() => {
        const paths = shredBuffer.current;
        shredBuffer.current = [];
        shredTimer.current = null;
        if (paths.length > 0) onShredRequest(paths);
      }, 500);
    });

    // ── Tauri event: tray shield toggle ───────────────────────────────────
    // Fires when user clicks "Enable Shield" in the system tray menu.
    // Gate: only act if the privacyShield module is enabled.
    const unlistenTrayShield = listen("tray-shield-toggle-requested", async () => {
      if (appSettingsRef.current?.ideal?.privacy?.privacyShield?.fleetManaged === true) {
        showWarning("Privacy Shield is managed by Fleet.");
        return;
      }
      if (!isModuleEnabled(modulesRef.current, 'privacyShield')) return;
      try {
        const shield = appSettingsRef.current?.ideal?.privacy?.privacyShield;
        const mode = resolvePrivacyShieldMode({ fleetManaged: false, localMode: shield?.notifyMode });
        const detectorTriggers = {
          gaze: shield?.gazeDetectionEnabled ?? true,
          faces: shield?.antiPeepingEnabled ?? true,
          device: shield?.cameraHunterEnabled ?? true,
        };
        const blurTriggers = privacyShieldBlurTriggers(mode, detectorTriggers);
        const res = await startPrivacyShield(0, detectorTriggers.gaze, detectorTriggers.faces, detectorTriggers.device, false, false, shield?.modelSize ?? "medium", shield?.confidenceThreshold ?? 0.5, shield?.blurOpacity ?? 200, shield?.wakeDelaySeconds ?? 150, shield?.deviceWakeMultiplier ?? 5, shield?.multiFaceWakeMultiplier ?? 5, shield?.detectionBufferFrames ?? 2, shield?.captureSpeed ?? 1, blurTriggers.gaze, blurTriggers.faces, blurTriggers.device);
        if (res.success) {
          await invoke("update_tray_shield_label", { running: true });
          showSuccess("Privacy Shield enabled.");
        } else {
          await invoke("update_tray_shield_label", { running: false }).catch(() => {});
          // Toggle result → Notifications tab, not System Alerts.
          showError(res.error || "Failed to start Privacy Shield.", undefined, { kind: "notification" });
        }
      } catch {
        await invoke("update_tray_shield_label", { running: false }).catch(() => {});
        showError("Failed to start Privacy Shield.", undefined, { kind: "notification" });
      }
    });
    const unlistenFleetShieldDenied = listen("fleet-privacy-shield-control-denied", () => {
      showWarning("Privacy Shield was started by Fleet and can only be stopped by a Fleet administrator.");
    });
    // The Fleet master policy sends only an alarm class and small counters —
    // never a clipboard pattern, process/tool name, path, peer, SSID, or
    // device name. Pro validates this closed shape again before queueing.
    const reportConfiguredFleetAlert = (
      alertType: string,
      detail: Record<string, string | number>,
      monitorReportingEnabled = false,
    ) => {
      if (appSettingsRef.current?.ideal?.security?.requireAllDeviceAlertsInFleet !== true
          && !monitorReportingEnabled) return;
      invoke("fleet_report_local_alert", { alertType, detail }).catch(() => {
        // A Fleet outage must never interfere with the local protection or toast.
      });
    };

    // ── Tauri event: F-1 paste monitor detected ─────────────────────────
    // Fires when the clipboard watcher matches a credential or malicious
    // -command pattern. Show a non-blocking toast — warning (yellow) for
    // credential leaks, error (red) for malicious commands. No clipboard
    // CONTENT is in the payload — only the pattern's display name like
    // "AWS Access Key" or "PowerShell encoded payload".
    const unlistenPasteMonitor = listen<PasteMonitorDetected>(
      "paste-monitor-detected",
      (event) => {
        const pattern = event.payload?.pattern ?? "credential";
        const severity = event.payload?.severity ?? "warning";
        recordEvidence("monitor", severity === "danger" ? "danger" : "warn", `Clipboard monitor: ${pattern}`);
        if (severity === "danger") {
          // ClickFix / pastejacking — the user is being actively
          // social-engineered to paste this somewhere. Use the loudest
          // toast we have + unambiguous "do NOT paste" copy.
          showError(formatPasteDangerMessage(pattern), 12_000);
        } else {
          showWarning(
            `Looks like you copied a ${pattern} — be careful where you paste it.`,
          );
        }
        reportConfiguredFleetAlert(
          "clipboard_guard",
          { class: "local_match", severity: severity === "danger" ? "danger" : "warning" },
          appSettingsRef.current?.ideal?.privacy?.clipboard?.pasteMonitorReportToFleet === true,
        );
      },
    );

    // ── Tauri event: F-2 decoy file accessed ─────────────────────────────
    // Filesystem honeypot fired — someone just modified / renamed /
    // removed an enrolled decoy file. Always danger severity (no
    // legitimate workflow touches files the user said they never use).
    const unlistenDecoy = listen<DecoyAccessed>(
      "decoy-accessed",
      (event) => {
        const fullPath = event.payload?.path ?? "decoy";
        const kind = event.payload?.kind ?? "accessed";
        // Strip to basename for toast brevity; full path lives in the
        // Privacy panel's recent log.
        const baseName = fullPath.split(/[/\\]/).filter(Boolean).pop() ?? fullPath;
        recordEvidence("monitor", "danger", `Decoy file ${kind}: ${baseName}`, fullPath);
        showError(
          `⚠ Decoy file ${kind}: ${baseName}. Investigate — possible malware or someone scanning for sensitive files.`,
          15_000,
        );
      },
    );

    // ── Tauri event: F-3 ransomware mass-modify detected ─────────────────
    // Threshold of file changes crossed within the rolling window.
    // The toast is the loudest one in the app — the user may have
    // SECONDS to react. Tell them what to do, not just what happened.
    const unlistenRansomware = listen<RansomwareDetected>(
      "ransomware-detected",
      (event) => {
        const count = event.payload?.count ?? 0;
        const win = event.payload?.window_seconds ?? 30;
        // v2: when the Pro ETW detector attributed + acted on a process,
        // name it and say what we did — far more actionable than v1's
        // generic "end any unfamiliar process".
        const image = event.payload?.image_name;
        const pid = event.payload?.pid;
        const act = event.payload?.action_taken;
        const who = image ? `${image}${pid ? ` (PID ${pid})` : ""}` : "";
        const actionMsg =
          act === "suspended" ? `Suspended ${who} — verify in Task Manager, then resume or end it.`
          : act === "killed" ? `Stopped ${who}.`
          : act === "suspend_failed" || act === "kill_failed" ? `Couldn't stop ${who} — end it manually in Task Manager.`
          : who ? `${who} is the likely culprit — end it in Task Manager.`
          : "End any unfamiliar process in Task Manager.";
        recordEvidence(
          "monitor",
          "danger",
          `Mass file modification: ${count} files in ${win}s${who ? ` by ${who}` : ""}${act && act !== "none" ? ` (${act})` : ""}`,
        );
        showError(
          `🚨 ${count} files modified in ${win}s. Possible ransomware. ` +
          `DISCONNECT NETWORK NOW (Ethernet/WiFi off). ${actionMsg}`,
          30_000,
        );
        reportConfiguredFleetAlert(
          "ransomware",
          { class: "mass_modification", severity: "danger", count },
          appSettingsRef.current?.ideal?.privacy?.ransomwareMonitor?.reportToFleet === true,
        );
      },
    );

    // ── Tauri event: F-5 coercion phrase fired ───────────────────────────
    // Silent — by design. The user is being actively coerced; any
    // visible UI tips off the duress-er. We invoke `full_lockdown`
    // (the universal orchestrator that runs the 17-step cleanup +
    // file erase + uninstaller + shutdown) so the lockdown is COMPLETE,
    // not just the lightweight lockdown file-erase path.
    // excludeBrowsers=false → erase browser data too (we want everything
    // gone). v2 may route this through user-configured Flow IDs once
    // G-1 ships.
    const unlistenCoercion = listen<CoercionPhraseFired>(
      "coercion-phrase-fired",
      (_event) => {
        // Log the receipt so DevTools tells you whether the Rust
        // hook is firing the event. If you type your phrase and see
        // nothing in the console, the keyboard hook isn't matching
        // (most likely cause: phrase isn't registered, or the
        // matcher's strict-no-Enter rule was tripped).
        console.log("[panic-phrase] event received; firing cascade");
        recordEvidence("lockdown", "danger", "Lockdown triggered (coercion phrase)");
        // Rust owns and executes the authorized action; this event is
        // informational only and never carries an authorization secret.
      },
    );

    // ── Tauri event: #4 remote-access monitor ───────────────────────────
    // Pro detector fires when a known remote-control tool is running
    // (confidence "info" — quiet warning) or has an established incoming
    // session (confidence "high" — loud, actionable error). The Pro
    // payload is FLAT: port/peer/logHint are top-level on RemoteAccessHit.
    const unlistenRemoteAccess = listen<RemoteAccessDetected>(
      "remote-access-detected",
      (event) => {
        const tool = event.payload?.tool ?? "a remote-access tool";
        const conf = event.payload?.confidence ?? "info";
        const peer = event.payload?.peer;
        recordEvidence("network", conf === "high" ? "danger" : "info", `Remote access: ${tool}${peer ? ` (${peer})` : ""}`);
        if (conf === "high") {
          showError(
            `⚠ Incoming remote session detected via ${tool}` +
              (peer ? ` (${peer})` : "") +
              `. If you didn't start this, end the session and disconnect now.`,
            15_000,
          );
        } else {
          showWarning(`${tool} is running — no active session detected yet.`, 5_000);
        }
        reportConfiguredFleetAlert("remote_access", {
          class: conf === "high" ? "incoming_session" : "tool_detected",
          severity: conf === "high" ? "danger" : "warning",
        });
      },
    );

    // ── Tauri event: #5 screen-capture tool detected ─────────────────────
    // Only toast high-confidence hits (a real screen-capture/recording
    // tool is running). Low-confidence (e.g. ffmpeg) stays silent.
    const unlistenScreenCapture = listen<ScreenCaptureDetected>(
      "screen-capture-detected",
      (event) => {
        const tool = event.payload?.tool ?? "A screen-capture tool";
        if ((event.payload?.confidence ?? "high") !== "high") return;
        recordEvidence("privacy", "warn", `Screen-capture tool active: ${tool}`);
        showWarning(
          `${tool} is running — your screen may be recorded. Sensitive windows can be hidden in Privacy → Screen Capture.`,
          10_000,
        );
        const settings = appSettingsRef.current;
        const reportToFleet = settings?.ideal?.security?.requireAllDeviceAlertsInFleet === true
          || settings?.ideal?.privacy?.screenCapture?.reportToFleet === true;
        if (reportToFleet) {
          invoke("fleet_report_local_alert", {
            alertType: "screen_capture",
            // Fleet needs the alert class, not a process name or executable.
            detail: { detected: "screen_capture" },
          }).catch(() => { /* best-effort — a Fleet outage must never affect local detection */ });
        }
      },
    );

    // ── Tauri event: #6 driver problem detected ──────────────────────────
    // Driver-health watcher fires for NEW critical problem devices.
    const unlistenDriverProblem = listen<DriverProblemDetected>(
      "driver-problem-detected",
      (event) => {
        const name = event.payload?.name ?? "A device";
        const text = event.payload?.problemText ?? "driver problem";
        recordEvidence("system", "warn", `Driver problem: ${name}`, text);
        showError(`Driver problem: ${name} — ${text}`, 12_000);
        reportConfiguredFleetAlert("driver_health", { class: "critical_problem", severity: "warning" });
      },
    );

    const unlistenWifiGuard = listen("wifi-guard-detected", () => {
      reportConfiguredFleetAlert(
        "wifi_guard",
        { class: "rogue_access_point", severity: "warning" },
        appSettingsRef.current?.ideal?.network?.wifiGuard?.reportToFleet === true,
      );
    });
    const unlistenNetworkHoneypot = listen("network-honeypot-detected", () => {
      reportConfiguredFleetAlert("network_honeypot", { class: "connection", severity: "warning" });
    });

    // ── Tauri event: panic-trigger-test ──────────────────────────────────
    // Diagnostic surface — fired by the "Test trigger" buttons in the
    // Panic Triggers section. Verifies the Rust→JS event pipe is alive
    // WITHOUT actually running the cascade. Source field tells the user
    // which trigger they're testing so a "panic_phrase" test confirms
    // the F-5 emit→listen path.
    const unlistenTriggerTest = listen<{ source: string }>(
      "panic-trigger-test",
      (event) => {
        const source = event.payload?.source ?? "unknown";
        const friendly = source === "panic_phrase" ? "Lockdown-word trigger"
          : "Trigger";
        showWarning(
          `${friendly} wiring OK. A real fire would run your configured Lockdown steps now (Privacy → Lockdown).`,
        );
      },
    );

    const productivityMaintenanceTimer = setInterval(async () => {
      if (!isModuleEnabled(modulesRef.current, 'productivity')) return;
      if (!productivityQuietManagedRef.current) return;
      try {
        await invokeProductivityEngineMaintenance();
      } catch {
        // Best-effort supervisor tick; do not block or notify from background maintenance.
      }
    }, 60_000);

    // ── One-shot: #6 driver-health scan/watch on startup ─────────────
    // Honors ideal.security.drivers persistence: scanOnStartup warms the
    // section's toast path; watchEnabled re-arms the Pro watcher across
    // restarts (the section itself only starts it on user toggle). Delay
    // ~6s to clear the splash storm. Best-effort — never blocks startup.
    const driverScanTimer = setTimeout(async () => {
      const drivers = appSettingsRef.current?.ideal?.security?.drivers;
      try {
        if (drivers?.scanOnStartup) {
          await invoke("get_driver_health");
        }
        if (drivers?.watchEnabled) {
          await invoke("start_driver_watch", { intervalSecs: drivers?.watchIntervalSecs ?? null });
        }
      } catch {
        // Best-effort; don't block startup (unlicensed / no Pro → no-op).
      }
    }, 6000);

    // ── One-shot: Auto-create RAM disk if user enabled autostart ──────
    // Mirrors the productivity-autostart pattern above. Runs once,
    // ~6s after splash to let ImDisk's device driver settle. Skips
    // silently if:
    //   • Autostart is disabled
    //   • ImDisk isn't installed (no point trying)
    //   • A disk is already mounted at the configured drive letter
    //     (user re-launched the app while the previous autostart disk
    //     is still around — don't error out trying to remount).
    const ramdiskAutostartTimer = setTimeout(async () => {
      const cfg = ramdiskAutostartRef.current;
      if (!cfg?.enabled) return;
      try {
        const test = await testRamDiskInstalled();
        const installed = (() => {
          if (!test?.success) return false;
          const d = test.data as { installed?: boolean } | boolean | undefined;
          if (typeof d === "boolean") return d;
          return Boolean(d?.installed);
        })();
        if (!installed) {
          console.log('[BackgroundPollers] RAM disk autostart skipped — ImDisk not installed');
          return;
        }
        const mountRequest = savedRamDiskMountRequest(cfg);
        const letter = mountRequest?.DriveLetter || 'R';
        // Don't double-mount if the drive letter is already occupied by ANY volume.
        let isOccupied = false;
        const avail = await getAvailableDriveLetters();
        if (avail.success && avail.data?.letters) {
          const isAvailable = avail.data.letters.some((l) => l.toUpperCase() === letter.toUpperCase());
          if (!isAvailable) {
            isOccupied = true;
          }
        } else {
          // Fallback to only checking existing RAM disks if the full drive list query fails
          const status = await getRamDiskStatus();
          const existing = (status?.data?.disks ?? []).some((d) => d.letter?.toUpperCase() === letter.toUpperCase());
          if (existing) {
            isOccupied = true;
          }
        }

        if (isOccupied) {
          console.log(`[BackgroundPollers] RAM disk autostart skipped — ${letter}: drive letter is already occupied`);
          return;
        }
        // Never turn a missing/corrupt saved size into a surprise 256 MB disk.
        // The user must explicitly save the size they chose in the RAM Disks
        // panel.  The backend still enforces the total-RAM minus 3 GB cap.
        if (!mountRequest) {
          showError(
            "RAM disk autostart needs a saved size. Open RAM Disks, choose the size, and save the startup spec.",
            undefined,
            { kind: "notification" },
          );
          return;
        }
        const { SizeMB: sizeMB } = mountRequest;
        const r = await createRamDisk(mountRequest);
        if (r?.success) {
          showSuccess(`RAM disk auto-started at ${letter}: (${sizeMB} MB).`);
        } else {
          // RAM-disk mount result → Notifications tab, not System Alerts.
          showError((r?.error as string | undefined) || `RAM disk autostart failed at ${letter}:.`, undefined, { kind: "notification" });
        }
      } catch (err) {
        console.warn('[BackgroundPollers] RAM disk autostart failed:', err);
      }
    }, 6000);

    // ── One-shot: Auto-start Privacy Shield if user enabled autostart ──
    // Mirrors the ramdisk-autostart pattern: ~6s delay, best-effort.
    // Edge guards (owner-requested):
    //   • Pro-only — auto-start is a paid feature; free users start it manually.
    //   • Skip when the camera is missing or the AI runtime (Python deps) isn't
    //     installed, instead of firing a doomed Python launch every boot on a
    //     server / camera-less / dependency-incomplete box. The pre-checks are
    //     the same probes the card uses (Get-PrivacyShieldStatus.cameraAvailable
    //     + getAIDependenciesStatus.installed).
    const privacyShieldAutostartTimer = setTimeout(async () => {
      if (!privacyShieldAutostartRef.current) return;
      if (!isModuleEnabled(modulesRef.current, 'privacyShield')) return;
      if (!hasPaidRef.current) return;
      try {
        const status = await executeBackendCommand<{ cameraAvailable?: boolean }>("Get-PrivacyShieldStatus");
        if (!status.success || status.data?.cameraAvailable !== true) return;
        const ai = await getAIDependenciesStatus();
        if (!ai.success || !ai.data?.installed) return;

        const shield = appSettingsRef.current?.ideal?.privacy?.privacyShield;
        const mode = resolvePrivacyShieldMode({ fleetManaged: false, localMode: shield?.notifyMode });
        const detectorTriggers = {
          gaze: shield?.gazeDetectionEnabled ?? true,
          faces: shield?.antiPeepingEnabled ?? true,
          device: shield?.cameraHunterEnabled ?? true,
        };
        const blurTriggers = privacyShieldBlurTriggers(mode, detectorTriggers);
        const r = await startPrivacyShield(0, detectorTriggers.gaze, detectorTriggers.faces, detectorTriggers.device, false, false, shield?.modelSize ?? "medium", shield?.confidenceThreshold ?? 0.5, shield?.blurOpacity ?? 200, shield?.wakeDelaySeconds ?? 150, shield?.deviceWakeMultiplier ?? 5, shield?.multiFaceWakeMultiplier ?? 5, shield?.detectionBufferFrames ?? 2, shield?.captureSpeed ?? 1, blurTriggers.gaze, blurTriggers.faces, blurTriggers.device);
        if (r.success) {
          await invoke("update_tray_shield_label", { running: true }).catch(() => {});
          showSuccess("Privacy Shield auto-started.");
        }
      } catch {
        // Best-effort — do not block startup
      }
    }, 6000);

    // ── REMOVED: All polling intervals ──────────────────────────────────
    // systemPollInterval (3s)               → now Rust-native via get_live_metrics
    // shieldInterval (60s)                  → event-driven (tray updated at start/stop)
    // broad productivity polling → quiet-mode-only maintenance above

    return () => {
      window.removeEventListener("apps-install-missing", handleAppsInstallMissing as EventListener);
      window.removeEventListener("apps-open-package-updates", handleOpenPackageUpdates);
      for (const p of [unlistenShred, unlistenTrayShield, unlistenFleetShieldDenied, unlistenPasteMonitor, unlistenDecoy,
        unlistenRansomware, unlistenCoercion,
        unlistenTriggerTest, unlistenRemoteAccess, unlistenScreenCapture, unlistenDriverProblem,
        unlistenWifiGuard, unlistenNetworkHoneypot]) {
        p.then(f => { try { f(); } catch { /* already torn down */ } });
      }
      clearInterval(productivityMaintenanceTimer);
      clearTimeout(ramdiskAutostartTimer);
      clearTimeout(driverScanTimer);
      clearTimeout(privacyShieldAutostartTimer);
      if (shredTimer.current) clearTimeout(shredTimer.current);
    };
  }, [startPrivacyShield, invokeProductivityEngineMaintenance, onShredRequest, onPanelChange, testRamDiskInstalled, getRamDiskStatus, createRamDisk, getAvailableDriveLetters, getAIDependenciesStatus]);

  // This component renders nothing — it's purely for side effects
  return null;
}
