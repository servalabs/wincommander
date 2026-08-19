/** Local, non-secret deployment-manifest types for the Fleet Vault tab. */

export type VaultKind = "container" | "partition" | "dual";
export type VaultPermission = "read" | "write";

export interface VaultVolumePolicy {
  id: string;
  label: string;
  kind: VaultKind;
  backing: string;
  sizeMb: number;
  driveLetter: string;
  credentialRef: string;
  groupPermissions: Record<string, VaultPermission>;
  ownerOnly: boolean;
  hiddenSizeMb?: number;
  hiddenCredentialRef?: string;
}

export interface VaultFleetPolicy {
  schema: 3;
  ownerPrincipal: string;
  diskNumber: number | null;
  diskUniqueId: string;
  confirmationText: string;
  mountScope: "per-user";
  allowUnallocatedSpace: boolean;
  unallocatedReserveMb: number;
  preloadDriverAtStartup: boolean;
  installDirectory: string;
  volumes: VaultVolumePolicy[];
}

export interface VaultMatrixRow {
  userClass: string;
  groups: string;
  volume: string;
  effectiveAccess: "None" | "Read" | "Write";
  canSeeBacking: boolean;
  canMount: boolean;
  canDecrypt: boolean;
  canAccessContent: boolean;
  seesOtherSessionMount: boolean;
}
