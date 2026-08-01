// src/panels/mesh/VpnKillSwitchSection.tsx
//
// VPN-drop kill switch UI — arms the Free `vpn_kill_switch` watcher. When a
// watched VPN tunnel (Pvt Mesh / ProtonVPN) drops, all internet is cut via the
// existing internet kill switch until it reconnects or the user releases it.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Icon, Tooltip } from "@/components/ui/bp";
import { useAppState } from "../../context/AppContext";
import { showError } from "../../utils/toast";
import "./VpnKillSwitchSection.css";

type TunnelState = "up" | "down" | "unknown";

interface VpnKsStatus {
  armed: boolean;
  fired: boolean;
  tunnelState: TunnelState;
  lastFiredAt: number;
}

const PROVIDERS: { value: string; label: string }[] = [
  { value: "auto", label: "Auto" },
  { value: "tailscale", label: "Pvt Mesh" },
  { value: "protonvpn", label: "ProtonVPN" },
];

const STATE_LABEL: Record<TunnelState, string> = {
  up: "Tunnel up",
  down: "Tunnel down",
  unknown: "Tunnel not watched",
};

export default function VpnKillSwitchSection() {
  const { appSettings, patchAppSettings } = useAppState();
  const cfg = appSettings?.ideal?.network?.vpnKillSwitch;
  const armed = cfg?.armed ?? false;
  const provider = cfg?.provider ?? "auto";

  const [status, setStatus] = useState<VpnKsStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await invoke<VpnKsStatus>("vpn_kill_switch_status"));
    } catch {
      /* status is best-effort */
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), 5000);
    return () => clearInterval(id);
  }, [refresh]);

  useEffect(() => {
    const unlisten = listen("vpn-kill-switch-fired", () => void refresh());
    return () => {
      void unlisten.then((f) => f());
    };
  }, [refresh]);

  const setArmed = useCallback(
    async (next: boolean) => {
      setBusy(true);
      try {
        await patchAppSettings({
          ideal: { network: { vpnKillSwitch: { armed: next, provider } } },
        });
        await invoke("vpn_kill_switch_arm", { enable: next });
        await refresh();
      } catch (e) {
        showError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [patchAppSettings, provider, refresh],
  );

  const changeProvider = useCallback(
    async (next: string) => {
      try {
        await patchAppSettings({
          ideal: { network: { vpnKillSwitch: { provider: next, armed } } },
        });
        // Re-arm so the watcher picks up the new provider.
        if (armed) {
          await invoke("vpn_kill_switch_arm", { enable: false });
          await invoke("vpn_kill_switch_arm", { enable: true });
        }
        await refresh();
      } catch (e) {
        showError(String(e));
      }
    },
    [patchAppSettings, armed, refresh],
  );

  const tunnel = status?.tunnelState ?? "unknown";

  return (
    <div className={`vpn-ks-card${armed ? " vpn-ks-card--armed" : ""}`}>
      <div className="vpn-ks-header">
        <div className="vpn-ks-copy">
          <span className="vpn-ks-eyebrow">Feature</span>
          <span className="vpn-ks-label">
            Block internet if VPN drops
            <Tooltip content="While armed, a VPN drop also blocks this app's own network (licence checks, updates) until the VPN reconnects or you release the kill switch on the dashboard.">
              <Icon icon="info-sign" size={11} className="vpn-ks-info-icon" />
            </Tooltip>
          </span>
        </div>
        <span className="vpn-ks-toggle-cluster">
          <Select value={provider} onValueChange={(v) => void changeProvider(v)}>
            <SelectTrigger className="vpn-ks-select" aria-label="VPN provider to watch">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PROVIDERS.map((p) => (
                <SelectItem key={p.value} value={p.value}>
                  {p.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className={`vpn-ks-armed-label${armed ? " vpn-ks-armed-label--on" : ""}`}>
            {armed ? "ARMED" : "DISARMED"}
          </span>
          <Switch
            checked={armed}
            disabled={busy}
            onCheckedChange={(v) => void setArmed(v)}
            aria-label="Block internet if VPN drops"
          />
        </span>
      </div>
      <div className="vpn-ks-controls">
        <span className={`vpn-ks-state-chip vpn-ks-state-chip--${tunnel}`}>{STATE_LABEL[tunnel]}</span>
        {status?.fired && <span className="vpn-ks-fired">internet currently cut</span>}
      </div>
    </div>
  );
}
