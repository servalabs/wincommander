import { createRoot, type Root } from 'react-dom/client';
import StartupAnimation, { type StartupAnimationProps } from '../components/StartupAnimation';
import splashStyles from '../components/SplashScreen.css?inline';
import '@fontsource/ibm-plex-mono/600.css';

let root: Root | undefined;
let container: HTMLDivElement | undefined;
let generation = 0;
let appearance: Pick<StartupAnimationProps, 'branding' | 'isLight' | 'reducedMotion'> | undefined;

/** The bootstrap and application update one animation instance without replay. */
export function showStartupAnimation(props: StartupAnimationProps): void {
    generation += 1;
    if (!container) {
        container = document.createElement('div');
        container.id = 'startup-animation';
        document.body.append(container);
        // Dashboard styles and theme hydration cannot reset a running splash.
        const shadow = container.attachShadow({ mode: 'open' });
        const style = document.createElement('style');
        style.textContent = splashStyles;
        const mount = document.createElement('div');
        shadow.append(style, mount);
        root = createRoot(mount);
        appearance = { branding: props.branding, isLight: props.isLight, reducedMotion: props.reducedMotion };
    }
    container.hidden = false;
    root!.render(<StartupAnimation {...props} {...appearance} />);
}

export function hideStartupAnimation(): void {
    const closingGeneration = ++generation;
    // React effect cleanup may be followed immediately by a new owner/update.
    queueMicrotask(() => {
        if (generation === closingGeneration) {
            if (container) container.hidden = true;
            root?.render(null);
        }
    });
}
