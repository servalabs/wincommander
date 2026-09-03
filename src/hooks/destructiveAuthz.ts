import { invoke } from "@tauri-apps/api/core";

export type DestructiveRequest =
  | { command: "lockdown"; deactivateLicenseFirst: boolean; shutdownSystem: boolean }
  | { command: "full_lockdown" }
  | { command: "disk_delete_item"; path: string }
  | { command: "delete_decoy"; path: string }
  | { command: "internet_kill_switch_set"; enable: boolean }
  | { command: "secure_erase"; path: string }
  | { command: "free_space_erase"; driveLetter: string; mediaType: string }
  | {
      command: "selective_crypto_erase";
      target: {
        kind: string;
        path?: string;
        mountLetter?: string;
        mountPoint?: string;
        confirmed: boolean;
        osVolumeAck?: string;
      };
    };

export function requestDestructiveCapability(
  request: DestructiveRequest,
  pin?: string,
): Promise<string> {
  return invoke<string>("request_destructive_confirmation", {
    request,
    pin: pin ?? null,
  });
}
