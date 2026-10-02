import { useEffect, useRef, useState } from 'react';
import { LOGO_URL } from '../assets/logoUrl';
import StartupRecovery from './startup/StartupRecovery';
import StartupNotice from './startup/StartupNotice';

const SPLASH_DURATION_MS = 1500;
const SCRAMBLE_GLYPHS = '#$%01/\\[]{}<>+-*ABCDEF';
const SCRAMBLE_TICK_MS = 60;
const SCRAMBLE_HOLD_TICKS = 4;
const RAIN_CHARS = [
    'S', 'E', 'R', 'V', 'A', 'L', 'B', 'W', 'I', 'N', 'C', 'O', 'M', 'D',
    's', 'e', 'r', 'v', 'a', 'l', 'b', 'w', 'i', 'n', 'c', 'o', 'm', 'd',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    '#', '*', '|', '/', '\\', '%', '$', '@', '!', '?', ':', '=', '>', '<', '+', '-', '^', '&',
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ', 'ﾅ', 'ﾆ', 'ﾇ', 'ﾐ', 'ﾑ', 'ﾒ', 'ﾓ',
];

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

function scrambleWord(target: string, resolved: number): string {
    return target.split('').map((character, index) => {
        if (index < resolved || character === ' ') return character;
        return SCRAMBLE_GLYPHS[Math.floor(Math.random() * SCRAMBLE_GLYPHS.length)];
    }).join('');
}

