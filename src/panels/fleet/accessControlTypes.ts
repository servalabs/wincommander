/** Shared Windows-user groups used by Fleet feature policies. */
export interface FleetAccessUser {
  id: string;
  username: string;
  displayName?: string;
  sid?: string;
  isCurrent?: boolean;
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
