/** Shared Windows-user groups used by Fleet feature policies. */
export interface FleetAccessUser {
  id: string;
  username: string;
  displayName?: string;
  sid?: string;
  isCurrent?: boolean;
  /** A successful Windows discovery did not return this account. Keep its
   * historical memberships visible to validation, but never offer it for a
   * new assignment. */
  isAvailable?: boolean;
}

export interface FleetAccessGroup {
  id: string;
  name: string;
  localGroup: string;
  userIds: string[];
}

export interface FleetAccessDirectory {
  schema: 1;
  users: FleetAccessUser[];
  groups: FleetAccessGroup[];
}

/** Frozen `svc.vault.reconcile_access_groups` wire shape — one row per Windows local group. */
export interface AccessGroupReconcileRequest {
  local_group: string;
  member_sids: string[];
}

export type AccessGroupReconcileState = "created" | "updated" | "unchanged" | "failed";

export interface AccessGroupReconcileResult {
  local_group: string;
  state: AccessGroupReconcileState;
  error: string | null;
}

export interface AccessGroupReconcileResponse {
  results: AccessGroupReconcileResult[];
}