function randomRainCharacter(): string {
    return RAIN_CHARS[Math.floor(Math.random() * RAIN_CHARS.length)];
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
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const animationFrameRef = useRef(0);
    const pausedRef = useRef(Boolean(startupError) || !isWindowVisible);
    const [scrambleText, setScrambleText] = useState(() => scrambleWord(branding.companyLabel, 0));
    const [animationDone, setAnimationDone] = useState(false);
    pausedRef.current = Boolean(startupError) || !isWindowVisible;

    // The release animation is canvas-based and capped at 30fps. It is only
    // presentational: readiness still controls when the app is revealed.
    useEffect(() => {
        if (reducedMotion) return;
        const canvas = canvasRef.current;
        const context = canvas?.getContext('2d');
        if (!canvas || !context) return;

        let width = 0;
        let height = 0;
        let columns = 0;
        let rows = 0;
        let characters: string[][] = [];
        let brightness: Float32Array[] = [];
        let drops: Array<Array<{ row: number; speed: number }>> = [];
        const cellWidth = 16;
        const rowHeight = 17;
        const prepare = () => {
            width = canvas.offsetWidth;
            height = canvas.offsetHeight;
            if (!width || !height) return false;
            const deviceScale = window.devicePixelRatio || 1;
            canvas.width = Math.round(width * deviceScale);
            canvas.height = Math.round(height * deviceScale);
            context.setTransform(deviceScale, 0, 0, deviceScale, 0, 0);
            context.font = '700 12px "JetBrains Mono", ui-monospace, monospace';
            context.textAlign = 'center';
            context.textBaseline = 'top';
            columns = Math.max(1, Math.floor(width / cellWidth));
            rows = Math.ceil(height / rowHeight) + 2;
            characters = Array.from({ length: columns }, () => Array.from({ length: rows }, randomRainCharacter));
            brightness = Array.from({ length: columns }, () => new Float32Array(rows));
            drops = Array.from({ length: columns }, () => Array.from(
                { length: Math.random() < 0.38 ? 2 : 1 },
                () => ({ row: -(Math.random() * rows * 0.85), speed: 0.55 + Math.random() * 0.55 }),
            ));
            return true;
        };

        if (!prepare()) return;
        const observer = new ResizeObserver(() => prepare());
        observer.observe(canvas);
        let lastFrame = 0;
        const frame = (timestamp: number) => {
            animationFrameRef.current = requestAnimationFrame(frame);
            if (pausedRef.current || timestamp - lastFrame < 1000 / 30) return;
            lastFrame = timestamp;
            context.clearRect(0, 0, width, height);
            for (let column = 0; column < columns; column += 1) {
                for (let row = 0; row < rows; row += 1) {
                    if (brightness[column][row] > 0) brightness[column][row] = Math.max(0, brightness[column][row] - 0.038);
                    if (Math.random() < 0.004) characters[column][row] = randomRainCharacter();
                }
                for (const drop of drops[column]) {
                    const row = Math.floor(drop.row);
                    if (row >= 0 && row < rows) {
                        brightness[column][row] = 1;
                        if (row > 0) brightness[column][row - 1] = Math.max(brightness[column][row - 1], 0.68);
                    }
                    drop.row += drop.speed;
                    if (drop.row >= rows) {
                        drop.row = -(Math.random() * rows * 0.4 + 1);
                        drop.speed = 0.55 + Math.random() * 0.55;
                    }
                }
                const x = column * cellWidth + cellWidth / 2;
                for (let row = 0; row < rows; row += 1) {
                    const level = brightness[column][row];
                    if (level < 0.025) continue;
                    if (level > 0.92) context.fillStyle = isLight ? '#001b30' : '#ffffff';
                    else if (level > 0.55) context.fillStyle = isLight ? `rgba(0,50,90,${level.toFixed(2)})` : `rgba(130,255,225,${level.toFixed(2)})`;
                    else context.fillStyle = isLight ? `rgba(0,90,140,${(level * 0.88).toFixed(2)})` : `rgba(0,210,190,${(level * 0.88).toFixed(2)})`;
                    context.fillText(characters[column][row], x, row * rowHeight);
                }
            }
        };
        animationFrameRef.current = requestAnimationFrame(frame);
        return () => {
            cancelAnimationFrame(animationFrameRef.current);
            observer.disconnect();
        };
    }, [isLight, reducedMotion]);

    useEffect(() => {
        if (!isWindowVisible) return;
        const scrambleMs = branding.companyLabel.length * SCRAMBLE_HOLD_TICKS * SCRAMBLE_TICK_MS;
        const timer = setTimeout(() => setAnimationDone(true), reducedMotion ? 500 : Math.max(SPLASH_DURATION_MS, scrambleMs + 400));
        return () => clearTimeout(timer);
    }, [branding.companyLabel, isWindowVisible, reducedMotion]);

    useEffect(() => {
        if (!isWindowVisible) return;
        if (reducedMotion) {
            setScrambleText(branding.companyLabel);
            return;
        }
        let tick = 0;
        const interval = setInterval(() => {
            tick += 1;
            const resolved = Math.floor(tick / SCRAMBLE_HOLD_TICKS);
            if (resolved >= branding.companyLabel.length) {
                setScrambleText(branding.companyLabel);
                clearInterval(interval);
                return;
            }
            setScrambleText(scrambleWord(branding.companyLabel, resolved));
        }, SCRAMBLE_TICK_MS);
        return () => clearInterval(interval);
    }, [branding.companyLabel, isWindowVisible, reducedMotion]);

    useEffect(() => {
        if (!animationDone || !isAppReady || startupError || !isWindowVisible || calledRef.current) return;
        calledRef.current = true;
        onComplete();
    }, [animationDone, isAppReady, startupError, onComplete, isWindowVisible]);

    return (
        <div className={`splash-screen${isLight ? ' splash-screen--light' : ''}${reducedMotion ? ' splash-screen--reduced' : ''}${startupError ? ' splash-screen--failed' : ''}${startupNotice && !startupError ? ' splash-screen--notice' : ''}${!isWindowVisible ? ' splash-screen--waiting' : ''}`}>
            <div className="sp-scanlines" aria-hidden="true" />
            <div className="sp-glow" aria-hidden="true" />
            <div className="sp-grid" aria-hidden="true" />
            <canvas ref={canvasRef} className="sp-matrix-canvas" aria-hidden="true" />
            <div className="sp-content">
                <div className="sp-logo-wrap"><div className="sp-logo-core">
                    <svg className="sp-ring sp-ring-outer" viewBox="0 0 160 160" fill="none" aria-hidden="true"><circle cx="80" cy="80" r="72" stroke="currentColor" strokeWidth="1.5" strokeDasharray="10 8" className="sp-ring-circle-outer" /></svg>
                    <svg className="sp-ring sp-ring-inner" viewBox="0 0 138 138" fill="none" aria-hidden="true"><circle cx="69" cy="69" r="60" stroke="currentColor" strokeWidth="1" strokeDasharray="5 9" className="sp-ring-circle-inner" /></svg>
                    <div className="sp-logo-plate" aria-hidden="true"><img src={LOGO_URL} alt="" className="sp-logo-img" /></div>
                </div></div>
                <h1 className="sp-brand sp-scramble">{scrambleText}</h1>
                <p className="sp-sub">{branding.productLabel}</p>
                <StartupNotice message={startupNotice} onRetry={onRetry} />
                <StartupRecovery error={startupError} onRetry={onRetry} />
            </div>
        </div>
    );
}
