/** Local, non-secret deployment-manifest types for the Fleet Vault tab. */
export interface VaultGroup {
  id: "accounting" | "sales" | "partner";
  localGroup: string;
  users: string[];
}

export type VaultKind = "container" | "partition" | "dual";

export interface VaultVolumePolicy {
  id: string;
  label: string;
  kind: VaultKind;
  backing: string;
  sizeMb: number;
  driveLetter: string;
  credentialRef: string;
  allowedGroups: Array<VaultGroup["id"]>;
  ownerOnly: boolean;
  hiddenSizeMb?: number;
  hiddenCredentialRef?: string;
  groupReadOnly?: boolean;
}

export interface VaultFleetPolicy {
  schema: 1;
  ownerPrincipal: string;
  diskNumber: number | null;
  diskUniqueId: string;
  confirmationText: string;
  mountScope: "per-user";
  allowUnallocatedSpace: boolean;
  unallocatedReserveMb: number;
  preloadDriverAtStartup: boolean;
  installDirectory: string;
  groups: VaultGroup[];
  volumes: VaultVolumePolicy[];
}

export interface VaultMatrixRow {
  userClass: "owner" | VaultGroup["id"];
  volume: string;
  canSeeBacking: boolean;
  canMount: boolean;
  canDecrypt: boolean;
  canAccessContent: boolean;
  seesOtherSessionMount: boolean;
}
