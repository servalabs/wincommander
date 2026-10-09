import { describe, expect, it } from 'bun:test';
import { shouldRecoverPortGuard } from './portGuardRecovery';

describe('Port Guard recovery', () => {
  it('restores a saved passive watch after collector exit', () => {
    expect(shouldRecoverPortGuard({ schemaVersion: 2, desiredEnabled: true, running: false })).toBe(true);
  });
  it('does not rearm a user-disabled monitor or duplicate a live collector', () => {
    expect(shouldRecoverPortGuard({ schemaVersion: 2, desiredEnabled: false, running: false })).toBe(false);
    expect(shouldRecoverPortGuard({ schemaVersion: 2, desiredEnabled: true, running: true })).toBe(false);
  });
  it('does not open legacy decoy listeners on startup', () => {
    expect(shouldRecoverPortGuard({ running: false })).toBe(false);
    expect(shouldRecoverPortGuard({ schemaVersion: 1, desiredEnabled: true, running: false })).toBe(false);
  });
});
