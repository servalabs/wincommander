import { getDisplayBranding } from '../lib/branding';

const KEY = 'wc-startup-branding';
type Branding = { companyLabel: string; productLabel: string };

export function readStartupBranding(): Branding {
    try {
        const cached = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Branding | null;
        if (cached && [cached.companyLabel, cached.productLabel].every(
            value => typeof value === 'string' && value.length > 0 && value.length <= 256,
        )) return cached;
    } catch { /* Startup presentation never depends on storage availability. */ }
    return getDisplayBranding(null);
}

export function cacheStartupBranding(branding: Branding): void {
    try { localStorage.setItem(KEY, JSON.stringify(branding)); } catch { /* Optional cache. */ }
}
