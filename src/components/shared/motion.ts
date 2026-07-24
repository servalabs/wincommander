// src/components/shared/motion.ts
// Single source of truth for all motion: durations, easings, springs.
// Import these everywhere — never hardcode ms or cubic-bezier literals.
// CSS parity lives in src/styles/v2-theme.css (--dur-*, --ease); keep in sync.

/** Canonical duration scale (milliseconds). */
export const DURATION = {
  instant: 100, // micro-feedback (sweep, flash)
  fast: 150, // toggle response, panel exit
  normal: 200, // panel enter, state transitions
  slow: 300, // overlays, card pulse
  celebrate: 500, // rare — threshold/success moments
} as const;

/** Same scale in seconds, for framer-motion transition durations. */
export const DURATION_S = {
  instant: DURATION.instant / 1000,
  fast: DURATION.fast / 1000,
  normal: DURATION.normal / 1000,
  slow: DURATION.slow / 1000,
  celebrate: DURATION.celebrate / 1000,
} as const;

type Cubic = [number, number, number, number];

/** House easing curves. `standard` is the calm decel baseline used everywhere. */
export const EASE = {
  standard: [0.22, 0.61, 0.36, 1] as Cubic, // calm decel — the one baseline
  enter: [0, 0, 0.2, 1] as Cubic, // decelerate-in
  exit: [0.4, 0, 1, 1] as Cubic, // accelerate-out
} as const;

/** String form of EASE.standard for inline styles / CSS-var parity. */
export const cssEase = "cubic-bezier(0.22,0.61,0.36,1)";

/** Spring presets — reserved for accents, not routine transitions. */
export const SPRING = {
  gentle: { type: "spring", stiffness: 300, damping: 26 }, // success/confirmation accents
  snappy: { type: "spring", stiffness: 500, damping: 35 }, // shared-element (tab/sidebar indicator)
  knob: { type: "spring", stiffness: 420, damping: 28 }, // switch/toggle knob
} as const;

// ── Backward-compatible aliases (pre-existing import names) ──
/** @deprecated prefer DURATION (kept for older imports; `glacial` aliases `celebrate`) */
export const TIMING = { ...DURATION, glacial: DURATION.celebrate } as const;
/** @deprecated prefer EASE / SPRING (note: `standard` is now the house curve, not Material) */
export const EASING = { ...EASE, spring: SPRING.gentle } as const;

/** Panel enter/exit — chosen treatment "Crossfade + rise". Use with AnimatePresence mode="wait". */
export const panelVariants = {
  initial: { opacity: 0, y: 8 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
} as const;

export const panelTransition = {
  duration: DURATION_S.normal,
  ease: EASE.standard,
} as const;
