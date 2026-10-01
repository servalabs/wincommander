import { useSyncExternalStore } from 'react';

const subscribe = (changed: () => void) => {
    document.addEventListener('visibilitychange', changed);
    return () => document.removeEventListener('visibilitychange', changed);
};

export function usePageVisible(): boolean {
    return useSyncExternalStore(subscribe, () => document.visibilityState === 'visible', () => false);
}
