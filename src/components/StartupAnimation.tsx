import { useEffect, useRef, useState } from 'react';
import { LOGO_URL } from '../assets/logoUrl';
import StartupRecovery from './startup/StartupRecovery';
import StartupNotice from './startup/StartupNotice';

const SPLASH_DURATION_MS = 1500;

export interface StartupAnimationProps {
    branding: { companyLabel: string; productLabel: string };
    isLight: boolean;
    reducedMotion: boolean;
    onComplete: () => void;
    isAppReady: boolean;
    isWindowVisible?: boolean;
    startupError: string | null;
    startupNotice?: string | null;
    onRetry: () => void;
}

export default function StartupAnimation({
    onComplete,
    isAppReady,
    isWindowVisible = true,
    startupError,
    startupNotice = null,
    onRetry,
    branding,
    isLight,
    reducedMotion,
}: StartupAnimationProps) {
    const calledRef = useRef(false);
    const [animationDone, setAnimationDone] = useState(false);

    // This is a presentation-only hold. Readiness still owns dismissal, and a
    // delayed native reveal never starts a timer against an invisible window.
    useEffect(() => {
        if (!isWindowVisible) return;
        const timer = setTimeout(() => setAnimationDone(true), reducedMotion ? 500 : SPLASH_DURATION_MS);
        return () => clearTimeout(timer);
    }, [isWindowVisible, reducedMotion]);

    useEffect(() => {
        if (!animationDone || !isAppReady || startupError || !isWindowVisible || calledRef.current) return;
        calledRef.current = true;
        onComplete();
    }, [animationDone, isAppReady, startupError, onComplete, isWindowVisible]);

    return (
        <div className={`splash-screen${isLight ? ' splash-screen--light' : ' splash-screen--dark'}${reducedMotion ? ' splash-screen--reduced' : ''}${startupError ? ' splash-screen--failed' : ''}${startupNotice && !startupError ? ' splash-screen--notice' : ''}${!isWindowVisible ? ' splash-screen--waiting' : ''}`}>
            <div className="sp-blueprint-grid" aria-hidden="true" />
            <div className="sp-blueprint-glow" aria-hidden="true" />

            <div className="sp-content">
                <div className="sp-logo-wrap">
                    <div className="sp-logo-core">
                        <svg className="sp-ring sp-ring-outer" viewBox="0 0 160 160" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
                            <circle cx="80" cy="80" r="72" stroke="currentColor" strokeWidth="1.5" strokeDasharray="10 8" className="sp-ring-circle-outer" />
                        </svg>
                        <svg className="sp-ring sp-ring-inner" viewBox="0 0 138 138" fill="none" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
                            <circle cx="69" cy="69" r="60" stroke="currentColor" strokeWidth="1" strokeDasharray="5 9" className="sp-ring-circle-inner" />
                        </svg>
                        <div className="sp-logo-plate" aria-hidden="true">
                            <img src={LOGO_URL} alt="" className="sp-logo-img" />
                        </div>
                    </div>
                </div>

                <h1 className="sp-brand">{branding.companyLabel}</h1>
                <p className="sp-sub">{branding.productLabel}</p>
                <StartupNotice message={startupNotice} onRetry={onRetry} />
                <StartupRecovery error={startupError} onRetry={onRetry} />
            </div>
        </div>
    );
}
