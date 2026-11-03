import { expect, mock, test } from 'bun:test';

const realReact = await import('react');
const cells: unknown[] = [];
let cursor = 0;
let initialRender = true;
const effects: Array<() => unknown> = [];
mock.module('react', () => ({
  ...realReact,
  useState(initial: unknown) {
    const index = cursor++;
    if (!(index in cells)) cells[index] = typeof initial === 'function' ? (initial as () => unknown)() : initial;
    return [cells[index], (next: unknown) => { cells[index] = typeof next === 'function' ? (next as (value: unknown) => unknown)(cells[index]) : next; }];
  },
  useRef(initial: unknown) {
    const index = cursor++;
    if (!(index in cells)) cells[index] = { current: initial };
    return cells[index];
  },
  useMemo(fn: () => unknown) { return fn(); },
  useEffect(fn: () => unknown) { if (initialRender) effects.push(fn); },
}));

const notices: Array<[string, string]> = [];
let migrationCalls = 0;
let removalDenied = false;
let retainRemovedTask = false;
let rows: unknown[] = [];
const backend = {
  error: 'Access to the synthetic Application Data junction is denied.',
  getUserProfiles: async () => ({ success: true, data: { profiles: [{ name: 'Self', sid: 'S-1-5-21-1-2-3-1001', path: 'C:\\Users\\Self', isCurrent: true }], currentUser: 'Self', currentSid: 'S-1-5-21-1-2-3-1001', isAdmin: true } }),
  getAutoEraseSchedules: async () => ({ success: true, data: { schedules: rows } }),
  invokeAutoEraseMigration: async () => { migrationCalls++; return { success: false, error: 'An unrelated schedule upgrade was denied.' }; },
  removeAutoEraseSchedule: async () => {
    if (removalDenied) return { success: false, error: 'The selected task could not be removed.' };
    if (!retainRemovedTask) rows = [];
    return { success: true, data: { status: 'disabled' } };
  },
};
mock.module('../../src/hooks/useBackend', () => ({ default: () => backend }));
mock.module('../../src/hooks/queries/useSettingsQuery', () => ({ useSettingsQuery: () => ({}), usePatchSettings: () => ({ mutate() {} }) }));
mock.module('../../src/components/shared/AppConfirmDialog', () => ({ useAppConfirm: () => async () => true }));
mock.module('../../src/utils/toast', () => ({ showSuccess: (message: string) => notices.push(['success', message]), showError: (message: string) => notices.push(['error', message]) }));
mock.module('../../src/panels/maintenance/diskCleanupScheduleState', () => ({ invalidateDiskCleanupScheduleStatus() {}, loadDiskCleanupSchedules: (load: () => unknown) => load() }));

test('schedule removal verifies its own outcome without retrying unrelated migration or broadcasting scan errors', async () => {
  const { useCleanupScan } = await import('../../src/panels/cleanup/useCleanupScan');
  const render = () => {
    cursor = 0;
    const hook = useCleanupScan({ schedulesEnabled: true, entitlementsReady: true, migrationEnabled: true });
    initialRender = false;
    return hook;
  };
  let hook = render();
  for (const effect of effects) effect();
  await new Promise(resolve => setTimeout(resolve, 0));
  hook = render();
  expect(migrationCalls).toBe(1);
  expect(notices).toEqual([['error', 'An unrelated schedule upgrade was denied.']]);
  notices.length = 0;
  expect(await hook.handleClearSchedule('usageTraceLogs')).toBe(true);
  expect(migrationCalls).toBe(1);
  expect(notices).toEqual([['success', 'Auto-clean turned off for Usage Trace Logs']]);
  notices.length = 0;
  removalDenied = true;
  expect(await hook.handleClearSchedule('usageTraceLogs')).toBe(false);
  expect(notices).toEqual([['error', 'The selected task could not be removed.']]);
  removalDenied = false;
  retainRemovedTask = true;
  rows = [{ categoryId: 'usageTraceLogs', state: 'Ready', enabled: true, scope: 'system', targetUser: null, intervalMinutes: 1440, payloadVerified: true, managedByAutoSet: true }];
  notices.length = 0;
  expect(await hook.handleClearSchedule('usageTraceLogs')).toBe(false);
  expect(notices).toEqual([['error', 'Usage Trace Logs: schedule removal could not be confirmed. Check Task Scheduler and retry.']]);
  expect(migrationCalls).toBe(1);
});
