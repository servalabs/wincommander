// src/components/shared/AnimatedNumber.tsx
// Treatment: "Count-up + pop" — chosen 2026-06-15 (motion-options.html).
//
// WHY rAF tween + separate spring pop instead of useSpring for the number:
// useSpring on a numeric value overshoots and produces non-integer display
// values that flicker. A linear rAF tween reads more precisely (suitable for
// radar scores, health %, server specs), then a spring scale-pop signals
// "settled" without affecting the numeric readability.

import { motion, useAnimation } from "framer-motion";
import { useEffect, useRef, useState } from "react";
import useMotionPreference from "../../hooks/useMotionPreference";
import { DURATION, SPRING } from "./motion";

interface AnimatedNumberProps {
  value: number;
  className?: string;
  /** Optional formatter — defaults to Math.round. */
  format?: (n: number) => string;
  /** Tween duration in ms. Defaults to DURATION.celebrate (500ms). */
  durationMs?: number;
}

const DEFAULT_FORMAT = (n: number) => String(Math.round(n));

/**
 * AnimatedNumber — tweens the displayed number toward `value`, then fires a
 * brief scale-pop on settle to confirm completion.
 *
 * Reduced motion: jumps to final value instantly, skips the pop.
 * Framer MotionConfig (App.tsx) collapses the spring variant automatically;
 * we additionally skip the rAF loop entirely so no intermediate renders fire.
 */
export default function AnimatedNumber({
  value,
  className,
  format = DEFAULT_FORMAT,
  durationMs = DURATION.celebrate,
}: AnimatedNumberProps) {
  const pref = useMotionPreference();
  const reduced = pref === "reduced";

  const [displayed, setDisplayed] = useState<number>(value);
  const rafRef = useRef<number | null>(null);
  // Track the start-of-tween baseline so direction changes mid-tween work.
  const fromRef = useRef<number>(value);
  const startTimeRef = useRef<number | null>(null);

  const popControls = useAnimation();

  useEffect(() => {
    // Under reduced motion: jump immediately, no animation.
    if (reduced) {
      setDisplayed(value);
      return;
    }

    // Cancel any in-flight tween before starting a new one.
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
    }

    const from = displayed;
    fromRef.current = from;
    startTimeRef.current = null;

    const tick = (now: number) => {
      if (startTimeRef.current === null) startTimeRef.current = now;
      const elapsed = now - startTimeRef.current;
      const progress = Math.min(elapsed / durationMs, 1);

      // Apply the house easing curve [0.22,0.61,0.36,1] via a cubic approximation.
      // We can't call framer's easing directly on a rAF loop, so we use the
      // closed-form value of the same "calm decel" feel: ease-out-quart gives a
      // perceptually close match without adding a dep for one interpolation.
      const t = 1 - Math.pow(1 - progress, 4); // ease-out-quart ≈ EASE.standard feel

      setDisplayed(from + (value - from) * t);

      if (progress < 1) {
        rafRef.current = requestAnimationFrame(tick);
      } else {
        // Tween complete — fire the scale-pop to signal "settled".
        rafRef.current = null;
        popControls.start({
          scale: [1, 1.12, 1],
          transition: { ...SPRING.gentle },
        });
      }
    };

    rafRef.current = requestAnimationFrame(tick);

    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    };
    // `displayed` is intentionally NOT in deps — we only want the snapshot at
    // the moment the prop changes, which fromRef.current captures inside the effect.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, durationMs, reduced]);

  return (
    // gpu-clip prevents Tauri corner-bleed on scale transforms (pattern from SovereigntyRing).
    <motion.span
      className={`gpu-clip${className ? ` ${className}` : ""}`}
      animate={popControls}
      style={{ display: "inline-block" }}
    >
      {format(displayed)}
    </motion.span>
  );
}
