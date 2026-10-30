import { describe, expect, test } from 'bun:test';
import { createCleanupVerification, retainCleanupVerification } from './cleanupVerification';
import { reconcileCleanupClear } from './useCleanupScan';
import { cleanupCoverageLimitation, cleanupVerificationText } from './cleanupDisplayModel';

describe('privacy-safe cleanup verification history', () => {
  test('stores only status and time for guarded outcomes', () => {
    const at = '2026-10-28T12:00:00.000Z';
    for (const status of ['cleared', 'reduced', 'reset', 'unchanged', 'deferred', 'failed'] as const) {
      expect(createCleanupVerification(status, at)).toEqual({ status, verifiedAt: at });
    }
    expect(createCleanupVerification(reconcileCleanupClear(15, 0, false).result, at).status).toBe('failed');
  });

  test('a later scan retains the last verification without changing new scan data', () => {
    const verification = createCleanupVerification('reset', '2026-10-28T12:00:00.000Z');
    const next = { count: 2, items: ['new sample'], verification: undefined };
    expect(retainCleanupVerification(next, { verification })).toEqual({ ...next, verification });
    const failedScan = { count: -1, error: 'Unavailable', verification: undefined };
    expect(retainCleanupVerification(failedScan, { verification })).toEqual({ ...failedScan, verification });
  });

  test('a new guarded outcome replaces the old verification', () => {
    const previous = { verification: createCleanupVerification('cleared', '2026-10-27T12:00:00.000Z') };
    const verification = createCleanupVerification('failed', '2026-10-28T12:00:00.000Z');
    expect(retainCleanupVerification({ verification }, previous).verification).toEqual(verification);
  });

  test('an ordinary scan does not invent a verification result', () => {
    expect(retainCleanupVerification({ verification: undefined })).toEqual({ verification: undefined });
    expect(createCleanupVerification('deferred').verifiedAt.length > 0).toBe(true);
  });

  test('retains the Wi-Fi coverage limitation in the verification message', () => {
    const outcome = reconcileCleanupClear(2, 0, true, true);
    const text = cleanupVerificationText(createCleanupVerification(outcome.result));
    expect(text).toContain('limited coverage');
    expect(text).not.toContain('no remaining items');
  });

  test('uses the actual scan coverage and category-specific explanation', () => {
    const docker = { coverage: 'limited', coverageNote: 'Customized disk locations are not included.' };
    const vm = { coverage: 'limited', coverageNote: 'Relocated machines were not inspected.' };
    expect(cleanupCoverageLimitation('dockerDesktopData', docker, false)).toBe(docker.coverageNote);
    expect(cleanupCoverageLimitation('virtualMachineArtifacts', vm, false)).toBe(vm.coverageNote);
    expect(cleanupCoverageLimitation('wlanProfiles', {}, true)).toContain('Private or policy-managed');
    expect(cleanupCoverageLimitation('dnsCache', {}, false)).toBeUndefined();
    expect(reconcileCleanupClear(2, 0, true, !!cleanupCoverageLimitation('dockerDesktopData', docker, false)).result).toBe('limited');
  });
});
