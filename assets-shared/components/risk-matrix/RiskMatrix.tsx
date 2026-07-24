import { Fragment, useState } from "react";
import { motion, AnimatePresence, useReducedMotion } from "framer-motion";
import { AlertTriangle, Globe, Activity, ExternalLink, XCircle } from "lucide-react";
import { SCANDALS, TECH_ORDER, AGENCY_ORDER } from "./scandals";
import { logos, editorial } from "./assets";
import FingerprintMirror from "./FingerprintMirror";
import type { RiskMatrixProps } from "./types";
import "./risk-matrix.css";

const RADIUS = 174;

/** Distribute orbit nodes across the top ~2/3, leaving a gap at the bottom so
 *  they never collide with the central cluster or the YOU node. Radius + arc
 *  are sized so the current node count (14) stays ~57px center-to-center — the
 *  same comfortable spacing the layout had at 11 nodes. */
function getPos(index: number, total: number) {
  if (total <= 1) return { x: 0, y: 0 };
  const gapAngle = 115 * (Math.PI / 180);
  const startAngle = Math.PI / 2 + gapAngle / 2;
  const availableAngle = 2 * Math.PI - gapAngle;
  const angle = startAngle + (index / (total - 1)) * availableAngle;
  return { x: Math.cos(angle) * RADIUS, y: Math.sin(angle) * RADIUS };
}

