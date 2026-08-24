import { Input } from "@/components/ui/input";
import { useCallback, useEffect, useMemo, useState } from "react";
import useBackend, { AppLicenseStatus } from "../hooks/useBackend";
import useInvestigatorInstall from "../hooks/useInvestigatorInstall";
import { showSuccess } from "../utils/toast";
import { fireLicenseCelebration } from "./shared/LicenseCelebrationListener";
import useProInstall from "../hooks/useProInstall";
import LicenseQuickStats from "./LicenseQuickStats";
import { licenseStateLabel } from "../utils/licensePresentation";

export default function LicenseQuickPanel() {
  const { getLicenseStatus, activateAppLicense, refreshAppLicense, clearAppLicenseCache, deactivateAppLicense } = useBackend();
  const [status, setStatus] = useState<AppLicenseStatus | null>(null);
  const [licenseKey, setLicenseKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const hasPaid = status?.valid === true && (status.features ?? []).includes("paid");
  // Pro install detection shares the module-level cache with the installer,
  // but it only performs local sidecar work after entitlement is known.
  const { isInstalled: proInstalled } = useProInstall({
    status: hasPaid,
    manifest: false,
    defender: false,
  });
  // Two-step confirm: false = idle, true = awaiting second click
  const [confirmPending, setConfirmPending] = useState(false);

  const daysRemaining = useMemo(() => {
    const expiry = status?.trial_active
      ? status.trial_expires_at
      : status?.service_expires_at ?? status?.entitlement_expires_at;
    if (!expiry) return null;
    const now = Math.floor(Date.now() / 1000);
    const diff = expiry - now;
    if (diff <= 0) return 0;
    return Math.max(1, Math.floor(diff / 86400));
  }, [status]);
  const hasInvestigator = status?.valid === true && (status.features ?? []).includes("advanced");
  const investigator = useInvestigatorInstall(hasInvestigator);

  const refreshStatus = useCallback(async (preferOnline: boolean) => {
    setLoading(true);
    setMessage(null);
    try {
      if (preferOnline) {
        try {
          const refreshed = await refreshAppLicense();
          setStatus(refreshed);
          if (refreshed.reason) {
            if (/refreshed/i.test(refreshed.reason)) {
              if (!refreshed.valid) {
                showSuccess(refreshed.reason);
              }
            } else {
              setMessage(refreshed.reason);
            }
          }
          return;
        } catch {
          // Fallback to cached status when offline.
        }
      }
      const current = await getLicenseStatus();
      setStatus(current);
      if (current.reason) {
        if (/refreshed/i.test(current.reason)) {
          if (!current.valid) {
            showSuccess(current.reason);
          }
        } else {
          setMessage(current.reason);
        }
      }
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [getLicenseStatus, refreshAppLicense]);

  const activate = async () => {
    const trimmed = licenseKey.trim();
    if (!trimmed) { setMessage("Enter key"); return; }
    setLoading(true);
    setMessage(null);
    try {
      const next = await activateAppLicense(trimmed);
      setStatus(next);
      if (next.reason) {
        if (/refreshed/i.test(next.reason)) {
          if (!next.valid) {
            showSuccess(next.reason);
          }
        } else {
          setMessage(next.reason);
        }
      } else {
        setMessage("Activated");
      }
      setLicenseKey("");
      // Fire the global celebration when activation actually succeeded.
      // Without this, activating from the sidebar quick panel was silent
      // — no confetti, no congrats toast, no banner.
      if (next.licensed && next.valid) {
        fireLicenseCelebration({
          message: "WinCommander Pro is now active!",
          toast: "WinCommander Pro is now active — all paid features unlocked. Enjoy!",
        });
      }
      window.dispatchEvent(new CustomEvent("license-updated"));
    } catch (err) {
      setMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    // The global LicenceGate owns online refresh scheduling. This sidebar
    // reads the signed local cache so mounting it cannot add an hourly request.
    void refreshStatus(false);
    const onLicenseUpdated = () => refreshStatus(false);
    window.addEventListener("license-updated", onLicenseUpdated as EventListener);
    return () => {
      window.removeEventListener("license-updated", onLicenseUpdated as EventListener);
    };
  }, [refreshStatus]);

  // isTrialActive covers both: a local free trial AND a server-activated trial key
  // (where the admin worker sets plan = "trial"). The key path uses the normal
  // license activation flow so trial_active stays false; we detect it by plan name.
  const isTrialActive = !!status?.trial_active ||
    (status?.licensed === true && status?.valid === true && status?.plan?.toLowerCase() === "trial");
  const isTrialExpired = !!status?.trial_expired;
  const statusText = !status
    ? "Checking..."
    : !status.configured
      ? "Unconfigured"
      : isTrialExpired
        ? "Trial Expired"
          : isTrialActive
            ? "Free Trial"
          : status.licensed && status.valid
            ? licenseStateLabel(status)
            : status.licensed
              ? "Expired"
              : "Not Active";
  const isActive = !!status?.configured && !!status?.licensed && !!status?.valid && !isTrialExpired;

  // First click → show "Confirm?", second click → execute deactivation
  const handleDeactivateClick = async () => {
    if (!confirmPending) {
      setConfirmPending(true);
      return;
    }
    setConfirmPending(false);
    setLoading(true);
    setMessage(null);
    try {
      const wasTrial = status?.trial_active === true;
      await deactivateAppLicense();
      setStatus(null);
      setMessage(wasTrial
        ? "Trial ended on this device. Trial eligibility remains recorded by the server."
        : "License deactivated. Device released.");
      // Attempt local fallback already done inside deactivateAppLicense on the Rust side.
      window.dispatchEvent(new CustomEvent("license-updated"));
      await refreshStatus(false);
    } catch {
      try {
        await clearAppLicenseCache();
        setStatus(null);
        setMessage("License removed from this device.");
      } catch (err2) {
        setMessage(err2 instanceof Error ? err2.message : String(err2));
      }
    } finally {
      setLoading(false);
    }
  };

  const installOrLaunchInvestigator = async () => {
    setLoading(true);
    setMessage(null);
    try {
      await investigator.launchOrInstall();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  };

  // Keep licensing controls opt-in. A fresh Free install must open directly
  // to the dashboard, not an activation prompt; the panel expands when its
  // header is intentionally selected.
  const initialExpanded = status
    ? isActive && daysRemaining !== null && daysRemaining <= 14
    : false;
  const [isExpanded, setIsExpanded] = useState(initialExpanded);
  const [hasAutoSet, setHasAutoSet] = useState(false);

  useEffect(() => {
    if (status && !hasAutoSet) {
        setIsExpanded(isActive && daysRemaining !== null && daysRemaining <= 14);
        setHasAutoSet(true);
    }
  }, [status, isActive, daysRemaining, hasAutoSet]);

  const shouldCollapse = !isExpanded;

  return (
    <div className="license-quick-panel">
      <div className="license-row" onClick={() => setIsExpanded(!isExpanded)} style={{ marginBottom: shouldCollapse ? 0 : 6, cursor: 'pointer' }}>
        <span className="license-title">License</span>
        <span className={`license-pill ${isTrialExpired ? "warn" : isTrialActive ? "trial" : status?.licensed && status?.valid ? "ok" : ""}`}>{statusText}</span>
      </div>

      {!shouldCollapse && (
        <>
          {/* Stat row — hide for trial users (no seats, no days countdown). */}
          {isActive && !isTrialActive && (
            <LicenseQuickStats
              plan={status?.plan}
              daysRemaining={daysRemaining}
              seatsUsed={status?.seats_used}
              seatLimit={status?.seat_limit}
            />
          )}
          {isTrialExpired && (
            <div className="license-btn-row" style={{ marginTop: 0 }}>
              <button
                className="license-btn"
                onClick={() =>
                  window.dispatchEvent(
                    new CustomEvent("license-gate-open", { detail: { tab: "buy" } })
                  )
                }
              >
                Upgrade
              </button>
              <button
                className="license-btn ghost"
                onClick={() =>
                  window.dispatchEvent(
                    new CustomEvent("license-gate-open", { detail: { tab: "activate" } })
                  )
                }
              >
                Have a Key
              </button>
            </div>
          )}

          {isTrialActive && (
            <div className="license-btn-row" style={{ marginTop: 0 }}>
              <button
                className="license-btn"
                onClick={() =>
                  window.dispatchEvent(
                    new CustomEvent("license-gate-open", { detail: { tab: "buy" } })
                  )
                }
              >
                Upgrade
              </button>
              <button
                className={`license-btn danger${confirmPending ? ' confirm-pending' : ''}`}
                disabled={loading}
                onClick={handleDeactivateClick}
                onBlur={() => setConfirmPending(false)}
              >
                {confirmPending ? "Confirm?" : "Deactivate"}
              </button>
            </div>
          )}

          {!isActive && !isTrialExpired && !isTrialActive && (
            <div className="license-btn-row" style={{ marginTop: 0 }}>
              <button
                className="license-btn"
                onClick={() =>
                  window.dispatchEvent(
                    new CustomEvent("license-gate-open", { detail: { tab: "buy" } })
                  )
                }
              >
                Get License
              </button>
              <button
                className="license-btn ghost"
                onClick={() =>
                  window.dispatchEvent(
                    new CustomEvent("license-gate-open", { detail: { tab: "activate" } })
                  )
                }
              >
                Have a Key
              </button>
            </div>
          )}

          {isActive && !isTrialActive && (
            <div className="license-btn-row">
              {hasInvestigator && (
                <button
                  className="license-btn"
                  title="Open Investigator"
                  disabled={loading || investigator.isBusy}
                  onClick={() => void installOrLaunchInvestigator()}
                >
                  Investigator
                </button>
              )}
              {/* "Install Pro" only when the licence is active but the
                  Pro sidecar EXE isn't on disk yet -- e.g. user activated
                  on a fresh machine and skipped the auto-prompt. Fires
                  the same `pro-install-open` event LicenseGate uses. */}
              {!proInstalled && (
                <button
                  className="license-btn"
                  onClick={() => window.dispatchEvent(new CustomEvent("pro-install-open"))}
                >
                  Install Pro
                </button>
              )}
              <button className="license-btn ghost" disabled={loading} onClick={() => refreshStatus(true)}>
                Refresh
              </button>
              <button
                className={`license-btn danger${confirmPending ? ' confirm-pending' : ''}`}
                disabled={loading}
                onClick={handleDeactivateClick}
                onBlur={() => setConfirmPending(false)}
              >
                {confirmPending ? "Confirm?" : "Deactivate"}
              </button>
            </div>
          )}

          {/* Hidden field kept for the legacy in-line activate flow; the
              two CTAs above now route through the global LicenseGate
              dialog so only one purchase surface exists. The state
              still wires the activate fn for any future inline use. */}
          {false && (
            <div className="license-input-row" aria-hidden>
              <Input
                placeholder="XXXX-XXXX-XXXX"
                value={licenseKey}
                onChange={(e) => setLicenseKey(e.target.value)}
                disabled={loading}
                className="license-input"
              />
              <button className="license-btn" disabled={loading} onClick={activate}>
                {loading ? "..." : "Activate"}
              </button>
            </div>
          )}

          {message && !/refreshed/i.test(message) && <div className="license-msg">{message}</div>}
        </>
      )}
    </div>
  );
}
