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

function applyThemeClass(theme: Theme) {
    const root = window.document.documentElement;
    root.classList.remove('light', 'dark');
    root.classList.add(theme);
    root.setAttribute('data-theme', theme === 'light' ? 'daylight' : 'anduril');
}

// Read the cached theme synchronously so initial render matches the user's
// choice. The inline script in index.html already applied the class to <html>
// before React mounted; this just keeps React state in sync with that.
function readCachedTheme(): Theme {
    try {
        const cached = localStorage.getItem(THEME_STORAGE_KEY);
        if (cached === 'dark' || cached === 'light') return cached;
    } catch { /* localStorage may be unavailable */ }
    return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

export const ThemeProvider: React.FC<ThemeProviderProps> = ({ children }) => {
    const [theme, setThemeState] = useState<Theme>(readCachedTheme);
    const [themeReady, setThemeReady] = useState(false);

    // The splash must start only after the persisted setting has won over a
    // stale WebView cache. Otherwise it starts light, is corrected to dark,
    // and visibly restarts mid-animation in packaged builds.
    useEffect(() => {
        invoke<string>('get_setting', { path: 'app.theme' })
            .then((settingsTheme) => {
                const resolvedTheme = settingsTheme === 'dark' || settingsTheme === 'light'
                    ? settingsTheme
                    : readCachedTheme();
                applyThemeClass(resolvedTheme);
                setThemeState(resolvedTheme);
                try { localStorage.setItem(THEME_STORAGE_KEY, resolvedTheme); } catch { /* */ }
            })
            .catch(() => applyThemeClass(readCachedTheme()))
            .finally(() => {
                window.document.documentElement.setAttribute('data-theme-ready', 'true');
                setThemeReady(true);
            });
    }, []);

    useEffect(() => {
        applyThemeClass(theme);
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
            {themeReady ? children : null}
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
