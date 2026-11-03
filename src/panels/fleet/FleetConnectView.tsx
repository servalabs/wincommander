// Device enrollment card — connects THIS machine to a self-hosted Fleet server.
// Independent of the admin login card (LoginView): admin auth is for managing
// the fleet; this card is for enrolling this device as an agent.

import { useEffect, useRef, useState } from "react";
import { FleetServiceContact } from "./FleetServiceContact";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Icon } from "@/components/ui/icon";
import { getSettingsOnce } from "@/hooks/useSettings";
import { useLicenseQuery } from "@/hooks/queries/useLicenseQuery";
import { fleetLinkLabel, fleetLinkState, type FleetLinkStatus } from "./fleetLinkPresentation";
import { fleetShieldProfileStatus } from "../../lib/fleetShieldProfileStatus";
import { FleetSignedContact } from "./FleetSignedContact";

interface FleetStatus extends FleetLinkStatus {
  // True while Pro's check-in loop is alive and self-healing (connected or
  // mid a transient-failure retry); false once it has permanently stopped
  // (never started, disconnected, or a terminal rejection — e.g. this device
  // was removed/unenrolled server-side and needs a fresh enroll).
  retrying: boolean;
  // True while the enrollment is accepted by the server but still AWAITING
  // ADMIN APPROVAL — the device checks in (so `connected` is true) but gets no
  // config/commands until an admin approves it in the fleet console. The card
  // shows "Request submitted — waiting for admin approval" instead of
  // "Enrolled" while this holds.
  pendingApproval?: boolean;
}

// Steady-state poll while enrolled (shows live connection health).
const STEADY_POLL_MS = 4_000;
// Aggressive poll run after clicking "Connect" — catches async enrollment.
const ENROLL_POLL_MS = 1_000;
// Return to steady polling after this interval; live agent status still owns retry state.
const ENROLL_TIMEOUT_MS = 75_000;
// Refresh cadence while a managed-device unenroll request is awaiting admin approval.
const UNENROLL_POLL_MS = 5_000;

interface UnenrollInfo {
  status: string;
  approvals: number;
  required: number;
}

// Result shape shared by the initial request and status refreshes.
type UnenrollResult = {
  status: string;
  approvals: number;
  required_approvals: number;
  approved?: boolean;
};

function isUnenrollApproved(result: UnenrollResult) {
  return result.approved === true && result.status === "approved";
}

function pendingUnenroll(result: UnenrollResult): UnenrollInfo | null {
  if (result.status !== "pending" || (result.approved !== undefined && result.approved !== false)
      || !Number.isSafeInteger(result.approvals) || result.approvals < 0
      || !Number.isSafeInteger(result.required_approvals) || result.required_approvals < 0) return null;
  return { status: result.status, approvals: result.approvals, required: result.required_approvals };
}

const UNENROLL_UNCONFIRMED = "Could not verify Fleet removal status. Check the connection or contact an administrator, then retry.";

