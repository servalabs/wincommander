import { createRoot, type Root } from 'react-dom/client';
import StartupAnimation, { type StartupAnimationProps } from '../components/StartupAnimation';
import splashStylesUrl from '../components/SplashScreen.css?url';
import '@fontsource/ibm-plex-mono/600.css';

let root: Root | undefined;
let container: HTMLDivElement | undefined;
let generation = 0;
let appearance: Pick<StartupAnimationProps, 'branding' | 'isLight' | 'reducedMotion'> | undefined;
let stylesReady: Promise<void> | undefined;
let stylesLoaded = false;
let latestProps: StartupAnimationProps | undefined;

/** The bootstrap and application update one animation instance without replay. */
export function showStartupAnimation(props: StartupAnimationProps): void {
    generation += 1;
    latestProps = props;
    if (!container) {
        container = document.createElement('div');
        container.id = 'startup-animation';
        document.body.append(container);
        // Dashboard styles and theme hydration cannot reset a running splash.
        const shadow = container.attachShadow({ mode: 'open' });
        // Packaged Tauri CSP authorizes bundled styles, not runtime inline CSS.
        const style = document.createElement('link');
        style.rel = 'stylesheet';
        style.href = splashStylesUrl;
        stylesReady = new Promise<void>((resolve, reject) => {
            style.onload = () => resolve();
            style.onerror = () => reject(new Error('Unable to load startup animation styles.'));
        });
        // The startup entry awaits readiness; later React owners only render.
        void stylesReady.then(() => {
            stylesLoaded = true;
            if (latestProps && container && !container.hidden) {
                root!.render(<StartupAnimation {...latestProps} {...appearance} />);
            }
        }).catch(() => {});
        const mount = document.createElement('div');
        shadow.append(style, mount);
        root = createRoot(mount);
        appearance = { branding: props.branding, isLight: props.isLight, reducedMotion: props.reducedMotion };
    }
    container.hidden = false;
    if (stylesLoaded) root!.render(<StartupAnimation {...props} {...appearance} />);
}

/** Wait for styled, committed artwork before exposing the native window. */
export async function waitForStartupAnimationReady(): Promise<void> {
    if (!stylesReady || !container) throw new Error('Startup animation has not been created.');
    const host = container;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let cancelled = false;
    try {
        await Promise.race([
            (async () => {
                await stylesReady;
                // Hidden native windows may suspend RAF, so inspect commits with a timer.
                while (!cancelled && !host.shadowRoot?.querySelector('.splash-screen')) {
                    await new Promise(resolve => setTimeout(resolve, 10));
                }
                if (cancelled) return;
                const logo = host.shadowRoot?.querySelector<HTMLImageElement>('.sp-logo-img');
                if (logo) await logo.decode();
                await document.fonts.load('600 16px "IBM Plex Mono"');
                await document.fonts.ready;
                await new Promise<void>(resolve => {
                    const fallback = setTimeout(resolve, 50);
                    requestAnimationFrame(() => requestAnimationFrame(() => {
                        clearTimeout(fallback);
                        resolve();
                    }));
                });
            })(),
            new Promise<never>((_, reject) => {
                timeout = setTimeout(() => reject(new Error('Startup artwork did not become ready.')), 10000);
            }),
        ]);
    } finally {
        cancelled = true;
        clearTimeout(timeout);
    }
}

export function hideStartupAnimation(): void {
    const closingGeneration = ++generation;
    // React effect cleanup may be followed immediately by a new owner/update.
    queueMicrotask(() => {
        if (generation === closingGeneration) {
            if (container) container.hidden = true;
            latestProps = undefined;
            root?.render(null);
        }
    });
}
