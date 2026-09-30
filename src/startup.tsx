import { showStartupAnimation, hideStartupAnimation } from './startup/animationRoot';
import { readStartupBranding } from './startup/brandingCache';
import { applyMotionClass } from './lib/motionPolicy';
import { prepareStartupTheme, revealStartupWindow } from './hooks/startupWindow';
import { flushSync } from 'react-dom';

const native = (window as typeof window & {
    __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } };
}).__TAURI_INTERNALS__;
const label = native?.metadata?.currentWindow?.label;
const isMain = Boolean(native) && (!label || label === 'main')
    && new URLSearchParams(location.search).get('wc-window') !== 'notification-alerts';

applyMotionClass();
async function startMainWindow(): Promise<void> {
    try {
        await prepareStartupTheme();
    } catch (error: unknown) {
        // Theme persistence is optional.  It must never prevent the desktop
        // window from appearing; the application will apply its normal theme
        // once its providers mount.
        console.warn('Unable to prepare the cached startup theme', error);
    }

    const initialAnimation = {
        branding: readStartupBranding(),
        isLight: document.documentElement.classList.contains('light'),
        reducedMotion: document.documentElement.classList.contains('wc-no-motion'),
        isAppReady: false,
        isWindowVisible: false,
        startupError: null,
        // The bootstrap never presents a retry control.  A no-op preserves the
        // shared splash component contract until the dashboard takes ownership.
        onComplete: () => {},
        onRetry: () => {},
    };
    // Mount content before asking the native side to show the HWND.  Do not
    // gate visibility on a stylesheet, logo decode, web font, or animation
    // frame: any one of those can be delayed or unavailable after an update.
    // Native visibility must follow a committed DOM, not a queued React render.
    flushSync(() => showStartupAnimation(initialAnimation));
    const shown = await revealStartupWindow();
    // The intro clock starts after the real native window is revealed.
    showStartupAnimation({ ...initialAnimation, isWindowVisible: true });
    if (!shown) hideStartupAnimation();
    // Loading the dashboard is deliberately last: a slow module can no longer
    // leave a hidden, white-looking native window during startup.
    await import('./main');
}

if (isMain) void startMainWindow().catch(error => {
    console.error('WinCommander startup failed', error);
    const status = document.createElement('div');
    status.setAttribute('role', 'alert');
    status.style.cssText = 'position:fixed;inset:0;z-index:2147483647;display:grid;place-content:center;gap:16px;padding:32px;background:#0a0f12;color:#fff;font:16px system-ui';
    const message = document.createElement('p');
    message.textContent = 'WinCommander could not finish opening its interface. Retry startup. Your saved settings have not been reset.';
    const retry = document.createElement('button');
    retry.textContent = 'Retry startup';
    retry.onclick = () => location.reload();
    status.append(message, retry);
    document.body.append(status);
});
else void import('./main');
