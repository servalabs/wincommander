import { expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { ALL_CATEGORIES } from '../../panels/cleanup/cleanupCategories';
import { isOtherAccountScheduleOnly, OTHER_ACCOUNT_SCHEDULE_ONLY_IDS, OTHER_ACCOUNT_READ_ONLY_IDS } from '../../panels/cleanup/cleanupOtherAccountScheduleOnly';
import CleanupTraceCard from './CleanupTraceCard';

test('protected categories offer read-only scans for one selected account', () => {
  expect(OTHER_ACCOUNT_SCHEDULE_ONLY_IDS.size).toBe(0);
  for (const id of OTHER_ACCOUNT_READ_ONLY_IDS) {
    const category = ALL_CATEGORIES.find(item => item.id === id)!;
    expect(category).not.toBe(undefined);
    expect(isOtherAccountScheduleOnly(category, 'S-1-5-21-100-200-300-1002', false)).toBe(false);
    const html = renderToStaticMarkup(<CleanupTraceCard category={category} count={-1} preview={[]} loading={false} clearing={false} scheduleState="verified" onSetSchedule={() => true} onLoad={() => {}} />);
    expect(html).toContain(`Schedule a wipe for ${category.label}`);
    expect(html).toContain(`Scan ${category.label}`);
    expect(html).not.toContain(`Clear ${category.label}`);
  }
  const browser = ALL_CATEGORIES.find(item => item.id === 'browserFootprints')!;
  expect(isOtherAccountScheduleOnly(browser, 'S-1-5-21-100-200-300-1002', false)).toBe(false);
  expect(OTHER_ACCOUNT_READ_ONLY_IDS.has('sshState')).toBe(true);
  expect(OTHER_ACCOUNT_READ_ONLY_IDS.has('passwordManagerCaches')).toBe(true);
  expect(OTHER_ACCOUNT_READ_ONLY_IDS.has('wslData')).toBe(true);
  expect(OTHER_ACCOUNT_READ_ONLY_IDS.has('dockerDesktopData')).toBe(true);
  expect(OTHER_ACCOUNT_READ_ONLY_IDS.has('virtualMachineArtifacts')).toBe(true);
  expect(OTHER_ACCOUNT_READ_ONLY_IDS.has('credentialManager')).toBe(true);
});
