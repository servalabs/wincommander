import { showStartupAnimation } from './startup/animationRoot';
import { readStartupBranding } from './startup/brandingCache';
import { applyMotionClass } from './lib/motionPolicy';

const native = (window as typeof window & {
    __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } };
}).__TAURI_INTERNALS__;
const label = native?.metadata?.currentWindow?.label;
const isMain = Boolean(native) && (!label || label === 'main')
    && new URLSearchParams(location.search).get('wc-window') !== 'notification-alerts';

applyMotionClass();
const initialAnimation = {
    branding: readStartupBranding(),
    isLight: document.documentElement.classList.contains('light'),
    reducedMotion: document.documentElement.classList.contains('wc-no-motion'),
    isAppReady: false,
    startupError: null,
    onComplete: () => {},
    onRetry: () => location.reload(),
};

if (isMain) showStartupAnimation(initialAnimation);

function loadApplication(): void {
    void import('./main').catch((error: unknown) => {
        console.error('Unable to load the application', error);
        if (isMain) showStartupAnimation({
            ...initialAnimation,
            startupError: 'WinCommander could not start. Retry to reload the app.',
        });
    });
}

// Let the actual animation paint before loading the dashboard's module graph.
if (isMain) requestAnimationFrame(() => requestAnimationFrame(loadApplication));
else loadApplication();
