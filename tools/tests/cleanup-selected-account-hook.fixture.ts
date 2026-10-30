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
const backend = {
  getUserProfiles: async () => ({ success: true, data: { profiles: [
    { name: 'Self', sid: current, path: 'C:\\Users\\Self', isCurrent: true },
    { name: 'Other', sid: other, path: 'C:\\Users\\Other', isCurrent: false },
  ], currentUser: 'Self', currentSid: current, isAdmin: true } }),
  getAutoEraseSchedules: async () => ({ success: true, data: { schedules: rows } }),
  setMultiUserAutoEraseSchedule: async (...args: unknown[]) => {
    calls.push(['set', ...args]);
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
mock.module('../../src/components/shared/AppConfirmDialog', () => ({ useAppConfirm: () => async (options: { description: string }) => { requestedConfirmation = options.description; return approved; } }));
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

expect(await hook.handleSetSchedule('browserFootprints', 60, false)).toBe(false);
expect(requestedConfirmation).toContain(other);
expect(calls).toHaveLength(0);
approved = true;
expect(await hook.handleSetSchedule('browserFootprints', 60, false)).toBe(true);
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
