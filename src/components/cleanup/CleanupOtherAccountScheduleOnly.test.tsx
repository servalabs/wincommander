import { expect, test } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { ALL_CATEGORIES } from '../../panels/cleanup/cleanupCategories';
import { isOtherAccountScheduleOnly, OTHER_ACCOUNT_SCHEDULE_ONLY_IDS } from '../../panels/cleanup/cleanupOtherAccountScheduleOnly';
import CleanupTraceCard from './CleanupTraceCard';

test('six protected categories have schedule-only cards for one selected account', () => {
  expect(OTHER_ACCOUNT_SCHEDULE_ONLY_IDS.size).toBe(6);
  for (const id of OTHER_ACCOUNT_SCHEDULE_ONLY_IDS) {
    const category = ALL_CATEGORIES.find(item => item.id === id)!;
    expect(category).not.toBe(undefined);
    expect(isOtherAccountScheduleOnly(category, 'S-1-5-21-100-200-300-1002', false)).toBe(true);
    expect(isOtherAccountScheduleOnly(category, null, false)).toBe(false);
    expect(isOtherAccountScheduleOnly(category, 'S-1-5-21-100-200-300-1002', true)).toBe(false);
    const html = renderToStaticMarkup(<CleanupTraceCard category={category} count={-1} preview={[]} loading={false} clearing={false} scheduleOnly scheduleState="verified" onSetSchedule={() => true} />);
    expect(html).toContain('data-cleanup-state="schedule-only"');
    expect(html).toContain('Records cannot be scanned or cleared for this account here');
    expect(html).toContain(`Schedule a wipe for ${category.label}`);
    expect(html).not.toContain(`Scan ${category.label}`);
    expect(html).not.toContain(`Clear ${category.label}`);
    expect(html).not.toContain(`View details for ${category.label}`);
  }
  const browser = ALL_CATEGORIES.find(item => item.id === 'browserFootprints')!;
  expect(isOtherAccountScheduleOnly(browser, 'S-1-5-21-100-200-300-1002', false)).toBe(false);
});
