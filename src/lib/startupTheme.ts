export type AppTheme = 'dark' | 'light';
let startupTheme: AppTheme | null = null;

export function resolveStartupTheme(saved: unknown, cached: unknown, system: AppTheme): AppTheme {
    if (saved === 'dark' || saved === 'light') return saved;
    if (saved === 'system') return system;
    return cached === 'dark' || cached === 'light' ? cached : system;
}

export function getStartupTheme(): AppTheme | null { return startupTheme; }

export function applyStartupTheme(theme: AppTheme): void {
    startupTheme = theme;
    const root = document.documentElement;
    root.classList.remove('light', 'dark');
    root.classList.add(theme);
    root.setAttribute('data-theme', theme === 'light' ? 'daylight' : 'anduril');
    try { localStorage.setItem('wc-theme', theme); } catch { /* Optional cache. */ }
}
