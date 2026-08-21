// SPDX-License-Identifier: AGPL-3.0-or-later
// src/panels/dev/index.tsx
//
// DEV / TEST panel — only visible when is_dev_build() returns true.
//
// Gate: calls is_dev_build() on mount.  If the binary is a release build
// the panel renders a one-liner explanation and exits early.  This means
// the panel is safe to ship in the build — it has zero footprint in release.
//
// Contents:
//   • Test action buttons: Simulate event, Reset state
//
// Styling: V2 token system — var(--color-*), var(--radius-*), font-mono.
// No Blueprint imports except the bp shim Icon.

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/bp";
import { Icon } from "@/components/ui/icon";
import { showSuccess, showError } from "../../utils/toast";
import PanelHeader from "../../components/shared/PanelHeader";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";
import "./index.css";

type SimEventKind =
  | "paste_hit"
  | "decoy_hit"
  | "ransomware_detected"
  | "honeypot_connection"
  | "wifi_rogue_ap";

const SIM_EVENT_KINDS: SimEventKind[] = [
  "paste_hit",
  "decoy_hit",
  "ransomware_detected",
  "honeypot_connection",
  "wifi_rogue_ap",
];

// ── Main panel ─────────────────────────────────────────────────────────

export default function DevPanel() {
  const [isDev, setIsDev] = useState<boolean | null>(null);

  // Probe the Rust flag on mount — the only reliable gate for kit debug exes.
  useEffect(() => {
    invoke<boolean>("is_dev_build")
      .then(setIsDev)
      .catch(() => setIsDev(false));
  }, []);

  if (isDev === null) {
    return (
      <div className="dev-panel-release-guard" role="status" aria-busy="true">
        <Icon icon="refresh" size={20} />
        <span>Checking whether Dev Tools are available…</span>
      </div>
    );
  }

  if (!isDev) {
    return (
      <div className="dev-panel-release-guard">
        <Icon icon="lock" size={20} />
        <span>Dev panel is not available in release builds.</span>
      </div>
    );
  }

  return <DevPanelInner />;
}

// ── Inner panel (only renders when isDev === true) ─────────────────────

function DevPanelInner() {
  const confirmAction = useAppConfirm();
  // ── Test actions ──
  const [busy, setBusy] = useState<string | null>(null);
  const [simKind, setSimKind] = useState<SimEventKind>("paste_hit");

  const runAction = useCallback(async (id: string, fn: () => Promise<unknown>) => {
    setBusy(id);
    try {
      await fn();
      showSuccess(`Done: ${id}`);
    } catch (e) {
      showError(String(e));
    } finally {
      setBusy(null);
    }
  }, []);

  const simulateEvent = () =>
    runAction("simulate_event", () => invoke("dev_simulate_event", { kind: simKind }));

  const resetState = async () => {
    const accepted = await confirmAction({
      title: "Reset all WinCommander settings?",
      description: "This deletes store/settings.dat. WinCommander will start in first-run state on the next launch. This cannot be undone.",
      confirmLabel: "Reset settings",
    });
    if (!accepted) return;
    await runAction("reset_state", () => invoke("dev_reset_state"));
  };

  // ── Render ──
  return (
    <div className="dev-panel">
      <PanelHeader
        title="Dev Tools"
        description="Debug build only — absent from release builds."
      />

      {/* Test actions */}
      <SectionCard title="Test Actions" icon="play">
        <div className="dev-actions-grid">
          {/* Simulate event */}
          <div className="dev-action-row">
            <Button
              text="Simulate event"
              icon="warning-sign"
              intent="warning"
              className="dev-action-btn"
              loading={busy === "simulate_event"}
              onClick={simulateEvent}
            />
            <select
              className="dev-select"
              aria-label="Simulated event type"
              value={simKind}
              onChange={(e) => setSimKind(e.target.value as SimEventKind)}
            >
              {SIM_EVENT_KINDS.map((k) => (
                <option key={k} value={k}>{k}</option>
              ))}
            </select>
          </div>

          {/* Reset state */}
          <div className="dev-action-row">
            <Button
              text="Reset settings"
              icon="trash"
              intent="danger"
              className="dev-action-btn"
              loading={busy === "reset_state"}
              onClick={resetState}
            />
            <span className="dev-action-desc">
              Wipes <span className="font-mono">store/settings.dat</span> — restarts as first-run on next launch
            </span>
          </div>

        </div>
      </SectionCard>
    </div>
  );
}
