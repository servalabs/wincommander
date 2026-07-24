import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import useBackend from "../hooks/useBackend";
import { useAppState } from "../context/AppContext";
import { useInvalidateLicense, useLicenseQuery } from "../hooks/queries/useLicenseQuery";
import { fireLicenseCelebration } from "./shared/LicenseCelebrationListener";
import LicensePurchasePanel from "./LicensePurchasePanel";
import { DURATION_S, EASE } from "./shared/motion";
import "./LicenseGate.css";

const overlayVariants = {
  hidden: { opacity: 0 },
  visible: { opacity: 1 },
  exit: { opacity: 0 },
} as const;

const cardVariants = {
  hidden: { opacity: 0, scale: 0.96 },
  visible: { opacity: 1, scale: 1 },
  exit: { opacity: 0, scale: 0.96 },
} as const;

const gateTransition = { duration: DURATION_S.slow, ease: EASE.enter } as const;

interface LicenseGateProps {
  inline?: boolean;
  inlineFeatureLabel?: string;
  inlineDefaultTab?: "buy" | "activate";
  onInlineClose?: () => void;
  onInlineActivated?: () => void;
}

export default function LicenseGate({
  inline = false,
  inlineFeatureLabel,
  inlineDefaultTab,
  onInlineClose,
  onInlineActivated,
}: LicenseGateProps) {
  const [licenseKey, setLicenseKey] = useState("");
  const [licenseBusy, setLicenseBusy] = useState(false);
  const [licenseMessage, setLicenseMessage] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"buy" | "activate">(inlineDefaultTab ?? "buy");
  const [openOnDemand, setOpenOnDemand] = useState(inline);
  const [requestedFeatureLabel, setRequestedFeatureLabel] = useState<string | null>(
    inlineFeatureLabel ?? null
  );

  const { data: licenseStatus } = useLicenseQuery();
  const invalidateLicense = useInvalidateLicense();
  const { activateAppLicense, refreshAppLicense, startTrial } = useBackend();
  const {
    refreshAll,
    refreshDashboard,
    refreshNetwork,
    runAppInventoryScan,
    refreshMesh,
    refreshVault,
    refreshProductivity,
  } = useAppState();

  useEffect(() => {
    if (inline) return;
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<{
        tab?: "buy" | "activate";
        featureLabel?: string;
      }>).detail;
      if (detail?.tab) setActiveTab(detail.tab);
      setRequestedFeatureLabel(detail?.featureLabel ?? null);
      setOpenOnDemand(true);
    };
    window.addEventListener("license-gate-open", handler);
    return () => window.removeEventListener("license-gate-open", handler);
  }, [inline]);

  useEffect(() => {
    const refresh = async () => {
      try {
        await refreshAppLicense();
        invalidateLicense();
      } catch {
        // The signed local token remains authoritative through its offline grace.
      }
    };
    const onlineTimer = window.setTimeout(() => void refresh(), 1_500);
    const periodicTimer = window.setInterval(() => void refresh(), 12 * 60 * 60 * 1_000);
    return () => {
      window.clearTimeout(onlineTimer);
      window.clearInterval(periodicTimer);
    };
  }, [invalidateLicense, refreshAppLicense]);

  const refreshPaidData = useCallback(async () => {
    await refreshAll();
    await Promise.all([
      refreshDashboard(true),
      refreshNetwork(true),
      runAppInventoryScan(true),
      refreshMesh(true),
      refreshVault(true),
      refreshProductivity(true),
    ]);
  }, [
    refreshAll,
    refreshDashboard,
    refreshNetwork,
    runAppInventoryScan,
    refreshMesh,
    refreshVault,
    refreshProductivity,
  ]);

  const afterActivation = useCallback(async (message: string) => {
    invalidateLicense();
    window.dispatchEvent(new CustomEvent("license-updated"));
    fireLicenseCelebration({
      message,
      toast: "WinCommander Pro is active — paid features are unlocked.",
    });
    if (inline) onInlineActivated?.();
    else setOpenOnDemand(false);
    try {
      const proStatus = await invoke<{ installed: boolean }>("get_pro_install_status");
      if (!proStatus.installed) {
        window.setTimeout(
          () => window.dispatchEvent(new CustomEvent("pro-install-open")),
          400
        );
      }
    } catch {
      // Installation remains available from the sidebar.
    }
    await refreshPaidData();
  }, [inline, invalidateLicense, onInlineActivated, refreshPaidData]);

  const handleActivate = useCallback(async () => {
    if (!licenseKey.trim()) {
      setLicenseMessage("Enter a license key first.");
      return;
    }
    setLicenseBusy(true);
    setLicenseMessage(null);
    try {
      const result = await activateAppLicense(licenseKey.trim());
      if (!result.licensed || !result.valid) throw new Error(result.reason ?? "Activation failed.");
      setLicenseKey("");
      await afterActivation("WinCommander Pro is now active!");
    } catch (error) {
      setLicenseMessage(String(error));
    } finally {
      setLicenseBusy(false);
    }
  }, [activateAppLicense, afterActivation, licenseKey]);

  const handleStartTrial = useCallback(async () => {
    setLicenseBusy(true);
    setLicenseMessage(null);
    try {
      const result = await startTrial();
      if (!result.trial_active) throw new Error(result.reason ?? "Trial couldn't be started.");
      await afterActivation("16-day free trial activated!");
    } catch (error) {
      setLicenseMessage(String(error));
    } finally {
      setLicenseBusy(false);
    }
  }, [afterActivation, startTrial]);

  const closeModal = () => {
    if (inline) onInlineClose?.();
    else setOpenOnDemand(false);
    setLicenseMessage(null);
  };

  return (
    <AnimatePresence>
      {openOnDemand && (
        <motion.div
          className={`license-gate-overlay${inline ? " license-gate-overlay--inline" : ""}`}
          onClick={inline ? undefined : closeModal}
          variants={overlayVariants}
          initial="hidden"
          animate="visible"
          exit="exit"
          transition={gateTransition}
        >
          <motion.div
            className={`license-gate-card${inline ? " license-gate-card--inline" : ""}`}
            onClick={(event) => event.stopPropagation()}
            variants={cardVariants}
            initial="hidden"
            animate="visible"
            exit="exit"
            transition={gateTransition}
          >
            {!inline && (
              <button type="button" onClick={closeModal} aria-label="Close" className="license-gate-close">×</button>
            )}
            <div className="license-gate-header">
              <div className="license-gate-title">WINCOMMANDER · LICENSE</div>
              <div className="license-gate-subtitle">
                {requestedFeatureLabel
                  ? <><strong>{requestedFeatureLabel}</strong> requires WinCommander Pro.</>
                  : "Purchase securely or activate an existing key."}
              </div>
            </div>

            <div className="license-gate-tabs">
              <button className={`license-gate-tab ${activeTab === "buy" ? "active" : ""}`} onClick={() => setActiveTab("buy")}>Buy License</button>
              <button className={`license-gate-tab ${activeTab === "activate" ? "active" : ""}`} onClick={() => setActiveTab("activate")}>Enter Key</button>
            </div>

            {activeTab === "buy" && (
              <>
                <LicensePurchasePanel
                  licenseStatus={licenseStatus ?? null}
                  onActivated={() => void afterActivation("WinCommander Pro is now active!")}
                  onStartTrial={() => void handleStartTrial()}
                  isLicenseBusy={licenseBusy}
                />
                {licenseMessage && <div className="license-gate-message">{licenseMessage}</div>}
              </>
            )}

            {activeTab === "activate" && (
              <div className="license-gate-activate">
                <div className="license-gate-field">
                  <label className="license-gate-label">License Key</label>
                  <input
                    type="text"
                    className="license-gate-input"
                    placeholder="WC-PRO-XXXX-XXXX-XXXX-XXXX-XXXX"
                    value={licenseKey}
                    onChange={(event) => setLicenseKey(event.target.value)}
                    onKeyDown={(event) => event.key === "Enter" && void handleActivate()}
                    disabled={licenseBusy}
                    autoFocus
                  />
                </div>
                {licenseMessage && <div className="license-gate-message">{licenseMessage}</div>}
                <div className="license-gate-buttons">
                  <button className="license-gate-btn-primary" disabled={licenseBusy || !licenseKey.trim()} onClick={() => void handleActivate()}>
                    {licenseBusy ? "Activating…" : "Activate"}
                  </button>
                  <button
                    className="license-gate-btn-retry"
                    disabled={licenseBusy}
                    onClick={async () => {
                      try {
                        await refreshAppLicense();
                        invalidateLicense();
                      } catch (error) {
                        setLicenseMessage(String(error));
                      }
                    }}
                  >
                    Refresh
                  </button>
                </div>
              </div>
            )}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
