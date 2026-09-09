import { useEffect } from 'react';
import { useAppState } from '../context/AppContext';
import { useTheme } from '../context/ThemeContext';
import useMotionPreference from '../hooks/useMotionPreference';
import { getDisplayBranding } from '../lib/branding';
import { showStartupAnimation, hideStartupAnimation } from '../startup/animationRoot';
import { cacheStartupBranding } from '../startup/brandingCache';

interface SplashScreenProps {
    onComplete: () => void;
    isAppReady: boolean;
    startupError: string | null;
    onRetry: () => void;
}

export default function SplashScreen(props: SplashScreenProps) {
    const { appSettings } = useAppState();
    const { theme } = useTheme();
    const reducedMotion = useMotionPreference() === 'reduced';
    const { companyLabel, productLabel } = getDisplayBranding(appSettings);
    const { onComplete, isAppReady, startupError, onRetry } = props;

    useEffect(() => {
        if (appSettings) cacheStartupBranding({ companyLabel, productLabel });
    }, [appSettings, companyLabel, productLabel]);

    useEffect(() => {
        showStartupAnimation({
            onComplete, isAppReady, startupError, onRetry,
            branding: { companyLabel, productLabel },
            isLight: theme === 'light', reducedMotion,
        });
    }, [onComplete, isAppReady, startupError, onRetry, companyLabel, productLabel, theme, reducedMotion]);

    useEffect(() => hideStartupAnimation, []);

    return null;
}