export default function FleetConnectView() {
  const { data: license, isLoading: licenseLoading } = useLicenseQuery();
  // Status polling is read-only. The device-owned agent status is stored in
  // the shared service context, so it must not be hidden merely because this
  // Windows user's settings do not contain app.fleet.
  const fleetEntitled = license?.valid === true &&
    (license.active_service_features ?? license.features ?? []).includes("fleet");
  const [serverUrl, setServerUrl] = useState("");
  const [dispatch, setDispatch] = useState(false);
  const [signingKeyPub, setSigningKeyPub] = useState("");
  const [status, setStatus] = useState<FleetStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [repairingTrust, setRepairingTrust] = useState(false);
  const [machineTrustBound, setMachineTrustBound] = useState<boolean | null>(null);
  const [shieldPolicyStatus, setShieldPolicyStatus] = useState<"verified" | "effective_config_unavailable" | "unverified" | null>(null);
  // enrolling: true between a successful connect() call and first connected=true poll.
  const [enrolling, setEnrolling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  // wasEnrolled: app.fleet.enabled from persisted settings — "this device is
  // supposed to be enrolled" independent of the live Pro status. Pro is a
  // short-lived sidecar that can restart (crash, manual kill, app relaunch);
  // on a fresh process the fleet agent re-enrolls itself in the background
  // but connected briefly reads false. Without this signal that window is
  // indistinguishable from "never enrolled" and the UI would wrongly fall
  // back to the blank connect form even though this device is enrolled.
  const [wasEnrolled, setWasEnrolled] = useState(false);
  const [persistedDeviceId, setPersistedDeviceId] = useState("");
  // statusChecked: true once the first fleet_status poll of THIS mount has
  // settled. Gating render on this (alongside `loaded`) closes a remount race:
  // every panel switch tears down and recreates this component (App.tsx keys
  // the panel host by activePanel), so `status` briefly resets to null while
  // `wasEnrolled` — loaded from settings, usually the faster of the two reads —
  // is already true. Without this gate, isReconnecting would read
  // (wasEnrolled=true, isConnected=false) for that one tick and flash
  // "Reconnecting…" on every panel switch even though the device was never
  // disconnected — just re-mounted and waiting on its first status reply.
  const [statusChecked, setStatusChecked] = useState(false);
  // Non-null while an unenroll request is outstanding (submitted, awaiting
  // the required admin approval(s) from the fleet console).
  const [unenrollInfo, setUnenrollInfo] = useState<UnenrollInfo | null>(null);

  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const enrollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const unenrollPollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const unenrollEpochRef = useRef(0);

  // Prefill form from persisted settings on mount.
  useEffect(() => {
    getSettingsOnce().then((s) => {
      const f = s.app?.fleet;
      if (f) {
        setServerUrl(f.serverUrl ?? "");
        setDispatch(f.dispatch ?? false);
        setSigningKeyPub(f.signingKeyPub ?? "");
        setWasEnrolled(f.enabled === true);
      }
      setPersistedDeviceId(s.deviceId ?? "");
      setLoaded(true);
    }).catch(() => setLoaded(true));
  }, []);

  function stopPoll() {
    if (pollRef.current !== null) { clearInterval(pollRef.current); pollRef.current = null; }
    if (enrollTimeoutRef.current !== null) { clearTimeout(enrollTimeoutRef.current); enrollTimeoutRef.current = null; }
  }

  function stopUnenrollPoll() {
    unenrollEpochRef.current += 1;
    if (unenrollPollRef.current !== null) { clearInterval(unenrollPollRef.current); unenrollPollRef.current = null; }
  }

  function doPollStatus() {
    invoke<FleetStatus>("fleet_status")
      .then((s) => {
        setStatus(s);
        if (s.connected) {
          // Connected — drop to steady-state rate. Also latch wasEnrolled so
          // a later mid-session Pro respawn (which briefly reports
          // connected=false while it re-enrolls in the background) is
          // recognized as "reconnecting", not misread as "never enrolled".
          setWasEnrolled(true);
          setEnrolling(false);
          stopPoll();
          pollRef.current = setInterval(doPollStatus, STEADY_POLL_MS);
        }
      })
      .catch((reason) => {
        // A local Pro failure is distinct from an offline Fleet server. Do not
        // leave the enrollment view indefinitely on "Connecting" when no
        // local helper is alive to complete the request.
        const message = typeof reason === "string" ? reason : "Local Pro service did not respond.";
        setError(`Local WinCommander service problem: ${message}`);
        setEnrolling(false);
      })
      .finally(() => setStatusChecked(true));
  }

  function startSteadyPoll() {
    stopPoll();
    doPollStatus();
    pollRef.current = setInterval(doPollStatus, STEADY_POLL_MS);
  }

  function startEnrollPoll() {
    stopPoll();
    setEnrolling(true);
    doPollStatus();
    pollRef.current = setInterval(doPollStatus, ENROLL_POLL_MS);
    // After ENROLL_TIMEOUT_MS, drop back to steady rate regardless.
    enrollTimeoutRef.current = setTimeout(() => {
      setEnrolling(false);
      stopPoll();
      pollRef.current = setInterval(doPollStatus, STEADY_POLL_MS);
    }, ENROLL_TIMEOUT_MS);
  }

  // Steady-state poll on mount — shows status if already enrolled.
  useEffect(() => {
    if (licenseLoading) return;
    startSteadyPoll();
    return () => { stopPoll(); stopUnenrollPoll(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fleetEntitled, licenseLoading]);

  useEffect(() => {
    if (!fleetEntitled || !wasEnrolled) return;
    let cancelled = false;
    let requestSequence = 0;
    const refresh = () => {
      const sequence = ++requestSequence;
      void invoke<{ trustBound: unknown; shieldPolicyStatus?: unknown }>("fleet_machine_trust_status")
        .then((result) => {
          if (cancelled || sequence !== requestSequence) return;
          if (typeof result?.trustBound !== "boolean") throw new Error("Malformed Fleet trust status");
          setMachineTrustBound(result.trustBound);
          setShieldPolicyStatus(fleetShieldProfileStatus(result));
        })
        .catch(() => { if (!cancelled && sequence === requestSequence) { setMachineTrustBound(null); setShieldPolicyStatus("unverified"); } });
    };
    refresh();
    const timer = setInterval(refresh, 15_000);
    return () => { cancelled = true; clearInterval(timer); };
  }, [fleetEntitled, wasEnrolled]);

  async function repairMachineTrust() {
    setError(null);
    setBusy(true);
    setRepairingTrust(true);
    try {
      await invoke("fleet_repair_machine_trust");
      setMachineTrustBound(true);
      setShieldPolicyStatus("unverified");
      startSteadyPoll();
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "Machine trust repair failed.");
    } finally {
      setRepairingTrust(false);
      setBusy(false);
    }
  }

  async function connect() {
    // The Connect button is an explicit request for this paid service. Open the
    // paywall here (once per click) rather than allowing background status
    // polling to repeatedly open it for users without Fleet access.
    if (!fleetEntitled) {
      window.dispatchEvent(new CustomEvent("license-gate-open", { detail: { tab: "buy" } }));
      setError("An active Fleet subscription is required for fleet enrollment.");
      return;
    }
    setError(null);
    setBusy(true);
    try {
      // fleet_connect: persists config to settings, then sends fleet_agent_configure
      // to the Pro sidecar. Pro configures the fleet agent and returns immediately;
      // the actual HTTP enrollment to the fleet server happens asynchronously inside Pro.
      // We ignore the return value shape and instead poll fleet_status aggressively.
      await invoke("fleet_connect", {
        serverUrl: serverUrl.trim(),
        dispatch,
        signingKeyPub: signingKeyPub.trim(),
      });
      startEnrollPoll();
    } catch (err) {
      const msg = typeof err === "string" ? err : "Connection failed.";
      if (msg.startsWith("PRO_NOT_INSTALLED:")) {
        setError("Pro sidecar is not installed. Install Pro from Settings first.");
      } else if (msg.includes("binary hash") || msg.includes("hash mismatch")) {
        setError("Pro binary hash mismatch — reinstall Pro from Settings to fix.");
      } else if (msg.includes("entitlement required") || msg.includes("license")) {
        setError("An active Fleet subscription is required for fleet enrollment.");
      } else {
        setError(msg);
      }
    } finally {
      setBusy(false);
    }
  }

  async function finalizeApprovedUnenroll() {
    stopUnenrollPoll();
    const epoch = unenrollEpochRef.current;
    let serviceStopped = false;
    try {
      const result = await invoke<{ ok?: boolean; serviceReachabilityStopped?: boolean }>("fleet_disconnect");
      if (epoch !== unenrollEpochRef.current) return;
      if (result?.ok !== true) throw new Error(UNENROLL_UNCONFIRMED);
      serviceStopped = result?.serviceReachabilityStopped === true;
    } catch {
      if (epoch !== unenrollEpochRef.current) return;
      setUnenrollInfo(null);
      setError(UNENROLL_UNCONFIRMED);
      return;
    }
    setStatus(null);
    setWasEnrolled(false);
    setUnenrollInfo(null);
    if (!serviceStopped) setError("The device was removed from Fleet, but stopping local service reachability could not be confirmed. An administrator can stop the WinCommander service.");
  }

  // Submits (or, if one already exists, refreshes) the managed-device
  // unenroll request, then polls until an admin approves it — instead of
  // tearing the agent down immediately, which the server refuses for a
  // managed device anyway (see fleet_agent.rs::fleet_disconnect).
  async function requestOrPollUnenroll() {
    const epoch = ++unenrollEpochRef.current;
    let res: UnenrollResult;
    try { res = await invoke<UnenrollResult>("fleet_request_unenroll"); }
    catch (error) { if (epoch !== unenrollEpochRef.current) return; throw error; }
    if (epoch !== unenrollEpochRef.current) return;
    if (isUnenrollApproved(res)) {
      await finalizeApprovedUnenroll();
      return;
    }
    const pending = pendingUnenroll(res);
    if (!pending) throw new Error(UNENROLL_UNCONFIRMED);
    setUnenrollInfo(pending);
    if (unenrollPollRef.current !== null) return; // already polling
    let checking = false;
    let consecutiveFailures = 0;
    unenrollPollRef.current = setInterval(async () => {
      if (checking) return;
      checking = true;
      try {
        const r = await invoke<UnenrollResult>("fleet_unenroll_status");
        if (epoch !== unenrollEpochRef.current) return;
        if (consecutiveFailures > 0) setError(null);
        consecutiveFailures = 0;
        if (isUnenrollApproved(r)) { await finalizeApprovedUnenroll(); return; }
        const pending = pendingUnenroll(r);
        if (!pending) throw new Error(UNENROLL_UNCONFIRMED);
        setUnenrollInfo(pending);
      } catch {
        if (epoch !== unenrollEpochRef.current) return;
        setUnenrollInfo(null);
        setError(UNENROLL_UNCONFIRMED);
        consecutiveFailures += 1;
        if (consecutiveFailures >= 3) stopUnenrollPoll();
      } finally { checking = false; }
    }, UNENROLL_POLL_MS);
  }

  async function disconnect() {
    setError(null);
    setBusy(true);
    stopPoll();
    stopUnenrollPoll();
    setEnrolling(false);
    try {
      // Every device must have its departure approved by an admin — see
      // fleet_agent.rs::fleet_disconnect. This files (or refreshes) the
      // unenroll request and starts polling for approval.
      await requestOrPollUnenroll();
    } catch (err) {
      setUnenrollInfo(null);
      const message = typeof err === "string" ? err : err instanceof Error ? err.message : "";
      setError(!message || /HTTP\s+401\b/i.test(message) ? UNENROLL_UNCONFIRMED : message);
    } finally {
      setBusy(false);
      // Resume steady poll so status reflects reality after disconnect.
      startSteadyPoll();
    }
  }

  // Purpose-built loading state: mirrors the real card's shape (header +
  // badge + two meta rows) so nothing jumps when the first poll settles,
  // instead of the generic cross-panel PanelSkeleton.
  if (!loaded || !statusChecked) {
    return (
      <div className="fleet-connect-card" aria-busy="true" aria-label="Loading device enrollment status">
        <div className="fleet-connect-header">
          <div className="fleet-connect-title-row">
            <Icon icon="offline" size={16} className="fleet-connect-icon" />
            <h3 className="fleet-connect-title">Device Enrollment</h3>
          </div>
          <span className="fleet-connect-badge fleet-connect-badge--pending">
            <span className="fleet-dot is-pending" /> Connecting…
          </span>
        </div>
        <div className="fleet-connect-status">
          <dl className="fleet-connect-meta" aria-hidden="true">
            <div className="fleet-meta">
              <dt className="fleet-shimmer fleet-shimmer-label" />
              <dd className="fleet-shimmer fleet-shimmer-value" />
            </div>
            <div className="fleet-meta">
              <dt className="fleet-shimmer fleet-shimmer-label" />
              <dd className="fleet-shimmer fleet-shimmer-value" />
            </div>
          </dl>
        </div>
      </div>
    );
  }

  const linkState = fleetLinkState(status);
  const isConnected = linkState === "linked" || linkState === "pending";
  // Surface any Pro-agent error whether connected or not.
  const agentError = status?.lastError ?? null;
  const isRetrying = status?.retrying ?? false;
  // Enrolled + checking in, but the fleet admin hasn't approved this device
  // yet — the server withholds all policy/commands until they do. Distinct
  // from `unenrollInfo` (a LEAVE request awaiting approval); this is a JOIN
  // request awaiting approval.
  const isAwaitingApproval = linkState === "pending";
  // This device is persisted as enrolled but the live poll hasn't reported
  // connected=true — Pro (a short-lived sidecar) most likely restarted and is
  // re-enrolling itself in the background (env-seeded auto-resume), OR the
  // check-in loop is mid a transient-failure retry (network blip, server
  // 5xx). Distinct from `enrolling`, which tracks a fresh user-initiated
  // Connect click. Requires `isRetrying` — Pro's loop must actually be alive
  // and self-healing, otherwise this would also (wrongly) read true for a
  // device permanently removed from the fleet, see `isRemoved` below.
  const isReconnecting = linkState === "reconnecting" || (wasEnrolled && !isConnected && !enrolling && isRetrying);
  // Terminal: this device WAS enrolled but Pro's check-in loop has
  // permanently stopped (retrying=false) with an error — removed/unenrolled
  // server-side (401/403) or a permanent enroll failure. Unlike
  // `isReconnecting`, nothing is happening in the background anymore; the
  // "stays enrolled while it retries" copy would be false here, so this
  // falls through to the connect form (prefilled) with the real reason shown.
  const isRemoved = linkState === "error" || (wasEnrolled && !isConnected && !enrolling && !isRetrying && !!agentError);
  const isOffline = linkState === "offline";
  // A terminal agent error (for example, a server-side credential revocation)
  // still has a useful historical device identity, but it is no longer a live
  // Fleet link. Treat it as re-enrollable so the prefilled, confirmed Connect
  // flow remains available instead of trapping the user on a read-only error.
  const hasAuthoritativeDeviceLink = linkState !== "not_linked" && !isRemoved;

  return (
    <div className="fleet-connect-card">
      <div className="fleet-connect-header">
        <div className="fleet-connect-title-row">
          <Icon icon="offline" size={16} className="fleet-connect-icon" />
          <h3 className="fleet-connect-title">Device Enrollment</h3>
        </div>
        {isConnected && unenrollInfo ? (
          <span className="fleet-connect-badge fleet-connect-badge--pending">
            <span className="fleet-dot is-pending" /> Awaiting approval
          </span>
        ) : isAwaitingApproval ? (
          <span className="fleet-connect-badge fleet-connect-badge--pending">
            <span className="fleet-dot is-pending" /> Awaiting approval
          </span>
        ) : isConnected ? (
          <span className="fleet-connect-badge fleet-connect-badge--ok">
            <span className="fleet-dot is-online" /> {fleetLinkLabel(linkState)}
          </span>
        ) : enrolling ? (
          <span className="fleet-connect-badge fleet-connect-badge--pending">
            <span className="fleet-dot is-pending" /> Enrolling…
          </span>
        ) : isReconnecting ? (
          <span className="fleet-connect-badge fleet-connect-badge--pending">
            <span className="fleet-dot is-pending" /> Reconnecting…
          </span>
        ) : isRemoved ? (
          <span className="fleet-connect-badge fleet-connect-badge--off">
            <span className="fleet-dot is-offline" /> {fleetLinkLabel(linkState)}
          </span>
        ) : isOffline ? (
          <span className="fleet-connect-badge fleet-connect-badge--off">
            <span className="fleet-dot is-offline" /> {fleetLinkLabel(linkState)}
          </span>
        ) : (
          <span className="fleet-connect-badge fleet-connect-badge--off">
            <span className="fleet-dot is-offline" /> Not enrolled
          </span>
        )}
      </div>

      {isConnected && status ? (
        <div className="fleet-connect-status">
          <dl className="fleet-connect-meta">
            <div className="fleet-meta">
              <dt>Device ID</dt>
              <dd className="mono">{status.deviceId || "—"}</dd>
            </div>
            <div className="fleet-meta">
              <dt>Server</dt>
              <dd className="mono">{status.serverUrl || "—"}</dd>
            </div>
            {status.lastEnrollAt && (
              <div className="fleet-meta">
                <dt>Enrolled</dt>
                <dd>{new Date(status.lastEnrollAt).toLocaleString()}</dd>
              </div>
            )}
          </dl>
          {isAwaitingApproval && !unenrollInfo && (
            <p className="fleet-connect-enrolling">
              <Icon icon="time" size={13} /> Request submitted — waiting for the fleet admin to
              approve this device. It stays here (no policy or commands are applied) until an
              admin approves it in the fleet console.
            </p>
          )}
          {unenrollInfo && (
            <p className="fleet-connect-enrolling">
              <Icon icon="time" size={13} /> Unenroll request submitted — waiting for admin
              approval ({unenrollInfo.approvals}/{unenrollInfo.required}). This device stays
              enrolled until it's approved.
            </p>
          )}
          {agentError && (
            <p className="fleet-connect-error" role="alert">
              <Icon icon="warning-sign" size={13} intent="danger" /> {agentError}
            </p>
          )}
          {error && (
            <p className="fleet-connect-error" role="alert">
              <Icon icon="warning-sign" size={13} intent="danger" /> {error}
            </p>
          )}
          {fleetEntitled ? (
            <>
              {wasEnrolled && shieldPolicyStatus === "effective_config_unavailable" && (
                <p className="fleet-connect-enrolling" role="alert">
                  The last verified Fleet policy has no applicable Shield configuration. Previous settings are kept, but Shield policy cannot be verified. A Fleet Stop may wait while Shield is running because ownership cannot be confirmed. Ask your Fleet administrator to review the device's policy assignment.
                </p>
              )}
              {wasEnrolled && shieldPolicyStatus === "unverified" && (
                <p className="fleet-connect-enrolling" role="alert">
                  This device's Shield policy cannot currently be verified. Check the Fleet service and policy assignment.
                </p>
              )}
              {machineTrustBound === false && wasEnrolled && status.deviceId === persistedDeviceId && (
                <div>
                  <p className="fleet-connect-enrolling">
                    This device is linked, but its local service has no machine trust record.
                    An administrator can repair it using the same device identity. This does not request to leave Fleet.
                  </p>
                  <Button variant="outline" size="sm" disabled={busy} onClick={repairMachineTrust}>
                    {repairingTrust ? "Repairing…" : "Repair machine trust"}
                  </Button>
                </div>
              )}
              <Button variant="outline" size="sm" disabled={busy} onClick={disconnect}>
                {busy && !repairingTrust
                  ? (unenrollInfo ? "Checking…" : "Requesting…")
                  : unenrollInfo ? "Refresh status" : "Request to leave"}
              </Button>
            </>
          ) : <p className="fleet-connect-enrolling">This is the device agent’s read-only link status.</p>}
        </div>
      ) : isReconnecting ? (
        <div className="fleet-connect-status">
          <dl className="fleet-connect-meta">
            <div className="fleet-meta">
              <dt>Device ID</dt>
              <dd className="mono">{status?.deviceId || persistedDeviceId || "—"}</dd>
            </div>
            <div className="fleet-meta">
              <dt>Server</dt>
              <dd className="mono">{status?.serverUrl || serverUrl || "—"}</dd>
            </div>
          </dl>
          <p className="fleet-connect-enrolling">
            <Icon icon="refresh" size={13} /> Retrying contact with the fleet server. A current connection is not yet confirmed.
          </p>
          {agentError && (
            <p className="fleet-connect-error" role="alert">
              <Icon icon="warning-sign" size={13} intent="danger" /> {agentError}
            </p>
          )}
          {fleetEntitled && <Button variant="outline" size="sm" disabled={busy} onClick={disconnect}>
            {busy ? "Requesting…" : "Request to leave"}
          </Button>}
        </div>
      ) : hasAuthoritativeDeviceLink && status ? (
        <div className="fleet-connect-status">
          <dl className="fleet-connect-meta">
            <div className="fleet-meta"><dt>Device ID</dt><dd className="mono">{status.deviceId || "—"}</dd></div>
            <div className="fleet-meta"><dt>Server</dt><dd className="mono">{status.serverUrl || "—"}</dd></div>
          </dl>
          <p className="fleet-connect-enrolling">
            <Icon icon="offline" size={13} /> This device is known to Fleet but is not currently checking in.
          </p>
          {agentError && <p className="fleet-connect-error" role="alert"><Icon icon="warning-sign" size={13} intent="danger" /> {agentError}</p>}
          {!fleetEntitled && <p className="fleet-connect-enrolling">This is the device agent’s read-only link status.</p>}
        </div>
      ) : (
        <div className="fleet-connect-form">
          <label className="fleet-field">
            <span className="fleet-field-label">Fleet server URL</span>
            <Input
              value={serverUrl}
              onChange={(e) => setServerUrl(e.target.value)}
              placeholder="http://fleet.corp.ts.net:8787"
              spellCheck={false}
              autoComplete="url"
              disabled={enrolling}
            />
          </label>

          <label className="fleet-field">
            <span className="fleet-field-label">Signing public key</span>
            <Input
              value={signingKeyPub}
              onChange={(e) => setSigningKeyPub(e.target.value)}
              placeholder="Base64 Ed25519 key (optional)"
              spellCheck={false}
              className="mono"
              disabled={enrolling}
            />
            <span className="fleet-field-hint">
              Pin the Fleet signing key to verify commands and signed contact. Signed contact also requires a separate operator key configured in Fleet.
            </span>
          </label>

          <div className="fleet-connect-toggle">
            <div>
              <span className="fleet-connect-toggle-label">Enable command dispatch</span>
              <p className="fleet-connect-toggle-sub">
                Poll for and execute verified commands from the fleet server.
                Requires a signing key.
              </p>
            </div>
            <Switch
              checked={dispatch}
              onCheckedChange={setDispatch}
              disabled={!signingKeyPub.trim() || enrolling}
              aria-label="Enable command dispatch"
            />
          </div>

          {/* Enrollment in-progress hint */}
          {enrolling && !error && (
            <p className="fleet-connect-enrolling">
              <Icon icon="refresh" size={13} /> Contacting fleet server — this can take up to 30 s on first enroll.
            </p>
          )}

          {/* Agent-level error from fleet_status poll (enrollment errors from Pro) */}
          {agentError && !enrolling && (
            <p className="fleet-connect-error" role="alert">
              <Icon icon="warning-sign" size={13} intent="danger" /> Last agent error (may be from an earlier attempt): {agentError}
            </p>
          )}

          {/* Error from the connect() call itself (spawn failure, hash, license) */}
          {error && (
            <p className="fleet-connect-error" role="alert">
              <Icon icon="warning-sign" size={13} intent="danger" /> {error}
            </p>
          )}

          <Button
            variant="primary"
            disabled={busy || enrolling || !serverUrl.trim()}
            onClick={connect}
          >
            {busy ? "Connecting…" : enrolling ? "Enrolling…" : "Connect to Fleet"}
          </Button>
        </div>
      )}
      {status?.deviceId && <><FleetSignedContact timestamp={status.lastSignedContactAt} /><FleetServiceContact deviceId={status.deviceId} /></>}
    </div>
  );
}
