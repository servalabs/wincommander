import { invoke } from "@tauri-apps/api/core";

export interface RdpRedirectionStatus {
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

export type CapabilityKey =
  | "smart_cards"
  | "drives"
  | "clipboard"
  | "printers"
  | "audio_playback"
  | "microphone"
  | "pnp_devices"
  | "camera"
  | "webauthn";

export async function rdpMachineSetting<T>(value: Record<string, unknown>): Promise<T> {
  return invoke<T>("apply_machine_setting", {
    request: {
      setting: "rdp_redirection",
      value: { kind: "rdp_redirection", ...value },
    },
  });
}
