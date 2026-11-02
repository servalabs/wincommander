import type { AuthMode } from '../../context/AuthModeContext';

/** Apply only recognized native PIN outcomes. A wrong PIN has no side effects. */
export async function applyCalculatorPinResult(
    result: string,
    unlock: (mode: AuthMode) => Promise<void>,
    destroy: () => Promise<void>,
): Promise<void> {
    if (result === 'real' || result === 'open') await unlock('real');
    else if (result === 'decoy') await unlock('decoy');
    else if (result === 'destroy') await destroy();
}
