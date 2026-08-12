import React, { createContext, useContext, useEffect, useState, ReactNode, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';

type Theme = 'dark' | 'light';

interface ThemeContextType {
    theme: Theme;
    toggleTheme: () => void;
    setTheme: (theme: Theme) => void;
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined);

interface ThemeProviderProps {
    children: ReactNode;
}

const THEME_STORAGE_KEY = 'wc-theme';

// Read the cached theme synchronously so initial render matches the user's
// choice. The inline script in index.html already applied the class to <html>
// before React mounted; this just keeps React state in sync with that.
function readCachedTheme(): Theme {
    try {
        const cached = localStorage.getItem(THEME_STORAGE_KEY);
        if (cached === 'dark' || cached === 'light') return cached;
    } catch { /* localStorage may be unavailable */ }
    return 'dark';
}

export const ThemeProvider: React.FC<ThemeProviderProps> = ({ children }) => {
    const [theme, setThemeState] = useState<Theme>(readCachedTheme);

    // Hydrate from settings.json (authoritative) — corrects the cache if it drifted.
    useEffect(() => {
        invoke<string>('get_setting', { path: 'app.theme' })
            .then((settingsTheme) => {
                if (settingsTheme === 'dark' || settingsTheme === 'light') {
                    setThemeState(settingsTheme);
                    try { localStorage.setItem(THEME_STORAGE_KEY, settingsTheme); } catch { /* */ }
                }
            })
            .catch(() => { });
    }, []);

    useEffect(() => {
        const root = window.document.documentElement;
        root.classList.remove('light', 'dark');
        root.classList.add(theme);
        // V2: the named theme attribute drives the V2 token blocks and is
        // forward-compatible with adding Palantir/Vault later. Light ↔ Daylight,
        // dark ↔ Anduril (the warm-orange default).
        root.setAttribute('data-theme', theme === 'light' ? 'daylight' : 'anduril');
    }, [theme]);

    // Cross-window sync: when the main window changes theme it writes to
    // localStorage; the overlay window picks it up via the `storage` event
    // (fired in other same-origin windows of the same Tauri app).
    useEffect(() => {
        const onStorage = (e: StorageEvent) => {
            if (e.key !== THEME_STORAGE_KEY) return;
            const next = e.newValue;
            if (next === 'dark' || next === 'light') {
                setThemeState(next);
            }
        };
        window.addEventListener('storage', onStorage);
        return () => window.removeEventListener('storage', onStorage);
    }, []);

    const persist = useCallback((newTheme: Theme) => {
        try { localStorage.setItem(THEME_STORAGE_KEY, newTheme); } catch { /* */ }
        invoke('patch_settings_cmd', { patch: { app: { theme: newTheme } } }).catch(() => { });
    }, []);

    const toggleTheme = () => {
        setThemeState(prev => {
            const next = prev === 'dark' ? 'light' : 'dark';
            persist(next);
            return next;
        });
    };

    const setTheme = (newTheme: Theme) => {
        setThemeState(newTheme);
        persist(newTheme);
    };

    return (
        <ThemeContext.Provider value={{ theme, toggleTheme, setTheme }}>
            {children}
        </ThemeContext.Provider>
    );
};

export const useTheme = (): ThemeContextType => {
    const context = useContext(ThemeContext);
    if (!context) {
        throw new Error('useTheme must be used within a ThemeProvider');
    }
    return context;
};
