import { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom";
import { motion, AnimatePresence } from "framer-motion";
import { useTheme } from "../../context/ThemeContext";

const COLORS = [
  '#ff7a1a', '#f4b447', '#7bd88f', '#60a5fa',
  '#f472b6', '#a78bfa', '#34d399', '#fb7185',
  '#fbbf24', '#38bdf8',
];

type Shape = 'ribbon' | 'square' | 'circle' | 'star';

// Deterministic shape assignment (no Math.random per call) to avoid re-render drift
const SHAPE_SEQ: Shape[] = ['ribbon', 'square', 'ribbon', 'circle', 'ribbon', 'star', 'ribbon', 'square'];

function shapeStyle(shape: Shape, size: number, color: string): React.CSSProperties {
  const base: React.CSSProperties = { background: color };
  if (shape === 'ribbon') return { ...base, width: 3, height: 16, borderRadius: 1 };
  if (shape === 'circle') return { ...base, width: size, height: size, borderRadius: '50%', boxShadow: `0 0 6px ${color}` };
  if (shape === 'star') return {
    ...base, width: size, height: size,
    clipPath: 'polygon(50% 0%,61% 35%,98% 35%,68% 57%,79% 91%,50% 70%,21% 91%,32% 57%,2% 35%,39% 35%)',
    boxShadow: `0 0 6px ${color}`,
  };
  // square
  return { ...base, width: size, height: size, borderRadius: 2, boxShadow: `0 0 5px ${color}` };
}

interface Particle {
  angle: number; speed: number; shape: Shape; size: number;
  color: string; rotation: number; duration: number; delay: number;
}

function buildParticles(count: number): Particle[] {
  // Use a seeded sequence — Math.random is fine here since this array is built
  // once per mount and never recalculated (no state drives a re-render that
  // would call buildParticles again).
  return Array.from({ length: count }, (_, i) => {
    const angle = (i / count) * 360 + (i % 3 === 0 ? 12 : i % 3 === 1 ? -8 : 4);
    const speed = 180 + (i * 73 % 340);
    const shape = SHAPE_SEQ[i % SHAPE_SEQ.length];
    const size = shape === 'ribbon' ? 8 : 7 + (i * 37 % 8);
    const color = COLORS[i % COLORS.length];
    const rotation = (i % 2 === 0 ? 1 : -1) * (200 + (i * 53 % 600));
    const duration = 2.6 + (i * 17 % 1000) / 1000;
    const delay = i * 0.006;
    return { angle, speed, shape, size, color, rotation, duration, delay };
  });
}

const PARTICLES = buildParticles(72);

interface LicenseConfettiProps {
  onDone: () => void;
  message?: string;
}

export default function LicenseConfetti({ onDone, message }: LicenseConfettiProps) {
  const { theme } = useTheme();
  const isDark = theme === 'dark';
  const doneRef = useRef(false);
  const [showBanner, setShowBanner] = useState(true);
  const [showPopper, setShowPopper] = useState(true);

  const closeAll = () => {
    if (doneRef.current) return;
    doneRef.current = true;
    setShowBanner(false);
    setShowPopper(false);
    setTimeout(onDone, 350);
  };

  useEffect(() => {
    const bannerHide  = setTimeout(() => setShowBanner(false), 1100);
    const popperHide  = setTimeout(() => setShowPopper(false), 1100);
    const finalDone   = setTimeout(() => { if (!doneRef.current) { doneRef.current = true; onDone(); } }, 3800);
    return () => { clearTimeout(bannerHide); clearTimeout(popperHide); clearTimeout(finalDone); };
  }, [onDone]);

  // ── theme tokens ─────────────────────────────────────────────────────
  const bannerBg      = isDark ? 'linear-gradient(180deg,rgba(28,23,18,.98),rgba(17,15,13,.96))' : 'linear-gradient(180deg,rgba(255,250,244,.99),rgba(246,239,230,.98))';
  const bannerBorder  = isDark ? '1px solid rgba(255,122,26,.52)' : '1px solid rgba(217,119,6,.38)';
  const bannerShadow  = isDark ? '0 28px 74px rgba(0,0,0,.55),0 0 64px rgba(255,122,26,.28),inset 0 0 26px rgba(255,122,26,.07)' : '0 20px 56px rgba(80,52,28,.16),0 0 36px rgba(217,119,6,.16)';
  const accentColor   = isDark ? '#ff7a1a' : '#c65f12';
  const accentGlow    = isDark ? '0 0 14px rgba(255,122,26,.66)' : '0 0 10px rgba(217,119,6,.32)';
  const mainText      = isDark ? '#f7f0e7' : '#211912';
  const subText       = isDark ? 'rgba(247,240,231,.64)' : 'rgba(33,25,18,.58)';
  const closeBorder   = isDark ? 'rgba(255,122,26,.34)' : 'rgba(217,119,6,.34)';
  const closeBg       = isDark ? 'rgba(255,122,26,.07)' : 'rgba(217,119,6,.08)';
  const closeColor    = isDark ? 'rgba(247,240,231,.86)' : '#211912';
  const popperGlow    = isDark ? 'drop-shadow(0 0 36px rgba(255,122,26,.56))' : 'drop-shadow(0 0 28px rgba(217,119,6,.36))';

  const vhFall = typeof window !== 'undefined' ? window.innerHeight * 0.8 : 600;

  return ReactDOM.createPortal(
    <div style={{ position: 'fixed', inset: 0, pointerEvents: 'none', overflow: 'hidden', zIndex: 99999 }}>

      {/* ── Confetti particles: gravity-arc physics ────────────────────── */}
      {PARTICLES.map((p, i) => {
        const rad   = (p.angle * Math.PI) / 180;
        const tx    = Math.cos(rad) * p.speed;
        // y: shoot toward apex, then gravity pulls past start line to ground
        const apex  = Math.sin(rad) * p.speed - 220;
        const ground = Math.sin(rad) * p.speed + vhFall;

        return (
          <motion.div
            key={`c-${i}`}
            initial={{ opacity: 1, x: 0, y: 0, rotate: 0, scale: 1 }}
            animate={{
              opacity: [1, 1, 1, 0],
              x: [0, tx * 0.55, tx * 1.1],
              y: [0, apex, ground],
              rotate: p.rotation,
              scale: [1, 1, 0.25],
            }}
            transition={{
              duration: p.duration,
              delay: p.delay,
              // per-property timing so gravity feels real
              y:       { times: [0, 0.32, 1], ease: ['easeOut', 'easeIn'], duration: p.duration, delay: p.delay },
              x:       { times: [0, 0.32, 1], ease: ['easeOut', 'easeIn'], duration: p.duration, delay: p.delay },
              opacity: { times: [0, 0.5, 0.78, 1],                         duration: p.duration, delay: p.delay },
              rotate:  { ease: 'linear', duration: p.duration, delay: p.delay },
              scale:   { times: [0, 0.5, 1],                                duration: p.duration, delay: p.delay },
            }}
            style={{
              position: 'absolute',
              left: '50%',
              top: '34%',
              ...shapeStyle(p.shape, p.size, p.color),
            }}
          />
        );
      })}

      {/* ── Pulsing halo rings (behind the icon) ─────────────────────── */}
      {/* Centering is on the plain wrapper div — Framer Motion owns transform on the inner div */}
      {showPopper && [0, 1, 2].map(i => (
        <div
          key={`halo-${i}`}
          style={{
            position: 'absolute', left: '50%', top: '34%',
            transform: 'translate(-50%,-50%)',
            width: 180, height: 180,
            pointerEvents: 'none',
          }}
        >
          <motion.div
            initial={{ opacity: 0.75, scale: 0.25 }}
            animate={{ opacity: 0, scale: 2.6 }}
            transition={{ duration: 1.9, delay: i * 0.52, repeat: Infinity, ease: [0.15, 0.7, 0.3, 1] }}
            style={{
              width: '100%', height: '100%', borderRadius: '50%',
              border: `${2.5 - i * 0.5}px solid ${isDark ? 'rgba(255,122,26,0.72)' : 'rgba(195,85,8,0.62)'}`,
              pointerEvents: 'none',
            }}
          />
        </div>
      ))}

      {/* ── Rotating 12-spoke starburst (behind emoji) ────────────────── */}
      {/* Same pattern: centering wrapper div, Framer Motion handles rotate inside */}
      {showPopper && (
        <div style={{ position: 'absolute', left: '50%', top: '34%', transform: 'translate(-50%,-50%)', pointerEvents: 'none' }}>
          <motion.div
            animate={{ rotate: 360 }}
            transition={{ duration: 12, repeat: Infinity, ease: 'linear' }}
            style={{ pointerEvents: 'none' }}
          >
            <svg width="280" height="280" viewBox="0 0 280 280" style={{ overflow: 'visible', display: 'block' }}>
              {Array.from({ length: 12 }, (_, i) => {
                const a = (i * 30) * Math.PI / 180;
                return (
                  <line key={i}
                    x1="140" y1="140"
                    x2={140 + Math.cos(a) * 130}
                    y2={140 + Math.sin(a) * 130}
                    stroke={isDark ? 'rgba(255,122,26,0.14)' : 'rgba(195,85,8,0.11)'}
                    strokeWidth="2"
                  />
                );
              })}
            </svg>
          </motion.div>
        </div>
      )}

      {/* ── Central shield + checkmark (Pro activated) ───────────────── */}
      <div style={{ position: 'absolute', left: '50%', top: '34%', transform: 'translate(-50%,-50%)', pointerEvents: 'none' }}>
        <AnimatePresence>
          {showPopper && (
            <motion.div
              initial={{ opacity: 0, scale: 0, rotate: -18 }}
              animate={{ opacity: 1, scale: 1.35, rotate: 0 }}
              exit={{ opacity: 0, scale: 0.28, rotate: 12 }}
              transition={{ duration: 0.54, ease: [0.18, 0.85, 0.18, 1] }}
              style={{ filter: popperGlow, display: 'flex', alignItems: 'center', justifyContent: 'center' }}
            >
              {/* gentle float loop */}
              <motion.div
                animate={{ y: [0, -9, 0, -5, 0] }}
                transition={{ duration: 2.4, repeat: Infinity, ease: 'easeInOut', delay: 0.6 }}
              >
                <svg
                  width="168" height="185"
                  viewBox="0 0 100 110"
                  style={{ display: 'block', overflow: 'visible' }}
                >
                  <defs>
                    <linearGradient id="sg" x1="25%" y1="0%" x2="75%" y2="100%">
                      <stop offset="0%"   stopColor="#ffb86a" />
                      <stop offset="55%"  stopColor="#ff7a1a" />
                      <stop offset="100%" stopColor="#bf4c00" />
                    </linearGradient>
                    {/* convex highlight */}
                    <linearGradient id="sh" x1="0%" y1="0%" x2="0%" y2="100%">
                      <stop offset="0%"   stopColor="rgba(255,255,255,0.22)" />
                      <stop offset="100%" stopColor="rgba(255,255,255,0)" />
                    </linearGradient>
                  </defs>

                  {/* shield body */}
                  <path
                    d="M 50 5 L 93 23 L 93 57 C 93 84 73 101 50 111 C 27 101 7 84 7 57 L 7 23 Z"
                    fill="url(#sg)"
                    stroke={isDark ? 'rgba(255,185,90,0.65)' : 'rgba(190,110,0,0.55)'}
                    strokeWidth="2.5"
                  />

                  {/* convex sheen overlay */}
                  <path
                    d="M 50 5 L 93 23 L 93 57 C 93 84 73 101 50 111 C 27 101 7 84 7 57 L 7 23 Z"
                    fill="url(#sh)"
                  />

                  {/* checkmark — draws on after shield lands */}
                  <motion.path
                    d="M 24 57 L 42 76 L 76 40"
                    stroke="white"
                    strokeWidth="9"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    fill="none"
                    initial={{ pathLength: 0, opacity: 0 }}
                    animate={{ pathLength: 1, opacity: 1 }}
                    transition={{
                      pathLength: { duration: 0.42, delay: 0.38, ease: [0.4, 0, 0.2, 1] },
                      opacity:    { duration: 0.01, delay: 0.38 },
                    }}
                    style={{ filter: 'drop-shadow(0 1px 4px rgba(0,0,0,0.4))' }}
                  />
                </svg>
              </motion.div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* ── Congratulations banner ─────────────────────────────────────── */}
      <div style={{ position: 'absolute', left: '50%', top: '56%', transform: 'translateX(-50%)', maxWidth: '92vw' }}>
        <AnimatePresence>
          {showBanner && (
            <motion.div
              initial={{ opacity: 0, y: 40, scale: 0.85 }}
              animate={{ opacity: 1, y: 0,  scale: 1 }}
              exit={{ opacity: 0, y: -20, scale: 0.92 }}
              transition={{ duration: 0.38, ease: [0.2, 0.8, 0.2, 1] }}
              style={{
                position: 'relative', textAlign: 'center',
                padding: '24px 56px 22px', borderRadius: 16,
                background: bannerBg, border: bannerBorder, boxShadow: bannerShadow,
                color: mainText, fontFamily: 'var(--font-mono, monospace)',
                minWidth: 400, maxWidth: '90vw', pointerEvents: 'auto',
              }}
            >
              <button
                type="button" onClick={closeAll} aria-label="Close"
                style={{
                  position: 'absolute', top: 8, right: 10, width: 26, height: 26,
                  border: `1px solid ${closeBorder}`, background: closeBg, color: closeColor,
                  cursor: 'pointer', borderRadius: 4,
                  fontFamily: 'var(--font-mono, monospace)', fontSize: 14, lineHeight: 1,
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  padding: 0, pointerEvents: 'auto',
                }}
              >×</button>

              <div style={{ fontSize: 13, fontWeight: 800, letterSpacing: '0.4em', textTransform: 'uppercase', color: accentColor, marginBottom: 8, textShadow: accentGlow }}>
                🥳 Congratulations
              </div>
              <div style={{ fontSize: 20, fontWeight: 700, letterSpacing: '0.04em', color: mainText, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {message ?? 'WinCommander Pro is now active!'}
              </div>
              <div style={{ marginTop: 10, fontSize: 11, color: subText, letterSpacing: '0.12em' }}>
                All paid features unlocked · Enjoy
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>,
    document.body
  );
}
