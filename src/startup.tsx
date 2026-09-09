import { showStartupAnimation, hideStartupAnimation, waitForStartupAnimationReady } from './startup/animationRoot';
import { readStartupBranding } from './startup/brandingCache';
import { applyMotionClass } from './lib/motionPolicy';
import { prepareStartupTheme, revealStartupWindow } from './hooks/startupWindow';

const native = (window as typeof window & {
    __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } };
}).__TAURI_INTERNALS__;
const label = native?.metadata?.currentWindow?.label;
const isMain = Boolean(native) && (!label || label === 'main')
    && new URLSearchParams(location.search).get('wc-window') !== 'notification-alerts';

applyMotionClass();
async function startMainWindow(): Promise<void> {
    await prepareStartupTheme();
    const initialAnimation = {
        branding: readStartupBranding(),
        isLight: document.documentElement.classList.contains('light'),
        reducedMotion: document.documentElement.classList.contains('wc-no-motion'),
        isAppReady: false,
        isWindowVisible: false,
        startupError: null,
        onComplete: () => {},
        onRetry: () => location.reload(),
    };
    showStartupAnimation(initialAnimation);
    try {
        await waitForStartupAnimationReady();
        const shown = await revealStartupWindow();
        // The intro clock starts after the real native window is revealed.
        showStartupAnimation({ ...initialAnimation, isWindowVisible: true });
        if (!shown) hideStartupAnimation();
        await import('./main');
    } catch (error: unknown) {
        console.error('Unable to load the application', error);
        showStartupAnimation({
            ...initialAnimation,
            isWindowVisible: true,
            startupError: 'WinCommander could not start. Retry to reload the app.',
        });
        try {
            await waitForStartupAnimationReady();
            await revealStartupWindow();
        } catch {
            const { message } = await import('@tauri-apps/plugin-dialog');
            await message('WinCommander could not load its startup artwork. Please close and reopen the app. If this continues, reinstall WinCommander.', {
                title: 'WinCommander could not start', kind: 'error',
            });
        }
    }
}

if (isMain) void startMainWindow();
else void import('./main');
