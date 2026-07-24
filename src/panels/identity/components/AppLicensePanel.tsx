import { Button, FormGroup, InputGroup, Tag } from "@/components/ui/bp";
import { useCallback, useEffect, useMemo, useState } from "react";
import useBackend, { AppLicenseStatus } from "../../../hooks/useBackend";
import UniversalCallout from "../../../components/shared/UniversalCallout";
import { fireLicenseCelebration } from "../../../components/shared/LicenseCelebrationListener";
import {
  activeLicenseServices,
  licenseAccessSummary,
  licenseDeviceSummary,
  licensePlanLabel,
  licenseStateLabel,
} from "../../../utils/licensePresentation";
import './AppLicensePanel.css';


function formatUnixTime(unix?: number | null) {
  if (!unix) return "N/A";
  return new Date(unix * 1000).toLocaleString();
}

interface AppLicensePanelProps {
  onStatusLoaded?: (status: AppLicenseStatus) => void;
}

export default function AppLicensePanel({ onStatusLoaded }: AppLicensePanelProps) {
  const {
    getLicenseStatus,
    activateAppLicense,
    refreshAppLicense,
    deactivateAppLicense,
  } = useBackend();

  const [licenseKey, setLicenseKey] = useState("");
  const [status, setStatus] = useState<AppLicenseStatus | null>(null);
  const [loading, setLoading] = useState<Record<string, boolean>>({});
  const [feedback, setFeedback] = useState<{ intent: "success" | "danger" | "warning"; text: string } | null>(null);
  // Two-step confirm: null = idle, 'deactivate' = awaiting 2nd click
  const [confirmPending, setConfirmPending] = useState<'deactivate' | null>(null);

  const refreshStatus = useCallback(async () => {
    setLoading((prev) => ({ ...prev, status: true }));
    try {
      const nextStatus = await getLicenseStatus();
      setStatus(nextStatus);
      onStatusLoaded?.(nextStatus);
      if (nextStatus.reason && !nextStatus.valid) {
        setFeedback({ intent: "warning", text: nextStatus.reason });
      }
    } catch (err) {
      setFeedback({ intent: "danger", text: err instanceof Error ? err.message : String(err) });
    } finally {
      setLoading((prev) => ({ ...prev, status: false }));
    }
  }, [getLicenseStatus, onStatusLoaded]);

  useEffect(() => { refreshStatus(); }, [refreshStatus]);

  const runWithLoading = async (key: string, action: () => Promise<void>) => {
    setLoading((prev) => ({ ...prev, [key]: true }));
    setFeedback(null);
    try {
      await action();
    } catch (err) {
      setFeedback({ intent: "danger", text: err instanceof Error ? err.message : String(err) });
    } finally {
      setLoading((prev) => ({ ...prev, [key]: false }));
    }
  };

  const activate = async () => {
    await runWithLoading("activate", async () => {
      const key = licenseKey.trim();
      if (!key) { setFeedback({ intent: "warning", text: "Enter a license key first." }); return; }
      const nextStatus = await activateAppLicense(key);
      setStatus(nextStatus);
      setLicenseKey("");
      setFeedback({ intent: "success", text: "License activated successfully." });
      // Dispatch the global celebration — single source of truth for the
      // confetti+toast, mounted at app root. Works from any activation path.
      fireLicenseCelebration({
        message: "WinCommander Pro is now active!",
        toast: "WinCommander Pro is now active — all paid features unlocked. Enjoy!",
      });
      window.dispatchEvent(new CustomEvent("license-updated"));
      onStatusLoaded?.(nextStatus);
    });
  };

  const refresh = async () => {
    await runWithLoading("refresh", async () => {
      let nextStatus: AppLicenseStatus;
      try {
        nextStatus = await refreshAppLicense();
        if (!nextStatus.valid) {
          setFeedback({ intent: "success", text: "License refreshed from server." });
        }
      } catch (err) {
        nextStatus = await getLicenseStatus();
        const msg = err instanceof Error ? err.message : String(err);
        setFeedback({ intent: "warning", text: `${msg} — showing cached status.` });
      }
      setStatus(nextStatus);
      onStatusLoaded?.(nextStatus);
      window.dispatchEvent(new CustomEvent("license-updated"));
    });
  };

  // Two-step deactivation: first click → show "Confirm?", second click → execute
  const handleDeactivateClick = () => {
    if (confirmPending !== 'deactivate') {
      setConfirmPending('deactivate');
      return;
    }
    setConfirmPending(null);
    const isTrial = status?.trial_active === true;
    runWithLoading("deactivate", async () => {
      await deactivateAppLicense();
      setStatus(null);
      setFeedback({
        intent: "warning",
        text: isTrial
          ? "Trial ended on this device. Trial eligibility remains recorded by the server."
          : "License deactivated. Device released on the server.",
      });
      window.dispatchEvent(new CustomEvent("license-updated"));
    });
  };

  const statusText = useMemo(() => {
    return licenseStateLabel(status);
  }, [status]);

  // Can deactivate if there's an active paid license OR an active trial
  const canDeactivate = !!status?.configured &&
    (!!status?.trial_active || (!!status?.licensed && !!status?.valid));
  const accessSummary = licenseAccessSummary(status);
  const activeServices = activeLicenseServices(status);

  return (
    <div style={{ position: 'relative' }}>
      {feedback && <UniversalCallout message={feedback.text} intent={feedback.intent} className="mb-3" />}

      <div className="license-row-layout">
        {/* Left: key input + metadata */}
        <div className="license-row-left">
          <div className="flex justify-between items-center mb-2 px-1">
            <span className="text-[10px] font-bold text-accent tracking-tighter opacity-80 uppercase">License Status</span>
            <Tag minimal intent={status?.valid ? "success" : "warning"}>{statusText}</Tag>
          </div>
          <FormGroup label="License Key" className="compact-form" helperText="Format: WC-NAME-XXXX">
            <InputGroup
              value={licenseKey}
              onChange={(e) => setLicenseKey(e.target.value)}
              placeholder="WC-PRO-XXXX-XXXX-XXXX-XXXX-XXXX"
            />
          </FormGroup>
          <div className="license-meta">
            <div><strong>Plan:</strong> {licensePlanLabel(status?.plan)}</div>
            <div><strong>Product term:</strong> {status?.entitlement_expires_at ? formatUnixTime(status.entitlement_expires_at) : status?.plan === "pro_lifetime" ? "Lifetime" : "N/A"}</div>
            <div><strong>Service term:</strong> {formatUnixTime(status?.service_expires_at)}</div>
            <div><strong>Last Verified:</strong> {formatUnixTime(status?.last_verified_at)}</div>
            <div><strong>Offline Grace Until:</strong> {formatUnixTime(status?.grace_until)}</div>
            {status?.licensed && <div className="license-meta-wide"><strong>Devices:</strong> {licenseDeviceSummary(status)}</div>}
            {activeServices.length > 0 && <div className="license-meta-wide"><strong>Active services:</strong> {activeServices.join(", ")}</div>}
          </div>
          {accessSummary && <div className="license-access-summary">{accessSummary}</div>}
        </div>

        {/* Right: action buttons */}
        <div className="license-row-actions">
          <Button text="ACTIVATE" icon="key" onClick={activate} loading={!!loading.activate} className="compact-action-btn" />
          <Button text="REFRESH" icon="refresh" onClick={refresh} loading={!!loading.refresh} className="compact-action-btn secondary" />
          <Button text="CHECK" icon="diagnosis" onClick={refreshStatus} loading={!!loading.status} className="compact-action-btn secondary" />
          {canDeactivate && (
            <Button
              text={confirmPending === 'deactivate' ? "CONFIRM" : "REMOVE"}
              icon={confirmPending === 'deactivate' ? "warning-sign" : "log-out"}
              onClick={handleDeactivateClick}
              onBlur={() => setConfirmPending(null)}
              loading={!!loading.deactivate}
              className={`compact-action-btn danger${confirmPending === 'deactivate' ? ' confirm-pending' : ''}`}
              title={status?.trial_active
                ? "End your free trial on this device (trial cannot be restarted)"
                : "Release this device from the license and remove it from this PC"}
            />
          )}
        </div>
      </div>
    </div>
  );
}
