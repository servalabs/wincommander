import { useCallback, useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import useBackend from "../hooks/useBackend";
import { useAppState } from "../context/AppContext";
import { useInvalidateLicense, useLicenseQuery } from "../hooks/queries/useLicenseQuery";
import { nextLicenseRefreshDelay } from "../lib/licenseRefreshSchedule";
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

/** Keeps the pill tabs in the gate's mono/uppercase register. */
const TAB_TRIGGER_CLASS =
  "flex-1 font-[family-name:var(--font-mono)] text-[11px] uppercase tracking-[1.5px]";

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
  const titleId = useId();
  const subtitleId = useId();
  const licenseKeyId = useId();
  const licenseKeyInputRef = useRef<HTMLInputElement>(null);

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
    if (inline || !openOnDemand) return;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setOpenOnDemand(false);
      setLicenseMessage(null);
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [inline, openOnDemand]);

  useEffect(() => {
    if (!openOnDemand || activeTab !== "activate") return;
    const frame = window.requestAnimationFrame(() => licenseKeyInputRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [activeTab, openOnDemand]);

  useEffect(() => {
    let cancelled = false;
    let failures = 0;
    let timer: ReturnType<typeof window.setTimeout> | null = null;
    const schedule = (delay: number) => {
      timer = window.setTimeout(() => { void refresh(); }, delay);
    };
    const refresh = async () => {
      try {
        await refreshAppLicense();
        invalidateLicense();
        failures = 0;
      } catch {
        // The signed local token remains authoritative through its offline grace.
        failures += 1;
      } finally {
        if (!cancelled) schedule(nextLicenseRefreshDelay(failures));
      }
    };
    // One launch check plus jittered twice-daily validation keeps entitlement
    // current without repeatedly calling the licensing service.
    schedule(1_500);
    return () => {
      cancelled = true;
      if (timer !== null) window.clearTimeout(timer);
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
          variants={overlayVariants}
          initial="hidden"
          animate="visible"
          exit="exit"
          transition={gateTransition}
        >
          <motion.div
            className={`license-gate-card${inline ? " license-gate-card--inline" : ""}`}
            role={inline ? "region" : "dialog"}
            aria-modal={inline ? undefined : true}
            aria-labelledby={titleId}
            aria-describedby={subtitleId}
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
              <div id={titleId} className="license-gate-title">WINCOMMANDER · LICENSE</div>
              <div id={subtitleId} className="license-gate-subtitle">
                {requestedFeatureLabel
                  ? <><strong>{requestedFeatureLabel}</strong> requires WinCommander Pro.</>
                  : "Purchase securely or activate an existing key."}
              </div>
            </div>

            {/* ui/tabs supplies the animated sliding pill (framer layoutId) that the
                hand-rolled underline lacked. */}
            <Tabs value={activeTab} onValueChange={(next) => setActiveTab(next as "buy" | "activate")}>
              <TabsList className="w-full">
                <TabsTrigger value="buy" className={TAB_TRIGGER_CLASS}>Buy License</TabsTrigger>
                <TabsTrigger value="activate" className={TAB_TRIGGER_CLASS}>Enter Key</TabsTrigger>
              </TabsList>

              <TabsContent value="buy" className="mt-2">
                <LicensePurchasePanel
                  licenseStatus={licenseStatus ?? null}
                  onActivated={() => void afterActivation("WinCommander Pro is now active!")}
                  onStartTrial={() => void handleStartTrial()}
                  isLicenseBusy={licenseBusy}
                />
                {licenseMessage && <div className="license-gate-message">{licenseMessage}</div>}
              </TabsContent>

              <TabsContent value="activate" className="mt-2">
                <div className="license-gate-activate">
                  <div className="license-gate-field">
                    <label className="license-gate-label" htmlFor={licenseKeyId}>License Key</label>
                    <input
                      id={licenseKeyId}
                      type="text"
                      ref={licenseKeyInputRef}
                      className="license-gate-input"
                      placeholder="WC-PRO-XXXX-XXXX-XXXX-XXXX-XXXX"
                      value={licenseKey}
                      onChange={(event) => setLicenseKey(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key !== "Enter") return;
                        event.preventDefault();
                        event.stopPropagation();
                        void handleActivate();
                      }}
                      disabled={licenseBusy}
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
              </TabsContent>
            </Tabs>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
