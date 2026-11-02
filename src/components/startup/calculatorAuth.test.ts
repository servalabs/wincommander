import { expect, test } from 'bun:test';
import { applyCalculatorPinResult } from './calculatorAuth';

test('wrong and unrecognized PIN results leave the calculator locked', async () => {
    const actions: string[] = [];
    for (const result of ['wrong', '', 'unexpected']) {
        await applyCalculatorPinResult(result, async () => { actions.push('unlock'); }, async () => { actions.push('destroy'); });
    }
    expect(actions).toEqual([]);
});

test('only recognized PIN results request the matching action', async () => {
    const actions: string[] = [];
    const unlock = async (mode: string) => { actions.push(mode); };
    const destroy = async () => { actions.push('destroy'); };
    for (const result of ['real', 'open', 'decoy', 'destroy']) {
        await applyCalculatorPinResult(result, unlock, destroy);
    }
    expect(actions).toEqual(['real', 'real', 'decoy', 'destroy']);
});
