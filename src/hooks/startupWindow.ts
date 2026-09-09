import { invoke } from '@tauri-apps/api/core';
import { applyStartupTheme, resolveStartupTheme } from '../lib/startupTheme';

export async function prepareStartupTheme(): Promise<void> {
    let cached: string | null = null;
    try { cached = localStorage.getItem('wc-theme'); } catch { /* Optional cache. */ }
    const system = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
    let timer: ReturnType<typeof setTimeout> | undefined;
    try {
        const saved = await Promise.race([
            invoke<unknown>('get_setting', { path: 'app.theme' }),
            new Promise<undefined>(resolve => { timer = setTimeout(resolve, 1500); }),
        ]).catch(() => undefined);
        applyStartupTheme(resolveStartupTheme(saved, cached, system));
    } finally {
        if (timer) clearTimeout(timer);
    }
}

export function revealStartupWindow(): Promise<boolean> {
    return invoke<boolean>('startup_window_ready', {
        isLight: document.documentElement.classList.contains('light'),
    });
}
