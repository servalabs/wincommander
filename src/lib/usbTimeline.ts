export interface UsbVolumeIdentity {
  driveLetter: string;
  instanceId: string;
}

export type UsbWindowsDeviceState = 'allowed' | 'blocked' | 'unknown';

/**
 * A PnP state is only useful when Windows supplied a concrete result.  Do not
 * turn an old timeline row or an action acknowledgement into an allowed state.
 */
export function usbWindowsDeviceState(identity: {
  problemCode?: number | null;
  pnpStatus?: string | null;
}): UsbWindowsDeviceState {
  if (identity.problemCode === 22) return 'blocked';
  if (identity.problemCode === 0 || identity.pnpStatus?.trim().toUpperCase() === 'OK') return 'allowed';
  return 'unknown';
}

export type UsbVolumeAccessState = 'writable' | 'read-only' | 'unknown';

export function usbVolumeAccessState(volume: { readOnly?: boolean | null } | undefined): UsbVolumeAccessState {
  if (volume?.readOnly === true) return 'read-only';
  if (volume?.readOnly === false) return 'writable';
  return 'unknown';
}

export function matchUsbVolume<T extends UsbVolumeIdentity>(
  entry: { category: string; attached: boolean; instanceId: string; driveLetter: string | null },
  volumes: T[],
): T | undefined {
  if (entry.category !== 'Storage' || !entry.attached || !entry.instanceId) return undefined;
  const matches = volumes.filter((volume) =>
    volume.instanceId?.toUpperCase() === entry.instanceId.toUpperCase());
  if (entry.driveLetter) {
    return matches.find((volume) => volume.driveLetter.toUpperCase() === entry.driveLetter?.toUpperCase());
  }
  return matches.length === 1 ? matches[0] : undefined;
}

export interface UsbObservedEvent {
  id: string;
  deviceKey: string;
  kind: string;
  at: number;
  observedOrder?: number;
  verified?: boolean;
  volumeLetter?: string | null;
}

export function usbEventState(event: UsbObservedEvent): string {
  switch (event.kind) {
    case 'block_applied': return event.verified ? 'Block verified' : 'Block requested';
    case 'allow_applied': return event.verified ? 'Allow verified' : 'Allow requested';
    case 'block_failed': return 'Block failed';
    case 'allow_failed': return 'Allow failed';
    case 'block_unverified': return 'Block requested';
    case 'allow_unverified': return 'Allow requested';
    case 'mounted': return 'Mount observed';
    case 'unmounted': return 'Unmount observed';
    case 'policy_decision': return 'Policy decision';
    default: return event.kind.replaceAll('_', ' ');
  }
}

export function compareUsbTimelineEvents(
  first: { at: number; sortOrder?: number },
  second: { at: number; sortOrder?: number },
): number {
  return second.at - first.at || (second.sortOrder ?? second.at * 1000) - (first.sortOrder ?? first.at * 1000);
}

export function usbPolicyReceiptVerified(receipt: unknown): boolean {
  return typeof receipt === 'object' && receipt !== null
    && 'verified' in receipt && receipt.verified === true;
}

/**
 * The read-only control changes the physical USB disk.  An acknowledgement is
 * insufficient: the receipt must prove the requested disk state and scope.
 */
export function usbReadOnlyPolicyReceiptVerified(receipt: unknown, readOnly: boolean): boolean {
  return usbPolicyReceiptVerified(receipt)
    && typeof receipt === 'object' && receipt !== null
    && 'readOnly' in receipt && receipt.readOnly === readOnly
    && 'scope' in receipt && receipt.scope === 'physicalUsbDisk'
    && 'affectedDriveLetters' in receipt
    && Array.isArray(receipt.affectedDriveLetters)
    && receipt.affectedDriveLetters.length > 0
    && receipt.affectedDriveLetters.every((letter) => typeof letter === 'string' && letter.trim().length > 0);
}

export function isCurrentUsbSession(
  session: { deviceKey: string; attachedAt: number; detachedAt: number | null; endedUnobservedAt?: number | null },
  currentKeys: ReadonlySet<string>,
  monitorStartedAt: number | null,
): boolean {
  return monitorStartedAt != null && session.attachedAt >= monitorStartedAt
    && currentKeys.has(session.deviceKey)
    && session.detachedAt == null && session.endedUnobservedAt == null;
}

export interface UsbPolicyObservation {
  time: string;
  deviceKey: string;
  friendlyName: string;
  action: string;
  enforced: boolean;
  detail: string;
}

export function usbPolicyTimelineRows(actions: UsbPolicyObservation[]) {
  const seen = new Map<string, number>();
  return actions.flatMap((action) => {
    const at = Date.parse(action.time) / 1000;
    if (!Number.isFinite(at)) return [];
    const signature = JSON.stringify([action.time, action.deviceKey, action.action, action.enforced, action.detail]);
    const occurrence = seen.get(signature) ?? 0;
    seen.set(signature, occurrence + 1);
    const state = action.action === 'quarantine'
      ? action.enforced === true ? 'Isolation verified' : 'Isolation requested / unverified'
      : action.action === 'alert' ? 'Policy: alert only'
        : action.action === 'ignore' ? 'Policy: no automatic action' : 'Policy result unavailable';
    return [{ id: `policy:${signature}:${occurrence}`, deviceKey: action.deviceKey,
      name: action.friendlyName || 'USB device', at, state, detail: action.detail }];
  });
}

export function usbMonitorPresentation(savedEnabled: boolean, running: boolean, failed: boolean) {
  return {
    label: failed ? 'Needs attention' : running ? 'Monitoring' : savedEnabled ? 'Not running' : 'Off',
    checked: savedEnabled || running,
    mismatch: savedEnabled !== running,
  };
}

export async function changeUsbPreference(
  apply: () => Promise<unknown>, save: () => Promise<unknown>, refresh: () => Promise<unknown>,
): Promise<void> {
  try {
    await apply();
    await save();
  } finally {
    await refresh();
  }
}
