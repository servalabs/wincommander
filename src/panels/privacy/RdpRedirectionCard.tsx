import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import { Button, Spinner, Switch, Tag } from "@/components/ui/bp";
import SectionCard from "../../components/shared/SectionCard";
import { useAppState } from "../../context/AppContext";
import "./RdpRedirectionCard.css";

interface RdpRedirectionStatus {
  isWindowsServer: boolean;
  productName: string;
  installationType: string;
  isAdmin: boolean;
  smartCards: boolean;
  drives: boolean;
  clipboard: boolean;
  printers: boolean;
  audioPlayback: boolean;
  microphone: boolean;
  pnpDevices: boolean;
  camera: boolean;
  webauthn: boolean;
  genericUsbDisabled: boolean;
  qwaveInstalled: boolean;
  mediaFoundationInstalled: boolean;
}

type CapabilityKey =
  | "smart_cards"
  | "drives"
  | "clipboard"
  | "printers"
  | "audio_playback"
  | "microphone"
  | "pnp_devices"
  | "camera"
  | "webauthn";

const rows: Array<{ key: CapabilityKey; field: keyof RdpRedirectionStatus; label: string; detail: string }> = [
  { key: "smart_cards", field: "smartCards", label: "Smart cards / DSC", detail: "Native RDP smart-card channel for signing tokens." },
  { key: "drives", field: "drives", label: "Drives", detail: "Redirect client drives through \\tsclient, including removable drives." },
  { key: "clipboard", field: "clipboard", label: "Clipboard", detail: "Allow text and file clipboard redirection." },
  { key: "printers", field: "printers", label: "Printers", detail: "Use per-session redirected printers / Easy Print." },
  { key: "audio_playback", field: "audioPlayback", label: "Audio playback", detail: "Play server-session audio on the connecting PC." },
  { key: "microphone", field: "microphone", label: "Microphone", detail: "Record from the connecting PC inside its RDP session." },
  { key: "camera", field: "camera", label: "Camera", detail: "Redirect supported video-capture devices through RD Camera Bus." },
  { key: "webauthn", field: "webauthn", label: "WebAuthn / passkeys", detail: "Allow supported WebAuthn credentials in the remote session." },
  { key: "pnp_devices", field: "pnpDevices", label: "Supported Plug and Play", detail: "Allow supported PnP redirection without enabling generic USB passthrough." },
];

async function rdpMachineSetting<T>(value: Record<string, unknown>): Promise<T> {
  return invoke<T>("apply_machine_setting", {
    request: {
      setting: "rdp_redirection",
      value: { kind: "rdp_redirection", ...value },
    },
  });
}