export default function RiskMatrix({
  className,
  style,
  eyebrow = "Sovereignty Risk Matrix",
  title,
  subtitle,
  youPanel,
  fingerprint,
  youLabel = "YOU",
  youFlagUrl,
}: RiskMatrixProps) {
  const [selectedCompany, setSelectedCompany] = useState<string | null>(null);
  const [showFingerprint, setShowFingerprint] = useState(false);
  const [isEmbargo, setIsEmbargo] = useState(false);
  const reduce = useReducedMotion();

  const selectedData = selectedCompany ? SCANDALS[selectedCompany] : null;

  const handleYouClick = () => {
    setSelectedCompany(null);
    setShowFingerprint(true);
  };
  const handleCompanyClick = (key: string) => {
    setShowFingerprint(false);
    setSelectedCompany(key);
  };

  const youPanelNode =
    youPanel !== undefined ? youPanel : <FingerprintMirror {...(fingerprint || {})} onClose={() => setShowFingerprint(false)} />;

  return (
    <div className={`rm-root${className ? " " + className : ""}`} style={style}>
      {(title || eyebrow || subtitle) && (
        <div className="rm-header">
          {eyebrow && <span className="rm-eyebrow">{eyebrow}</span>}
          {title && <h2 className="rm-title">{title}</h2>}
          {subtitle && <p className="rm-subtitle">{subtitle}</p>}
        </div>
      )}

      <div className="rm-grid">
        {/* LEFT: THE MATRIX */}
        <div className={`rm-matrix${isEmbargo ? " is-embargo" : ""}`}>
          <div className="rm-matrix-bar">
            <span className="rm-bar-label">
              <span className="rm-live-dot" />
              Select a Vector
            </span>
            <button type="button" onClick={() => setIsEmbargo((v) => !v)} className={`rm-toggle${isEmbargo ? " is-on" : ""}`}>
              <Activity />
              {isEmbargo ? "Submit to US Demands" : "Simulate Sanction"}
            </button>
          </div>

          <div
            className="rm-stage"
            role="group"
            aria-label="Surveillance-network diagram: US intelligence agencies at the center, connected to major US technology companies. Select any node to view its documented incidents."
          >
            <span className="rm-sr-only" aria-live="polite">
              {isEmbargo
                ? "Sanction simulation on: US services shown as blocked and network links severed."
                : "Sanction simulation off."}
            </span>
            <div className="rm-grid-bg" />

            {/* central intel-agency cluster */}
            <div className="rm-agencies">
              {AGENCY_ORDER.map((key, i) => {
                const agency = SCANDALS[key];
                if (!agency) return null;
                const big = i === 1;
                return (
                  <motion.button
                    key={key}
                    type="button"
                    onClick={() => handleCompanyClick(key)}
                    whileHover={{ scale: 1.12, zIndex: 50 }}
                    whileTap={{ scale: 0.95 }}
                    className={`rm-agency-btn ${big ? "lg" : "sm"}`}
                  >
                    <img src={logos[agency.logo]} alt={agency.name} />
                  </motion.button>
                );
              })}
            </div>

            {/* orbiting tech giants */}
            <div className="rm-nodes">
              <div className="rm-nodes-inner">
                {TECH_ORDER.map((key, index) => {
                  const pos = getPos(index, TECH_ORDER.length);
                  const company = SCANDALS[key];
                  if (!company) return null;
                  const isSelected = selectedCompany === key;
                  return (
                    <Fragment key={key}>
                      <svg className="rm-line-svg">
                        <motion.line
                          initial={{ pathLength: 0, opacity: 0 }}
                          animate={{ pathLength: 1, opacity: isEmbargo ? 0.2 : 0.5 }}
                          x1={0}
                          y1={-20}
                          x2={pos.x}
                          y2={pos.y}
                          stroke={isEmbargo ? "var(--rm-muted)" : company.color}
                          strokeWidth={isSelected ? 2.5 : 1}
                          strokeDasharray={isEmbargo ? "5,5" : "none"}
                        />
                        {!isEmbargo && !reduce && (
                          <circle r="2" fill={company.color} opacity="0.6">
                            <animateMotion
                              dur={`${3 + index * 0.5}s`}
                              repeatCount="indefinite"
                              path={`M 0 -20 L ${pos.x} ${pos.y}`}
                              keyPoints="0;1"
                              keyTimes="0;1"
                            />
                          </circle>
                        )}
                      </svg>
                      <motion.button
                        type="button"
                        onClick={() => handleCompanyClick(key)}
                        whileHover={{ scale: 1.2 }}
                        whileTap={{ scale: 0.9 }}
                        animate={{ scale: isSelected ? 1.25 : 1 }}
                        className={`rm-node${isSelected ? " is-selected" : ""}`}
                        style={{
                          x: pos.x,
                          y: pos.y,
                          borderColor: isEmbargo ? "var(--rm-muted)" : company.color,
                          filter: isEmbargo ? "grayscale(100%)" : "none",
                        }}
                      >
                        <img src={logos[company.logo]} alt={company.name} />
                        <span className="rm-node-label">{company.name}</span>
                      </motion.button>
                    </Fragment>
                  );
                })}
              </div>
            </div>
          </div>

          {/* YOU node + sovereign-perimeter cable */}
          <div className="rm-you-wrap">
            <div className={`rm-cable${isEmbargo ? " is-embargo" : ""}`}>{!isEmbargo && <div className="rm-cable-pulse" />}</div>
            <motion.button
              type="button"
              onClick={handleYouClick}
              whileHover={{ scale: 1.02 }}
              whileTap={{ scale: 0.98 }}
              className={`rm-you-btn${isEmbargo ? " is-embargo" : showFingerprint ? " is-active" : ""}`}
            >
              <span className="rm-you-inner">
                {youFlagUrl && <img src={youFlagUrl} alt="" />}
                {isEmbargo ? "SANCTIONED" : youLabel}
              </span>
            </motion.button>
          </div>
        </div>

        {/* RIGHT: INTELLIGENCE PANEL */}
        <div className="rm-panel">
          <AnimatePresence mode="wait">
            {showFingerprint ? (
              <motion.div
                key="fp"
                className="rm-detail"
                style={{ height: "100%" }}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
              >
                {youPanelNode}
              </motion.div>
            ) : selectedData ? (
              <motion.div
                key={selectedCompany}
                initial={{ opacity: 0, x: 20 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -20 }}
                className="rm-detail"
              >
                <div className="rm-panel-head">
                  <div className="rm-panel-head-icons">
                    <img src={logos[selectedData.logo]} alt="" />
                    <button type="button" onClick={() => setSelectedCompany(null)} className="rm-close" aria-label="Close">
                      <XCircle />
                    </button>
                  </div>
                  <h3 className="rm-panel-name">{selectedData.name}</h3>
                  <p className="rm-panel-desc" style={{ color: "var(--rm-danger)" }}>
                    {selectedData.description}
                  </p>
                  <div className="rm-panel-badges">
                    <span className="rm-badge rm-badge--danger">
                      <AlertTriangle /> High Risk
                    </span>
                    <span className="rm-badge rm-badge--muted">HQ: USA (FISA 702)</span>
                  </div>
                </div>

                {/* data-lenis-prevent: this panel scrolls independently; without
                    it the host site's Lenis smooth-scroll hijacks the wheel and
                    scrolls the whole page instead of this list. Harmless when no
                    Lenis instance is present (e.g. the WinCommander desktop app). */}
                <div className="rm-timeline" data-lenis-prevent>
                  <h4 className="rm-timeline-title">Compliance &amp; Risk History</h4>
                  {selectedData.events.map((event, i) => (
                    <div key={i} className="rm-event">
                      <div className="rm-event-top">
                        <span className="rm-event-year">{event.year}</span>
                        <span className={`rm-sev rm-sev--${event.severity.toLowerCase()}`}>{event.severity}</span>
                      </div>
                      <h5 className="rm-event-title">{event.title}</h5>
                      <p className="rm-event-desc">{event.desc}</p>
                      {event.image && editorial[event.image] && (
                        <div className="rm-event-img">
                          <img src={editorial[event.image]} alt={event.title} draggable={false} />
                        </div>
                      )}
                      {event.sources.length > 0 && (
                        <div className="rm-sources">
                          {event.sources.map((source, si) => (
                            <a key={si} href={source.url} target="_blank" rel="noopener noreferrer" className="rm-source">
                              {source.label} <ExternalLink />
                            </a>
                          ))}
                        </div>
                      )}
                    </div>
                  ))}
                </div>

                <div className="rm-panel-foot">
                  <p>⚠ Data is subject to US laws</p>
                </div>
              </motion.div>
            ) : (
              <motion.div
                key="empty"
                className="rm-panel-empty"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
              >
                <Globe />
                <h3>Select a Vector</h3>
                <p>
                  Click any company, agency, or <strong>{youLabel}</strong> to explore.
                </p>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>
    </div>
  );
}
