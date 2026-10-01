import { expect, test } from 'bun:test';
import { settleStartupReveal, shouldKeepStartupAnimationVisible } from './revealBudget';

test('stalled native reveal releases its consumer within a bounded wait', async () => {
    let finish!: (shown: boolean) => void;
    const native = new Promise<boolean>(resolve => { finish = resolve; });
    const result = await settleStartupReveal(native, 5);
    expect(result).toBeNull();
    finish(true);
    expect(result).toBeNull();
});

test('rejected activation can recover without pretending the window was shown', async () => {
    expect(await settleStartupReveal(Promise.reject(new Error('unavailable')))).toBeNull();
});

test('suppressed and successful launches preserve the native visibility result', async () => {
    expect(await settleStartupReveal(Promise.resolve(false))).toBe(false);
    expect(await settleStartupReveal(Promise.resolve(true))).toBe(true);
});

test('an unconfirmed reveal keeps the rendered splash animated for a late handoff', () => {
    expect(shouldKeepStartupAnimationVisible(true)).toBe(true);
    expect(shouldKeepStartupAnimationVisible(null)).toBe(true);
    expect(shouldKeepStartupAnimationVisible(false)).toBe(false);
});
