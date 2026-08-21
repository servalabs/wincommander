// src/components/shared/ManagedPolicyBanner.tsx
//
// F9 — Managed-policy banner.
//
// Shows a compact info strip when get_managed_policy() reports managed=true,
// indicating that one or more settings are controlled by the organisation's
// Group Policy (or equivalent MDM push to the Policies hive).
//
// Usage:
//   <ManagedPolicyBanner />
//
// The component is self-contained: it calls get_managed_policy on mount and
// re-polls every 60 s so a live GPO update (e.g. gpupdate /force) is
// reflected without restarting the app.
//
// Phase-2 note:
//   Hard enforcement (locking individual toggles) is not wired here.
//   This banner is purely informational.  See gpo_policy.rs for the
//   phase-2 plan.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

// ── Types ────────────────────────────────────────────────────────────────────

interface ManagedPolicy {
  managed: boolean;
  source: string;
  values: Record<string, boolean | string>;
}

// Human-readable labels for the recognised policy keys.
const POLICY_LABELS: Record<string, string> = {
  LockTelemetryOff: "Telemetry settings locked off",
  ForceSecureDNS: "Secure DNS enforced",
  DisableSelfDestruct: "Self-destruct / panic disabled",
  RequireStartupPin: "Startup PIN required",
  ManagedBannerText: undefined as unknown as string, // shown separately
};

// ── Component ────────────────────────────────────────────────────────────────

export default function ManagedPolicyBanner() {
  const [policy, setPolicy] = useState<ManagedPolicy | null>(null);

  async function fetchPolicy() {
    try {
      const result = await invoke<ManagedPolicy>("get_managed_policy");
      setPolicy(result);
    } catch {
      // Best-effort — if the command fails (e.g. very old build), hide silently.
      setPolicy(null);
    }
  }

  useEffect(() => {
    fetchPolicy();
    // Re-poll every 60 s so a live gpupdate is reflected promptly.
    const id = setInterval(fetchPolicy, 60_000);
    return () => clearInterval(id);
  }, []);

  if (!policy?.managed) return null;

  // Derive the banner message: prefer the admin-supplied ManagedBannerText,
  // fall back to the built-in default.
  const customText =
    typeof policy.values["ManagedBannerText"] === "string" &&
    (policy.values["ManagedBannerText"] as string).trim().length > 0
      ? (policy.values["ManagedBannerText"] as string).trim()
      : null;

  const bannerMessage = customText ?? "Some settings are managed by your organisation.";

  // Collect human-readable active policy lines (skip ManagedBannerText itself).
  const activeLines = Object.entries(policy.values)
    .filter(([key]) => key !== "ManagedBannerText" && POLICY_LABELS[key])
    .map(([key, value]) => {
      const label = POLICY_LABELS[key];
      if (typeof value === "boolean") {
        return value ? label : `${label} (off)`;
      }
      return `${label}: ${String(value)}`;
    });

  return (
    <div
      role="status"
      aria-label="Organisation-managed policy notice"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: "4px",
        padding: "10px 14px",
        marginBottom: "12px",
        borderRadius: "6px",
        background: "rgba(var(--wc-blue-rgb, 59 130 246) / 0.10)",
        border: "1px solid rgba(var(--wc-blue-rgb, 59 130 246) / 0.30)",
        fontSize: "0.8125rem",
        lineHeight: "1.4",
        color: "var(--wc-text-secondary, #94a3b8)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "6px",
          color: "var(--wc-text-primary, #e2e8f0)",
          fontWeight: 500,
        }}
      >
        {/* Building / organisation icon — inline SVG, no external dependency. */}
        <svg
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="currentColor"
          aria-hidden="true"
          style={{ flexShrink: 0, opacity: 0.8 }}
        >
          <path d="M1 14V4.5L6 2v12H1zm1-1h3V3.2L2 4.8V13zm4 1V6l9-2v10H6zm1-1h7V5.3L7 7V13z" />
        </svg>
        {bannerMessage}
      </div>

      {activeLines.length > 0 && (
        <ul
          style={{
            margin: "2px 0 0 20px",
            padding: 0,
            listStyle: "disc",
            fontSize: "0.75rem",
            opacity: 0.75,
          }}
        >
          {activeLines.map((line) => (
            <li key={line}>{line}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
