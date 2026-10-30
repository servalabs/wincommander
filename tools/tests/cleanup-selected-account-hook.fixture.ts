// Run in a separate Bun process: mocks a minimal hook runtime and the backend,
// while exercising the real useCleanupScan action handlers. No Windows tasks.
import { mock, expect, test } from 'bun:test';

const realReact = await import('react');
const cells: unknown[] = [];
let cursor = 0;
let firstRender = true;
let effects: Array<() => unknown> = [];
mock.module('react', () => ({
  ...realReact,
  useState(initial: unknown) {
    const slot = cursor++;
    if (!(slot in cells)) cells[slot] = typeof initial === 'function' ? (initial as () => unknown)() : initial;
    return [cells[slot], (next: unknown) => { cells[slot] = typeof next === 'function' ? (next as (value: unknown) => unknown)(cells[slot]) : next; }];
  },
  useRef(initial: unknown) {
    const slot = cursor++;
    if (!(slot in cells)) cells[slot] = { current: initial };
    return cells[slot];
  },
  useMemo(fn: () => unknown) { return fn(); },
  useEffect(fn: () => unknown) { if (firstRender) effects.push(fn); },
}));

const current = 'S-1-5-21-100-200-300-1001';
const other = 'S-1-5-21-100-200-300-1002';
const another = 'S-1-5-21-100-200-300-1003';
let rows: Array<Record<string, unknown>> = [];
const calls: unknown[][] = [];
let approved = false;
let requestedConfirmation = '';
let responseOwner = other;
let confirmationWaiter: Promise<boolean> | null = null;
let mutationWaiter: Promise<void> | null = null;
const backend = {
  getUserProfiles: async () => ({ success: true, data: { profiles: [
    { name: 'Self', sid: current, path: 'C:\\Users\\Self', isCurrent: true },
    { name: 'Other', sid: other, path: 'C:\\Users\\Other', isCurrent: false },
  ], currentUser: 'Self', currentSid: current, isAdmin: true } }),
  getAutoEraseSchedules: async () => ({ success: true, data: { schedules: rows } }),
  getCleanupSummaryAllUsers: async (ids: string[], targets: string[]) => {
    calls.push(['scan', ids, targets]);
    const id = ids[0];
    const name = id === 'sshState' ? 'SSH known_hosts' : id === 'wslData' ? 'Linux distribution' : id === 'dockerDesktopData' ? 'Docker disk' : id === 'virtualMachineArtifacts' ? 'Virtual machine' : id === 'credentialManager' ? 'Credential storage artifact' : 'Password manager cache';
    return { success: true, data: { users: [{ sid: other, username: 'Other', categories: [{ id, count: 1, partial: true, status: 'partial', message: 'Selected locations only.', records: [{ name, path: 'C:\\Users\\Other\\fixture' }] }] }] } };
  },
  setMultiUserAutoEraseSchedule: async (...args: unknown[]) => {
    calls.push(['set', ...args]);
    if (mutationWaiter) await mutationWaiter;
    rows = [{ categoryId: 'browserFootprints', state: 'Ready', enabled: true, scope: 'other-user', targetUser: 'MACHINE\\Other', ownerSid: responseOwner, intervalMinutes: 60, payloadVerified: true, managedByAutoSet: false, legacy: false }];
    return { success: true, data: { status: 'ok', total: 1, results: [{ status: 'enabled', intervalMinutes: 60 }] } };
  },
  removeMultiUserAutoEraseSchedule: async (...args: unknown[]) => {
    calls.push(['remove', ...args]);
    rows = [{ categoryId: 'browserFootprints', state: 'Ready', enabled: true, scope: 'other-user', targetUser: 'MACHINE\\Another', ownerSid: another, intervalMinutes: 60, payloadVerified: true, managedByAutoSet: false, legacy: false }];
    return { success: true, data: { status: 'disabled', removed: ['synthetic'] } };
  },
};
mock.module('../../src/hooks/useBackend', () => ({ default: () => backend }));
mock.module('../../src/hooks/queries/useSettingsQuery', () => ({ useSettingsQuery: () => ({}), usePatchSettings: () => ({ mutate() {} }) }));
mock.module('../../src/components/shared/AppConfirmDialog', () => ({ useAppConfirm: () => async (options: { description: string }) => { requestedConfirmation = options.description; return confirmationWaiter ?? approved; } }));
mock.module('../../src/utils/toast', () => ({ showSuccess() {}, showError() {} }));
mock.module('../../src/panels/maintenance/diskCleanupScheduleState', () => ({ invalidateDiskCleanupScheduleStatus() {}, loadDiskCleanupSchedules: (load: () => unknown) => load() }));