export default function RdpRedirectionCard() {
  const { systemInfo } = useAppState();
  const osLooksLikeServer = systemInfo?.osName?.toLowerCase().includes("server") === true;
  const [status, setStatus] = useState<RdpRedirectionStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    if (!osLooksLikeServer) return;
    setLoading(true);
    setError(null);
    try {
      const next = await rdpMachineSetting<RdpRedirectionStatus>({ action: "status" });
      // Backend performs an independent edition check. If the frontend OS label
      // was ambiguous, fail closed and do not expose server-only controls.
      setStatus(next.isWindowsServer ? next : null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [osLooksLikeServer]);

  useEffect(() => {
    if (osLooksLikeServer) void refresh();
  }, [osLooksLikeServer, refresh]);

  const enabledCount = useMemo(
    () => status ? rows.filter((row) => status[row.field] === true).length : 0,
    [status],
  );

  const setCapability = async (key: CapabilityKey, enabled: boolean) => {
    setBusyKey(key);
    setError(null);
    try {
      const next = await rdpMachineSetting<RdpRedirectionStatus>({
        action: "set_capability",
        capability: key,
        enabled,
      });
      setStatus(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyKey(null);
    }
  };

  const applyRecommended = async () => {
    setBusyKey("recommended");
    setError(null);
    try {
      const next = await rdpMachineSetting<RdpRedirectionStatus>({ action: "apply_recommended" });
      setStatus(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyKey(null);
    }
  };

  const copyClientProfile = async () => {
    setBusyKey("profile");
    setError(null);
    try {
      const result = await rdpMachineSetting<{ profile: string }>({ action: "client_profile" });
      await navigator.clipboard.writeText(result.profile);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyKey(null);
    }
  };

  // Product requirement: this surface must not exist on Windows client SKUs.
  if (!osLooksLikeServer) return null;

  return (
    <SectionCard
      title="RDP Device Redirection"
      icon="desktop"
      headerRight={status ? <Tag minimal intent="primary">WINDOWS SERVER</Tag> : undefined}
    >
      {loading && !status ? (
        <div className="flex items-center gap-2 py-2 text-xs text-[var(--text-mute)]">
          <Spinner size={16} /> Reading Windows Server RDP policy…
        </div>
      ) : status ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-sm font-medium">Native per-session resources</div>
              <div className="mt-1 text-xs text-[var(--text-mute)]">
                {status.productName} · {enabledCount}/{rows.length} channels allowed
              </div>
            </div>
            <Tag minimal intent={status.genericUsbDisabled ? "success" : "warning"}>
              {status.genericUsbDisabled ? "GENERIC USB OFF" : "CHECK USB POLICY"}
            </Tag>
          </div>

          <div className="rdp-redirection-grid">
            {rows.map((row) => {
              const checked = status[row.field] === true;
              return (
                <div key={row.key} className="rdp-redirection-row">
                  <div className="min-w-0">
                    <div className="text-xs font-medium">{row.label}</div>
                    <div className="mt-0.5 text-[11px] leading-4 text-[var(--text-mute)]">{row.detail}</div>
                  </div>
                  <Switch
                    checked={checked}
                    disabled={!status.isAdmin || busyKey !== null}
                    onChange={(event) => void setCapability(row.key, event.currentTarget.checked)}
                    aria-label={`${row.label} RDP redirection`}
                  />
                </div>
              );
            })}
          </div>

          <div className="flex flex-wrap gap-2">
            <Tag minimal intent={status.mediaFoundationInstalled ? "success" : "warning"}>
              Media Foundation {status.mediaFoundationInstalled ? "ready" : "missing"}
            </Tag>
            <Tag minimal intent={status.qwaveInstalled ? "success" : "warning"}>
              qWave {status.qwaveInstalled ? "ready" : "missing"}
            </Tag>
          </div>

          {!status.isAdmin && (
            <div className="text-xs text-[var(--color-warning)]">
              Run WinCommander as administrator to change machine-wide RDP policy.
            </div>
          )}

          {error && <div className="text-xs text-[var(--color-danger)]">{error}</div>}

          <div className="flex flex-wrap gap-2">
            <Button
              small
              intent="primary"
              icon="endorsed"
              text="Apply recommended"
              loading={busyKey === "recommended"}
              disabled={!status.isAdmin || busyKey !== null}
              onClick={() => void applyRecommended()}
            />
            <Button
              small
              icon="clipboard"
              text={copied ? "RDP profile copied" : "Copy client RDP profile"}
              loading={busyKey === "profile"}
              disabled={busyKey !== null}
              onClick={() => void copyClientProfile()}
            />
            <Button
              small
              minimal
              icon="refresh"
              text="Refresh"
              disabled={busyKey !== null}
              onClick={() => void refresh()}
            />
          </div>

          <div className="text-[11px] leading-4 text-[var(--text-mute)]">
            Native RDP virtual channels only. Generic RemoteFX USB passthrough stays disabled. Reconnect existing RDP sessions after changing these settings so the client and server renegotiate channels.
          </div>
        </div>
      ) : (
        <div className="text-xs text-[var(--text-mute)]">
          {error ?? "Windows Server RDP redirection status is unavailable."}
        </div>
      )}
    </SectionCard>
  );
}
