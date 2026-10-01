import { waitForSoftTimeout } from '../lib/softTimeout';

/** Window activation may finish late; it must not strand application initialization. */
export async function settleStartupReveal(request: Promise<boolean>, timeoutMs = 6_000): Promise<boolean | null> {
    try {
        const result = await waitForSoftTimeout(request, timeoutMs);
        return result.status === 'completed' ? result.value : null;
    } catch {
        return null;
    }
}

/** A missing acknowledgement is not evidence that the native window is hidden. */
export function shouldKeepStartupAnimationVisible(revealResult: boolean | null): boolean {
    return revealResult !== false;
}
