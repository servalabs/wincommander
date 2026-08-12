// src/components/shared/SuccessFill.tsx
// Treatment: "Green sweep fill" — chosen 2026-06-15 (motion-options.html).
//
// WHY CSS keyframe instead of a framer-motion animate:
// The sweep is a background-fill scaleX — a single GPU-composited transform.
// A CSS @keyframes animation on an ::after pseudo requires zero JS per-frame
// and no framer overhead. The check-mark settle is framer so it participates
// in the global <MotionConfig reducedMotion> gate automatically.
//
// DENIABILITY NOTE (DN-07):
// This component is for ordinary task completion (cleanup done, apply succeeded,
// scan finished). It must NOT be auto-fired by any feature — callers opt in by
// rendering <SuccessFill active />. It is trivially omittable on any surface
// where the sweep would read as conspicuous confirmation cue.
//
// Usage:
//   <div style={{ position: "relative", overflow: "hidden" }}>
//     <SuccessFill active={isDone} />
//     <MyRowContent />
//   </div>
//
// Or via the hook for imperative surfaces:
//   const { active, trigger } = useSuccessFill();
//   <SuccessFill active={active} />

import { motion, AnimatePresence } from "framer-motion";
import { useCallback, useState } from "react";
import useMotionPreference from "../../hooks/useMotionPreference";
import { SPRING } from "./motion";

interface SuccessFillProps {
  /** When true, the sweep fires and the check settles in. */
  active: boolean;
  /**
   * Optional accessible label for the check mark (sr-only if provided).
   * Defaults to "Done".
   */
  label?: string;
  className?: string;
}

/**
 * SuccessFill — overlays a green sweep + check on any position:relative
 * host element when `active` is true.
 *
 * The sweep uses the .wc-success-sweep CSS utility (::after pseudo, scaleX
 * 0→1, fades out at end). The check mark is a framer-motion element so it
 * participates in the global MotionConfig gate.
 *
 * The host element must have:  position: relative; overflow: hidden;
 * (SuccessFill renders absolutely inside it.)
 *
 * Reduced motion: skips the CSS sweep class; shows the check instantly
 * (MotionConfig collapses the spring to its final state).
 */
export default function SuccessFill({ active, label = "Done", className }: SuccessFillProps) {
  const pref = useMotionPreference();
  const reduced = pref === "reduced";

  if (!active) return null;

  return (
    // Absolute fill so the host's own layout is untouched.
    <span
      // wc-success-sweep fires the CSS keyframe sweep via ::after.
      // Skip under reduced motion — only show the check.
      className={[
        "wc-success-fill",
        !reduced ? "wc-success-sweep" : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
      aria-label={label}
      role="status"
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        pointerEvents: "none",
      }}
    >
      {/* Check mark settles in via spring — collapses instantly under MotionConfig. */}
      <AnimatePresence>
        <motion.svg
          key="check"
          viewBox="0 0 20 20"
          fill="none"
          width={18}
          height={18}
          // gpu-clip: compositor hint to prevent Tauri corner-bleed on scale.
          className="gpu-clip"
          initial={{ scale: 0.4, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0.4, opacity: 0 }}
          transition={{ ...SPRING.gentle }}
          aria-hidden
        >
          <path
            d="M4 10.5L8.5 15L16 6"
            stroke="var(--ok)"
            strokeWidth={2.2}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </motion.svg>
      </AnimatePresence>
    </span>
  );
}

/**
 * useSuccessFill — imperative trigger for callers that respond to async
 * operations (e.g. "clear complete", "apply done").
 *
 * Returns { active, trigger } — call trigger() when the operation succeeds;
 * `active` resets to false after `resetMs` (default 2000ms).
 *
 * Usage:
 *   const { active, trigger } = useSuccessFill();
 *   <button onClick={async () => { await doWork(); trigger(); }}>Apply</button>
 *   <SuccessFill active={active} />
 */
export function useSuccessFill(resetMs = 2000) {
  const [active, setActive] = useState(false);

  const trigger = useCallback(() => {
    setActive(true);
    // Auto-reset so the fill doesn't persist forever after the operation.
    const id = setTimeout(() => setActive(false), resetMs);
    return () => clearTimeout(id);
  }, [resetMs]);

  return { active, trigger };
}
