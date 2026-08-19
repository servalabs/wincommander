import { useEffect, useRef, useState } from 'react';
import { useAppState } from '../context/AppContext';
import { useTheme } from '../context/ThemeContext';
import useMotionPreference from '../hooks/useMotionPreference';
import { getDisplayBranding } from '../lib/branding';
import { LOGO_DATA_URL } from '../assets/logoData';
import './SplashScreen.css';

const SPLASH_DURATION_MS = 1500;

const SCRAMBLE_GLYPHS = "#$%01/\\[]{}<>+-*ABCDEF";
const SCRAMBLE_TICK_MS = 60;
// Ticks each character holds scrambled before it "decodes".
// 4 × 60ms = 240ms per letter → 9-char label fully resolves in ~2160ms.
const SCRAMBLE_HOLD_TICKS = 4;

// Single-character pool for the canvas matrix rain.
// Upper + lower brand letters give variety; katakana adds Matrix DNA;
// symbols / digits fill gaps. Single chars only — multi-char tokens
// overflow the cell width and misalign adjacent columns on canvas.
const RAIN_CHARS = [
    // Brand: SERVALABS + WINCOMMANDER — upper and lower
    "S","E","R","V","A","L","B","W","I","N","C","O","M","D",
    "s","e","r","v","a","l","b","w","i","n","c","o","m","d",
    // Hex digits + extra letters for variety
    "0","1","2","3","4","5","6","7","8","9",
    "A","B","C","D","E","F","a","b","c","d","e","f",
    "x","y","z","g","h","k","p","q","u","t",
    "X","Y","Z","G","H","K","P","Q","U","T",
    // Symbols — read as data/code without being distracting
    "#","*","|","/","\\","%","$","@","!","?",":","=",">","<","+","-","^","&",
    // Katakana half-widths — Matrix-rain DNA
    "ｱ","ｲ","ｳ","ｴ","ｵ","ｶ","ｷ","ｸ","ｹ","ｺ","ﾅ","ﾆ","ﾇ","ﾐ","ﾑ","ﾒ","ﾓ",
];

interface SplashScreenProps {
    onComplete: () => void;
    isAppReady: boolean;
}

function scrambleWord(target: string, resolved: number): string {
    return target
        .split("")
        .map((ch, i) => {
            if (i < resolved) return ch;
            if (ch === " ") return " ";
            return SCRAMBLE_GLYPHS[Math.floor(Math.random() * SCRAMBLE_GLYPHS.length)];
        })
        .join("");
}

const rc = () => RAIN_CHARS[Math.floor(Math.random() * RAIN_CHARS.length)];