test('selected-account actions require consent and verify the exact owner', async () => {
const { useCleanupScan } = await import('../../src/panels/cleanup/useCleanupScan');
const render = () => { cursor = 0; const value = useCleanupScan({ schedulesEnabled: true, entitlementsReady: true, migrationEnabled: false }); firstRender = false; return value; };
let hook = render();
for (const effect of effects) effect();
effects = [];
await new Promise(resolve => setTimeout(resolve, 0));
hook = render();
expect(hook.scheduleState).toBe('verified');
hook.handleSwitchUser(other);
hook = render();
expect(hook.selectedOwnerSid).toBe(other);
expect(hook.otherScheduleState).toBe('verified');

await hook.loadUserView(other, ['sshState']);
expect(calls.shift()).toEqual(['scan', ['sshState'], [other]]);
hook = render();
expect(hook.otherUserDataMap.sshState.count).toBe(-1);
expect(hook.otherUserDataMap.sshState.raw).toMatchObject({ partial: true, records: [{ name: 'SSH known_hosts' }] });
await hook.handleOtherUserClear({ id: 'sshState' } as never);
await hook.handleCardClearAllUsers({ id: 'sshState' } as never);
expect(calls).toHaveLength(0);
await hook.loadUserView(other, ['passwordManagerCaches']);
expect(calls.shift()).toEqual(['scan', ['passwordManagerCaches'], [other]]);
hook = render();
expect(hook.otherUserDataMap.passwordManagerCaches.count).toBe(-1);
expect(hook.otherUserDataMap.passwordManagerCaches.raw).toMatchObject({ partial: true, records: [{ name: 'Password manager cache' }] });
await hook.handleOtherUserClear({ id: 'passwordManagerCaches' } as never);
await hook.handleCardClearAllUsers({ id: 'passwordManagerCaches' } as never);
expect(calls).toHaveLength(0);
await hook.loadUserView(other, ['wslData']);
expect(calls.shift()).toEqual(['scan', ['wslData'], [other]]);
hook = render();
expect(hook.otherUserDataMap.wslData.count).toBe(-1);
expect(hook.otherUserDataMap.wslData.raw).toMatchObject({ partial: true, records: [{ name: 'Linux distribution' }] });
await hook.handleOtherUserClear({ id: 'wslData' } as never);
await hook.handleCardClearAllUsers({ id: 'wslData' } as never);
expect(calls).toHaveLength(0);
await hook.loadUserView(other, ['dockerDesktopData']);
expect(calls.shift()).toEqual(['scan', ['dockerDesktopData'], [other]]);
hook = render();
expect(hook.otherUserDataMap.dockerDesktopData.count).toBe(-1);
expect(hook.otherUserDataMap.dockerDesktopData.raw).toMatchObject({ partial: true, records: [{ name: 'Docker disk' }] });
await hook.handleOtherUserClear({ id: 'dockerDesktopData' } as never);
await hook.handleCardClearAllUsers({ id: 'dockerDesktopData' } as never);
expect(calls).toHaveLength(0);
await hook.loadUserView(other, ['virtualMachineArtifacts']);
expect(calls.shift()).toEqual(['scan', ['virtualMachineArtifacts'], [other]]);
hook = render();
expect(hook.otherUserDataMap.virtualMachineArtifacts.count).toBe(-1);
expect(hook.otherUserDataMap.virtualMachineArtifacts.raw).toMatchObject({ partial: true, records: [{ name: 'Virtual machine' }] });
await hook.handleOtherUserClear({ id: 'virtualMachineArtifacts' } as never);
await hook.handleCardClearAllUsers({ id: 'virtualMachineArtifacts' } as never);
expect(calls).toHaveLength(0);
await hook.loadUserView(other, ['credentialManager']);
expect(calls.shift()).toEqual(['scan', ['credentialManager'], [other]]);
hook = render();
expect(hook.otherUserDataMap.credentialManager.count).toBe(-1);
expect(hook.otherUserDataMap.credentialManager.raw).toMatchObject({ partial: true, records: [{ name: 'Credential storage artifact' }] });
await hook.handleOtherUserClear({ id: 'credentialManager' } as never);
await hook.handleCardClearAllUsers({ id: 'credentialManager' } as never);
expect(calls).toHaveLength(0);

let releaseConfirmation!: (value: boolean) => void;
confirmationWaiter = new Promise(resolve => { releaseConfirmation = resolve; });
const pendingSchedule = hook.handleSetSchedule('browserFootprints', 60, false);
hook = render();
hook.handleSwitchUser(current);
hook = render();
expect(hook.selectedUser).toBe(other);
releaseConfirmation(false);
expect(await pendingSchedule).toBe(false);
confirmationWaiter = null;
hook = render();

confirmationWaiter = new Promise(resolve => { releaseConfirmation = resolve; });
const pendingClear = hook.handleOtherUserClear({ id: 'browserFootprints', label: 'Browser' } as never);
hook = render();
hook.handleSwitchUser(current);
hook = render();
expect(hook.selectedUser).toBe(other);
releaseConfirmation(false);
await pendingClear;
confirmationWaiter = null;
hook = render();
expect(calls).toHaveLength(0);

expect(await hook.handleSetSchedule('browserFootprints', 60, false)).toBe(false);
expect(requestedConfirmation).toContain(other);
expect(calls).toHaveLength(0);
approved = true;
let releaseMutation!: () => void;
mutationWaiter = new Promise(resolve => { releaseMutation = resolve; });
const scheduledMutation = hook.handleSetSchedule('browserFootprints', 60, false);
await new Promise(resolve => setTimeout(resolve, 0));
hook = render();
hook.handleSwitchUser(current);
hook = render();
expect(hook.selectedUser).toBe(other);
releaseMutation();
expect(await scheduledMutation).toBe(true);
mutationWaiter = null;
expect(calls).toEqual([['set', 'browserFootprints', 60, [other], false, true]]);
hook = render();
expect(hook.otherScheduleSnapshot?.records.browserFootprints?.ownerSid).toBe(other);
expect(await hook.handleClearSchedule('browserFootprints')).toBe(true);
expect(calls[1]).toEqual(['remove', 'browserFootprints', [other]]);
hook = render();
expect(hook.otherScheduleSnapshot?.records.browserFootprints).toBeUndefined();
responseOwner = another;
expect(await hook.handleSetSchedule('browserFootprints', 60, false)).toBe(false);
expect(calls[2]).toEqual(['set', 'browserFootprints', 60, [other], false, true]);
hook = render();
hook.handleSwitchUser('__all__');
hook = render();
expect(await hook.handleSetSchedule('browserFootprints', 60, false)).toBe(false);
expect(await hook.handleClearSchedule('browserFootprints')).toBe(false);
expect(calls).toHaveLength(3);
});
