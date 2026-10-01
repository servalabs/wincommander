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
