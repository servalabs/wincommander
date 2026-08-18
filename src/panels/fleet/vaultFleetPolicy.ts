import type { VaultFleetPolicy, VaultMatrixRow, VaultVolumePolicy } from "./vaultFleetTypes";

export const DEFAULT_VAULT_POLICY: VaultFleetPolicy = {
  schema: 1,
  ownerPrincipal: "",
  diskNumber: null,
  diskUniqueId: "",
  confirmationText: "",
  mountScope: "per-user",
  allowUnallocatedSpace: true,
  unallocatedReserveMb: 256,
  preloadDriverAtStartup: true,
  installDirectory: "C:\\Program Files\\WinCommander\\VeraCrypt Engine",
  groups: [
    { id: "accounting", localGroup: "WC_Accounting", users: ["Accounting1", "Accounting2", "Accounting3"] },
    { id: "sales", localGroup: "WC_Sales", users: ["Sales1", "Sales2", "Sales3"] },
    { id: "partner", localGroup: "WC_Partner", users: ["Partner1"] },
  ],
  volumes: [
    {
      id: "partner-decoy",
      label: "Partner documents",
      kind: "dual",
      backing: "partner-documents.hc",
      sizeMb: 768,
      hiddenSizeMb: 192,
      driveLetter: "P",
      credentialRef: "PartnerOuter",
      hiddenCredentialRef: "OwnerHidden",
      allowedGroups: ["partner"],
      ownerOnly: false,
      groupReadOnly: true,
    },
    {
      id: "owner-decoy",
      label: "Archive",
      kind: "dual",
      backing: "archive.hc",
      sizeMb: 768,
      hiddenSizeMb: 192,
      driveLetter: "O",
      credentialRef: "OwnerOuter",
      hiddenCredentialRef: "OwnerArchiveHidden",
      allowedGroups: [],
      ownerOnly: true,
    },
    {
      id: "sales",
      label: "Sales",
      kind: "container",
      backing: "sales.hc",
      sizeMb: 1024,
      driveLetter: "S",
      credentialRef: "Sales",
      allowedGroups: ["sales"],
      ownerOnly: false,
    },
    {
      id: "accounting",
      label: "Accounting",
      kind: "container",
      backing: "accounting.hc",
      sizeMb: 1024,
      driveLetter: "A",
      credentialRef: "Accounting",
      allowedGroups: ["accounting"],
      ownerOnly: false,
    },
    {
      id: "sales-raw",
      label: "Sales raw volume",
      kind: "partition",
      backing: "allocate-from-unallocated:2048MiB",
      sizeMb: 2048,
      driveLetter: "R",
      credentialRef: "SalesRaw",
      allowedGroups: ["sales"],
      ownerOnly: false,
    },
    {
      id: "accounting-raw",
      label: "Accounting raw volume",
      kind: "partition",
      backing: "allocate-from-unallocated:2048MiB",
      sizeMb: 2048,
      driveLetter: "Q",
      credentialRef: "AccountingRaw",
      allowedGroups: ["accounting"],
      ownerOnly: false,
    },
  ],
};

const STORAGE_KEY = "wincommander.fleet.vault-policy.v1";

export function loadVaultPolicy(): VaultFleetPolicy {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (!stored) return DEFAULT_VAULT_POLICY;
    const parsed = JSON.parse(stored) as VaultFleetPolicy;
    return parsed.schema === 1 ? { ...DEFAULT_VAULT_POLICY, ...parsed } : DEFAULT_VAULT_POLICY;
  } catch {
    return DEFAULT_VAULT_POLICY;
  }
}

export function saveVaultPolicy(policy: VaultFleetPolicy) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(policy));
}

export function validateVaultPolicy(policy: VaultFleetPolicy): string[] {
  const errors: string[] = [];
  if (!policy.ownerPrincipal.trim()) errors.push("Owner identity is required.");
  if (policy.diskNumber == null || !policy.diskUniqueId) errors.push("Select and pin a test disk before deployment.");
  const users = policy.groups.flatMap(group => group.users.map(user => user.toLowerCase()));
  if (new Set(users).size !== users.length) errors.push("A test user may belong to only one access group.");
  if (policy.groups.some(group => group.users.length === 0)) errors.push("Every access group needs at least one standard user.");
  if (policy.volumes.some(volume => !volume.driveLetter.match(/^[A-Z]$/))) errors.push("Every volume needs one A-Z drive letter.");
  if (policy.volumes.some(volume => volume.driveLetter === "C" || volume.driveLetter === "D")) errors.push("C: and D: are protected and cannot be assigned to a Vault volume.");
  if (new Set(policy.volumes.map(volume => volume.driveLetter)).size !== policy.volumes.length) errors.push("Drive letters must be unique.");
  const deviceDiskPattern = /harddisk(\d+)/i;
  if (policy.diskNumber != null && policy.volumes.some(volume => {
    const match = volume.kind === "partition" ? volume.backing.match(deviceDiskPattern) : null;
    return match && Number(match[1]) !== policy.diskNumber;
  })) errors.push("Every raw device path must belong to the pinned test disk.");
  if (!policy.volumes.some(volume => volume.allowedGroups.includes("partner") && volume.kind === "dual")) errors.push("A Partner-accessible decoy/hidden pair is required.");
  if (!policy.volumes.some(volume => volume.ownerOnly && volume.kind === "dual")) errors.push("An owner-only decoy/hidden pair is required.");
  return errors;
}

export function buildVaultMatrix(policy: VaultFleetPolicy): VaultMatrixRow[] {
  const principals: VaultMatrixRow["userClass"][] = ["owner", "accounting", "sales", "partner"];
  return policy.volumes.flatMap(volume => principals.map(userClass => {
    const owner = userClass === "owner";
    const allowed = owner || (!volume.ownerOnly && volume.allowedGroups.includes(userClass));
    return {
      userClass,
      volume: volume.label,
      canSeeBacking: allowed,
      canMount: allowed,
      canDecrypt: allowed,
      canAccessContent: allowed,
      seesOtherSessionMount: owner,
    };
  }));
}

export function updateVolume(
  volumes: VaultVolumePolicy[],
  id: string,
  patch: Partial<VaultVolumePolicy>,
) {
  return volumes.map(volume => volume.id === id ? { ...volume, ...patch } : volume);
}
