import { useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQueryClient } from "@tanstack/react-query";
import { settingsKeys } from "./queries/useSettingsQuery";
import { executeBackendCommand } from "./useBackend";
import { getRadarDriftToggles, getToggleById } from "../registry";
import { useAuthMode } from "../context/AuthModeContext";
import { useAppState } from "../context/AppContext";
import useEntitlements from "./useEntitlements";
import type { AppSettings } from "../types/settings";
import { buildToggleCommandParams, setByPath } from "../types/toggles";
import { createAutoHealer, HEAL_COOLDOWN_MS, isAutoHealEnabled, TOGGLE_REPAIR_VERIFIED_EVENT } from "../lib/autoHeal";
import { DASHBOARD_POLICY_FIELDS } from "../lib/dashboardPolicyObservation";
import { dashboardFixFailure, verifyDashboardToggleFix } from "../panels/dashboard/fixVerification";
import { showWarning } from "../utils/toast";

export default function useAutoHeal() {
  const { mode } = useAuthMode();
  const { appSettings, refreshSettings } = useAppState();
  const { canUse } = useEntitlements();
  const queryClient = useQueryClient();
  const mounted = useRef(false);
  const latest = useRef({ mode, appSettings, refreshSettings, canUse });
  latest.current = { mode, appSettings, refreshSettings, canUse };
  const run = useMemo(() => createAutoHealer({
    getSettings: () => !mounted.current || latest.current.mode === "decoy" ? undefined : latest.current.appSettings ?? undefined,
    toggles: getRadarDriftToggles(),
    canUse: toggle => latest.current.canUse(toggle.tier),
    probe: async () => {
      const mayRun = () => mounted.current && latest.current.mode !== "decoy" && isAutoHealEnabled(latest.current.appSettings ?? undefined);
      const [base, privacy] = await Promise.all([
        executeBackendCommand<Record<string, unknown>>("Get-WCSystemProbe"),
        executeBackendCommand<Record<string, unknown>>("Get-DashboardPrivacyPolicyStatus"),
      ]);
      if (!mayRun()) return;
      if (!base.success || !base.data) throw new Error(base.error || "Windows status is unavailable. Auto Heal will check again.");
      const probe = structuredClone(base.data);
      // The general probe does not cover these policies. Invalidate missing
      // observations instead of repairing against their old cached values.
      for (const [id, field] of Object.entries({ ...DASHBOARD_POLICY_FIELDS, diagTracing: "diagnosticEventTracingDisabled" })) {
        if (id === "kernelDmaProtect") continue;
        const toggle = getToggleById(id);
        const value = privacy.success ? privacy.data?.[field] : null;
        if (toggle) setByPath(probe, toggle.currentPath.replace(/^current\./, ""), typeof value === "boolean" ? value : null);
      }
      const updated = await invoke<AppSettings>("update_current_state", { probe });
      if (!mayRun()) return;
      queryClient.setQueryData(settingsKeys.detail(), updated);
      await latest.current.refreshSettings();
      return { ...updated, current: probe as unknown as AppSettings["current"] };
    },
    repair: async (toggle, target) => {
      const params = buildToggleCommandParams(toggle, target, toggle.capabilityKey
        ? { Capability: toggle.capabilityKey, Access: target ? "Deny" : "Allow" } : undefined);
      const result = await executeBackendCommand(toggle.capabilityKey ? "Set-AppCapabilityAccess" : target ? toggle.enableCmd : toggle.disableCmd, params);
      await verifyDashboardToggleFix(toggle.id, target, result, () => executeBackendCommand(
        toggle.id === "kernelDmaProtect" ? "Get-HardeningStatus" : "Get-DashboardPrivacyPolicyStatus",
      ));
    },
    failureMessage: dashboardFixFailure,
    notify: (label, message) => { void showWarning(`Auto Heal — ${label}: ${message}`, undefined, { kind: "notification" }); },
    verified: repair => { window.dispatchEvent(new CustomEvent(TOGGLE_REPAIR_VERIFIED_EVENT, { detail: repair })); },
  }), [queryClient]);
  const enabled = mode !== "decoy" && isAutoHealEnabled(appSettings ?? undefined);
  useEffect(() => {
    if (!enabled) return;
    mounted.current = true;
    void run();
    const timer = window.setInterval(() => { void run(); }, HEAL_COOLDOWN_MS);
    return () => { mounted.current = false; window.clearInterval(timer); };
  }, [enabled, run]);
}
