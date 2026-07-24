import { useState, useEffect } from "react";
import { motion, useReducedMotion } from "framer-motion";
import { Fingerprint, RefreshCcw, X } from "lucide-react";
import { fingerprintIcons } from "./assets";
import type { FingerprintConfig } from "./types";

interface Props extends FingerprintConfig {
  onClose?: () => void;
}

const DEFAULTS = {
  identity: "eFiHbYR6nF0sKDPb4tKo",
  ip: "180.211.97.148",
  location: "GANDHINAGAR, IN",
  isp: "BLAZENET'S NETWORK",
  timezone: "ASIA/KOLKATA",
};

/** A self-contained "scan your device fingerprint" demo panel. Ships working
 *  defaults so it needs no host wiring; every value is overridable for hosts
 *  that want neutral / different copy. */
export default function FingerprintMirror({ onClose, tagline, identity, ip, location, isp, timezone }: Props) {
  const [isLoading, setIsLoading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [hasConsented, setHasConsented] = useState(false);
  const reduce = useReducedMotion();

  const id = identity ?? DEFAULTS.identity;

  const handleConsent = () => {
    setHasConsented(true);
    setIsLoading(true);
    setProgress(0);
  };

  const handleReset = () => {
    setHasConsented(false);
    setIsLoading(false);
    setProgress(0);
  };

  useEffect(() => {
    if (!isLoading) return;
    const timer = setInterval(() => {
      setProgress((prev) => {
        if (prev >= 100) {
          clearInterval(timer);
          setIsLoading(false);
          return 100;
        }
        return prev + 2;
      });
    }, 30);
    return () => clearInterval(timer);
  }, [isLoading]);

  const rows = [
    { label: "IP Address", val: ip ?? DEFAULTS.ip, threshold: 25 },
    { label: "Location", val: location ?? DEFAULTS.location, threshold: 50 },
    { label: "Internet Provider", val: isp ?? DEFAULTS.isp, threshold: 75 },
    { label: "Timezone", val: timezone ?? DEFAULTS.timezone, threshold: 100 },
  ];

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -10 }}
      transition={{ duration: 0.5 }}
      className="rm-fp"
    >
      <div className="rm-fp-inner">
        {onClose && (
          <button type="button" onClick={onClose} aria-label="Close identity scan" className="rm-fp-close">
            <X />
          </button>
        )}

        <motion.div
          animate={reduce ? { top: "50%" } : { top: ["25%", "85%", "25%"] }}
          transition={reduce ? { duration: 0 } : { duration: 4, repeat: Infinity, ease: "linear" }}
          className="rm-fp-beam"
          style={{ opacity: isLoading || hasConsented ? 0.4 : 0.1 }}
        />
        <div className="rm-fp-bg" />

        {!hasConsented ? (
          <div className="rm-fp-intro">
            <div>
              <h2 className="rm-fp-h2">
                Identify <br /> Your <br /> Device
              </h2>
              <p className="rm-fp-sub">
                Click to scan your digital fingerprint and reveal what websites know about you.
              </p>
            </div>
            <button onClick={handleConsent} className="rm-fp-scan">
              <span>
                <Fingerprint />
                Scan Identity
              </span>
            </button>
          </div>
        ) : (
          <div className="rm-fp-body">
            <div style={{ display: "flex", flexDirection: "column", gap: "1rem", flex: 1 }}>
              <div className="rm-fp-idhead">
                <div>
                  <span className="rm-fp-idlabel">
                    {tagline ?? (
                      <>
                        Digital Identity — Like <strong>Aadhaar</strong> for Device
                      </>
                    )}
                  </span>
                  <div className="rm-fp-idval">{isLoading ? "••••••••••••••••••••" : id}</div>
                </div>
                <div className={`rm-fp-spin${isLoading ? " busy" : ""}`}>{isLoading ? <RefreshCcw /> : null}</div>
              </div>

              <div className="rm-fp-stats">
                <div className="rm-fp-stat">
                  <img src={fingerprintIcons.vpn} alt="VPN" />
                  <b>{isLoading ? "..." : "NO"}</b>
                </div>
                <div className="rm-fp-stat">
                  <img src={fingerprintIcons.tor} alt="TOR" />
                  <b>{isLoading ? "..." : "NO"}</b>
                </div>
              </div>

              <div className="rm-fp-net">
                <span className="rm-fp-net-title">Network Intelligence</span>
                <div className="rm-fp-net-list">
                  {rows.map((item) => {
                    const revealed = progress >= item.threshold;
                    return (
                      <div key={item.label} className="rm-fp-net-row">
                        <span className="rm-fp-net-label">{item.label}:</span>
                        <span className={`rm-fp-net-val${revealed ? "" : " scanning"}`}>
                          {revealed ? item.val : "Scanning..."}
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            </div>

            <div className="rm-fp-foot">
              <div className="rm-fp-status">
                <div className={`rm-fp-dot${isLoading ? " busy" : ""}`} />
                <span>{isLoading ? "Identifying Path..." : "No Threats Detected"}</span>
              </div>
              <button onClick={handleReset} className="rm-fp-retry">
                <RefreshCcw />
                re-try
              </button>
            </div>
          </div>
        )}
      </div>
    </motion.div>
  );
}
