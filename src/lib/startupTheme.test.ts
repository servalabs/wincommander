import { describe, expect, test } from 'bun:test';
import { resolveStartupTheme } from './startupTheme';

describe('launch theme shared by splash and dashboard', () => {
    test('saved user preference wins over stale browser cache and system', () => {
        expect(resolveStartupTheme('dark', 'light', 'light')).toBe('dark');
        expect(resolveStartupTheme('light', 'dark', 'dark')).toBe('light');
    });
    test('fresh system preference wins over a previous cached system color', () => {
        expect(resolveStartupTheme('system', 'dark', 'light')).toBe('light');
        expect(resolveStartupTheme('system', 'light', 'dark')).toBe('dark');
    });
    test('unavailable settings use cached preference then system', () => {
        expect(resolveStartupTheme(undefined, 'dark', 'light')).toBe('dark');
        expect(resolveStartupTheme(undefined, null, 'light')).toBe('light');
    });
});
