import type { CSSProperties } from "react";
import { RiskMatrix } from "@assets/components/risk-matrix";

// Thin host wrapper around the vendored RiskMatrix. This file only maps the
// component's `--rm-*` theme contract onto WinCommander's `--color-*` design
// tokens (so it follows the app's dark/light theme automatically) and supplies
// the local heading. Matrix behaviour, data, sourcing, and the FingerprintMirror
// "YOU" panel all live in the shared component.
//
// WinCommander is country-neutral (no "you live in India" signalling): we render
// no flag and drop the India-specific Aadhaar line from the fingerprint demo.
const theme = {
  "--rm-surface": "var(--color-bg-secondary)",
  "--rm-card": "var(--color-bg-secondary)",
  "--rm-card-2": "var(--color-bg-tertiary)",
  "--rm-border": "var(--color-border)",
  "--rm-fg": "var(--color-text-primary)",
  "--rm-muted": "var(--color-text-muted)",
  "--rm-accent": "var(--color-accent)",
  "--rm-accent-fg": "var(--color-bg-primary)",
  "--rm-danger": "var(--color-danger)",
  "--rm-danger-fg": "#ffffff",
  "--rm-success": "var(--color-success)",
  "--rm-warning": "#eab308",
  "--rm-node-bg": "#ffffff",
} as CSSProperties;

export default function SovereigntyRiskMatrix() {
  return (
    <div className="sovereignty-risk-matrix">
      <RiskMatrix
        style={theme}
        eyebrow="Privacy Risks"
        title={
          <>
            <span className="wc-risk-title-line">Data Exposure &amp;</span>
            <span className="rm-title-accent wc-risk-title-line">Network Visibility</span>
          </>
        }
        subtitle={
          <>
            Click any company or agency to explore their documented incidents. Click <strong>YOU</strong> to scan your
            device fingerprint.
          </>
        }
        fingerprint={{ tagline: "Digital Identity — a unique ID every website can see" }}
      />
    </div>
  );
}
