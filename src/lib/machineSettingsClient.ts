import { invoke } from "@tauri-apps/api/core";

export type MachineSettingRequest =
  | {
      setting: "rdp_incoming";
      value: {
        kind: "rdp_incoming";
        enabled: boolean;
        idle_timeout_seconds: number;
      };
    }
  | {
      setting: "rdp_lock";
      value: {
        kind: "rdp_lock";
        locked: boolean;
      };
    };

export type MachineSettingObserved =
  | {
      kind: "rdp_incoming";
      enabled: boolean;
      deny_connections: boolean | null;
      idle_timeout_seconds: number | null;
      max_idle_time_ms: number | null;
      max_disconnection_time_ms: number | null;
      max_connection_time_ms: number | null;
      reset_broken: boolean | null;
    }
  | {
      kind: "rdp_lock";
      locked: boolean;
    };

export function applyMachineSetting(request: MachineSettingRequest): Promise<MachineSettingObserved> {
  return invoke<MachineSettingObserved>("apply_machine_setting", { request });
}