export default function SplashScreen({ onComplete, isAppReady }: SplashScreenProps) {
    const { appSettings } = useAppState();
    const { theme } = useTheme();
    const isLight = theme === 'light';
    // When animations are disabled (explicit toggle, OS reduced-motion, or the
    // low-spec hardware default), skip the splash's JS-driven work: the canvas
    // matrix rain, the eased progress rAF, and the letter-scramble. The CSS
    // effects (scanlines/glow/rings) are already killed by the global
    // html.wc-no-motion rule; only these JS loops need an explicit guard.
    const reducedMotion = useMotionPreference() === 'reduced';
    const calledRef = useRef(false);
    const canvasRef = useRef<HTMLCanvasElement>(null);
    const animRef = useRef<number>(0);
    const branding = getDisplayBranding(appSettings);
    const [logoFailed, setLogoFailed] = useState(false);
    const [logoLoaded, setLogoLoaded] = useState(false);
    const [logoReady, setLogoReady] = useState(false);
    const [scrambleText, setScrambleText] = useState(() => scrambleWord(branding.companyLabel, 0));
    const [animDone, setAnimDone] = useState(false);

    // React splash owns the frame now — drop the HTML first-paint overlay
    // so the two never stack.
    useEffect(() => {
        document.getElementById("boot-splash")?.setAttribute("hidden", "");
    }, []);

    // Canvas matrix rain — brightness-grid approach.
    // Every cell has a float brightness [0..1] that decays each frame; drops
    // stamp brightness=1 at their head position each frame. This gives:
    //   • Natural trailing fade without explicit trail management
    //   • Multiple simultaneous drops per column for dense coverage
    //   • Characters mutate in place for the "decrypting" shimmer
    //   • Canvas stays transparent so CSS glow/grid/scanlines show through
    // isLight is a dep so the canvas reinitializes if settings.json hydration
    // corrects a stale localStorage theme after the first render.
    useEffect(() => {
        // Reduced motion: don't run the matrix-rain animation loop at all.
        if (reducedMotion) return;
        const canvas = canvasRef.current;
        if (!canvas) return;
        const ctx = canvas.getContext('2d');
        if (!ctx) return;

        const dpr = window.devicePixelRatio || 1;

        const W = canvas.offsetWidth;
        const H = canvas.offsetHeight;
        if (!W || !H) return;

        canvas.width = W * dpr;
        canvas.height = H * dpr;
        ctx.scale(dpr, dpr);

        const FONT_PX = 12;
        const COL_W   = 16;   // tight columns → dense horizontal coverage
        const ROW_H   = 17;   // tight rows → dense vertical coverage
        const FADE    = 0.038; // brightness decay per frame — slightly faster fade to match quicker drops
        const numCols = Math.floor(W / COL_W);
        const numRows = Math.ceil(H / ROW_H) + 2;

        // Character grid — each cell holds the char drawn at that position.
        // Cells mutate randomly to give trails the "decrypting" shimmer.
        const grid: string[][] = Array.from({ length: numCols }, () =>
            Array.from({ length: numRows }, rc)
        );

        // Brightness per cell [0..1].  Head = 1.0, fades to 0 over ~1 s.
        const bright = Array.from({ length: numCols }, () => new Float32Array(numRows));

        // 1-2 simultaneous drops per column, staggered start so the entire
        // screen fills immediately rather than waiting for the first pass.
        const drops = Array.from({ length: numCols }, () => {
            const n = Math.random() < 0.38 ? 2 : 1;
            return Array.from({ length: n }, () => ({
                row:   -(Math.random() * numRows * 0.85),
                speed: 0.55 + Math.random() * 0.55,  // ~0.55–1.1 rows/frame — visibly fast rain
            }));
        });

        ctx.font        = `700 ${FONT_PX}px "JetBrains Mono", ui-monospace, monospace`;
        ctx.textAlign   = 'center';
        ctx.textBaseline = 'top';

        // Build the radial vignette ONCE. It depends only on W/H/isLight — all
        // constant for this effect's lifetime — so re-creating it every frame
        // (createRadialGradient + 3 addColorStop) was pure waste, 30×/sec, on
        // top of the fillText load. A CanvasGradient is reusable across frames.
        const cx = W / 2;
        const cy = H * 0.44;
        const innerR = Math.min(W, H) * 0.14;
        const outerR = Math.min(W, H) * 0.62;
        const vignette = ctx.createRadialGradient(cx, cy, innerR, cx, cy, outerR);
        if (isLight) {
            vignette.addColorStop(0,    'rgba(255,255,255,0.72)');
            vignette.addColorStop(0.5,  'rgba(255,255,255,0.28)');
            vignette.addColorStop(1,    'rgba(255,255,255,0)');
        } else {
            vignette.addColorStop(0,    'rgba(10,15,18,0.90)');
            vignette.addColorStop(0.45, 'rgba(10,15,18,0.55)');
            vignette.addColorStop(1,    'rgba(0,0,0,0)');
        }

        let lastT = 0;
        const FRAME_MS = 1000 / 30;

        const frame = (timestamp: number) => {
            animRef.current = requestAnimationFrame(frame);
            if (timestamp - lastT < FRAME_MS) return;
            lastT = timestamp;

            ctx.clearRect(0, 0, W, H);

            // Decay all cells; mutate ~0.4 % of chars for the shimmer effect
            for (let c = 0; c < numCols; c++) {
                for (let r = 0; r < numRows; r++) {
                    if (bright[c][r] > 0) {
                        bright[c][r] = Math.max(0, bright[c][r] - FADE);
                    }
                    if (Math.random() < 0.004) grid[c][r] = rc();
                }
            }

            // Advance drops, stamp head brightness
            for (let c = 0; c < numCols; c++) {
                for (const d of drops[c]) {
                    const r = Math.floor(d.row);
                    if (r >= 0 && r < numRows) {
                        bright[c][r] = 1.0;
                        // Second row from head: slightly dimmer so the gradient
                        // starts immediately behind the bright white head.
                        if (r > 0) bright[c][r - 1] = Math.max(bright[c][r - 1], 0.68);
                    }
                    d.row += d.speed;
                    // Reset when head clears the bottom (tail fades naturally on its own)
                    if (d.row >= numRows) {
                        d.row   = -(Math.random() * numRows * 0.4 + 1);
                        d.speed = 0.55 + Math.random() * 0.55;
                    }
                }
            }

            // Draw every cell that still has visible brightness
            for (let c = 0; c < numCols; c++) {
                const x = c * COL_W + COL_W / 2;
                for (let r = 0; r < numRows; r++) {
                    const b = bright[c][r];
                    if (b < 0.025) continue;

                    const y = r * ROW_H;
                    if (y > H) break;

                    if (b > 0.92) {
                        // Head — bright white / deep navy
                        ctx.fillStyle = isLight ? '#001b30' : '#ffffff';
                    } else if (b > 0.55) {
                        // Near-head — saturated cyan / dark teal
                        ctx.fillStyle = isLight
                            ? `rgba(0, 50, 90, ${b.toFixed(2)})`
                            : `rgba(130, 255, 225, ${b.toFixed(2)})`;
                    } else {
                        // Trail — fading cyan / fading teal
                        ctx.fillStyle = isLight
                            ? `rgba(0, 90, 140, ${(b * 0.88).toFixed(2)})`
                            : `rgba(0, 210, 190, ${(b * 0.88).toFixed(2)})`;
                    }

                    ctx.fillText(grid[c][r], x, y);
                }
            }

            // Radial vignette — softens the rain behind the logo so focus
            // stays on the brand mark. Gradient is prebuilt above (constant
            // for W/H/isLight); we just re-fill with it each frame.
            ctx.fillStyle = vignette;
            ctx.fillRect(0, 0, W, H);
        };

        animRef.current = requestAnimationFrame(frame);
        return () => cancelAnimationFrame(animRef.current);
    }, [isLight, reducedMotion]);

    // Two-stage logo safety valve — protects against slow cold-disk reads
    // (Defender scanning the bundle on first run) and GPU OOM edge cases.
    useEffect(() => {
        const fallbackTimer = setTimeout(() => {
            if (!logoLoaded) setLogoFailed(true);
        }, 1500);
        const readyTimer = setTimeout(() => setLogoReady(true), 2500);
        return () => {
            clearTimeout(fallbackTimer);
            clearTimeout(readyTimer);
        };
    }, [logoLoaded]);

    // Mark animation complete only AFTER the intro animation has had time to
    // finish, so the splash never cuts to the dashboard mid-animation. The
    // letter-scramble is the longest piece (label length × hold-ticks × tick),
    // so hold at least that long (plus a short beat) when motion is on. Under
    // reduced motion there's nothing to wait for, so a short hold is enough.
    // (Dismissal still also requires isAppReady, and the 20s hard-cap below
    // guarantees the splash never traps startup.)
    useEffect(() => {
        if (!logoReady) return;
        const scrambleMs = branding.companyLabel.length * SCRAMBLE_HOLD_TICKS * SCRAMBLE_TICK_MS;
        const holdMs = reducedMotion
            ? 500
            : Math.max(SPLASH_DURATION_MS, scrambleMs + 400);
        const t = setTimeout(() => setAnimDone(true), holdMs);
        return () => clearTimeout(t);
    }, [logoReady, reducedMotion, branding.companyLabel]);

    // When the app reports ready, show the final phase.
    useEffect(() => {
    }, [isAppReady]);

    // Gate: dismiss only when animation is done AND app data is ready.
    useEffect(() => {
        if (!animDone || !isAppReady) return;
        if (!calledRef.current) {
            calledRef.current = true;
            onComplete();
        }
    }, [animDone, isAppReady, onComplete]);

    // Hard-cap: dismiss after 20s regardless, so startup errors never hang the splash.
    useEffect(() => {
        if (!logoReady) return;
        const t = setTimeout(() => {
            if (!calledRef.current) {
                calledRef.current = true;
                onComplete();
            }
        }, 20_000);
        return () => clearTimeout(t);
    }, [logoReady, onComplete]);


    useEffect(() => {
        const target = branding.companyLabel;
        // Reduced motion: show the resolved brand text immediately, no scramble.
        if (reducedMotion) {
            setScrambleText(target);
            return;
        }
        let tick = 0;
        setScrambleText(scrambleWord(target, 0));
        const id = setInterval(() => {
            tick += 1;
            const resolved = Math.floor(tick / SCRAMBLE_HOLD_TICKS);
            if (resolved >= target.length) {
                setScrambleText(target);
                clearInterval(id);
                return;
            }
            setScrambleText(scrambleWord(target, resolved));
        }, SCRAMBLE_TICK_MS);
        return () => clearInterval(id);
    }, [branding.companyLabel, reducedMotion]);

    return (
        <div className="splash-screen">
            <div className="sp-scanlines" />
            <div className="sp-glow" />
            <div className="sp-grid" />

            {/* Canvas matrix rain — proper falling characters with white head
                + fading cyan trail, drawn per-frame via requestAnimationFrame */}
            <canvas ref={canvasRef} className="sp-matrix-canvas" aria-hidden="true" />

            <div className="sp-content">
                <div className="sp-logo-wrap">
                    <div className="sp-logo-core">
                        <svg className="sp-ring sp-ring-outer" viewBox="0 0 160 160" fill="none" xmlns="http://www.w3.org/2000/svg">
                            <circle cx="80" cy="80" r="72" stroke="currentColor" strokeWidth="1.5" strokeDasharray="10 8" className="sp-ring-circle-outer" />
                        </svg>
                        <svg className="sp-ring sp-ring-inner" viewBox="0 0 138 138" fill="none" xmlns="http://www.w3.org/2000/svg">
                            <circle cx="69" cy="69" r="60" stroke="currentColor" strokeWidth="1" strokeDasharray="5 9" className="sp-ring-circle-inner" />
                        </svg>
                        <div className="sp-logo-plate" aria-hidden="true">
                            {!logoFailed ? (
                                <img
                                    src={LOGO_DATA_URL}
                                    alt={branding.productLabel}
                                    className="sp-logo-img"
                                    onLoad={() => { setLogoLoaded(true); setLogoReady(true); }}
                                    onError={() => { setLogoFailed(true); setLogoReady(true); }}
                                />
                            ) : (
                                <div className="sp-logo-fallback" aria-label={branding.productLabel}>
                                    <span>WC</span>
                                </div>
                            )}
                        </div>
                    </div>
                </div>

                <h1 className="sp-brand sp-scramble">{scrambleText}</h1>
                <p className="sp-sub">{branding.productLabel}</p>

            </div>
        </div>
    );
}
