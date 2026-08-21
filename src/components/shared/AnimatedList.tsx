// src/components/shared/AnimatedList.tsx
// Treatment: "Fade + height" — chosen 2026-06-15 (motion-options.html).
//
// WHY grid-template-rows instead of tweening height px:
// Tweening height triggers layout/reflow on every frame — disqualified by the
// N100 low-power target. grid-template-rows: 0fr → 1fr animates the available
// space via the grid algorithm, which browsers can handle on the compositor
// for simple column counts.
//
// WHY capped stagger:
// A 100-item list would take 100 * step ≈ seconds to finish if uncapped.
// Cap at MAX_STAGGER so the last item never waits more than 250ms regardless
// of list length.

import { AnimatePresence, motion } from "framer-motion";
import type { CSSProperties, ReactNode } from "react";
import useMotionPreference from "../../hooks/useMotionPreference";
import { DURATION_S, EASE } from "./motion";

// Per-item stagger step and ceiling.
const STAGGER_STEP = 0.04; // 40ms between items
const MAX_STAGGER = 0.25; // never wait more than 250ms for any item

interface AnimatedRowProps {
  /** Unique, stable key — callers must pass this as React key AND layoutId. */
  layoutId?: string;
  children: ReactNode;
  className?: string;
  /**
   * Staggered entrance delay in seconds (use staggerDelay(idx)).
   * Applied only to the enter transition so exit is never delayed
   * (slow exits feel like the UI is hanging).
   */
  entranceDelay?: number;
}

interface AnimatedListProps {
  children: ReactNode;
  className?: string;
  /** Inline styles forwarded to the wrapper div (e.g. maxHeight, overflowY). */
  style?: CSSProperties;
}

/**
 * AnimatedRow — wraps a single list item with fade + height expand/collapse.
 *
 * The `layout` prop lets framer reorder rows smoothly when the list is sorted
 * or filtered without triggering mount/unmount cycles for stable items.
 *
 * Reduced motion: MotionConfig in App.tsx collapses all framer variants to
 * their final state instantly, so no explicit branch is needed here — the
 * wrapper still renders; it just skips transforms.
 */
export function AnimatedRow({ layoutId, children, className, entranceDelay = 0 }: AnimatedRowProps) {
  // Use variants so we can attach different `transition` objects per state.
  // Entrance: fade in with optional stagger delay.
  // Exit: fade out immediately — no delay so the UI never feels hung.
  // Reorder (layout): framer handles position smoothly without mount/unmount.
  const rowVariants = {
    initial: { opacity: 0 },
    animate: {
      opacity: 1,
      transition: { duration: DURATION_S.fast, ease: EASE.enter, delay: entranceDelay },
    },
    exit: {
      opacity: 0,
      // Exit delay is always 0 — a delayed exit feels like the UI is ignoring
      // the user's action. Entrance stagger is a UX nicety; exit speed is UX necessity.
      transition: { duration: DURATION_S.fast, ease: EASE.exit, delay: 0 },
    },
  };

  return (
    // Outer motion.div handles opacity (compositor-friendly).
    // `layout` drives smooth reorder when enabledRowsFirst re-sorts after a
    // toggle — the row slides to its new position rather than snapping.
    <motion.div
      layout
      layoutId={layoutId}
      variants={rowVariants}
      initial="initial"
      animate="animate"
      exit="exit"
      className={className}
      // Clip the height-expand so content doesn't paint outside the row
      // while the grid rows tween open.
      style={{ overflow: "hidden" }}
    >
      {/* Inner div drives the height via grid trick — no reflow on each frame. */}
      <motion.div
        initial={{ gridTemplateRows: "0fr" }}
        animate={{ gridTemplateRows: "1fr" }}
        exit={{ gridTemplateRows: "0fr" }}
        // No delay on the inner height collapse — exits should be snappy.
        transition={{ duration: DURATION_S.fast, ease: EASE.enter }}
        style={{ display: "grid" }}
      >
        {/* min-content child is required by the grid-template-rows hack. */}
        <div style={{ minHeight: 0 }}>{children}</div>
      </motion.div>
    </motion.div>
  );
}

export function AnimatedTableRow({ layoutId, children, className, entranceDelay = 0 }: AnimatedRowProps) {
  const rowVariants = {
    initial: { opacity: 0, y: 3 },
    animate: {
      opacity: 1,
      y: 0,
      transition: { duration: DURATION_S.fast, ease: EASE.enter, delay: entranceDelay },
    },
    exit: {
      opacity: 0,
      y: -2,
      transition: { duration: DURATION_S.fast, ease: EASE.exit, delay: 0 },
    },
  };

  return (
    <motion.div
      layout
      layoutId={layoutId}
      variants={rowVariants}
      initial="initial"
      animate="animate"
      exit="exit"
      className={className}
    >
      {children}
    </motion.div>
  );
}

/**
 * AnimatedList — wraps a list of <AnimatedRow> children (or any motion
 * elements) with AnimatePresence so exits animate out before DOM removal.
 *
 * Stagger is applied via the `custom` prop + `variants` pattern so every
 * child receives its index-derived delay without needing a special prop.
 * Callers need only wrap children in <AnimatedRow key={id}>.
 *
 * Usage (simplest):
 *   <AnimatedList>
 *     {items.map((item, i) => (
 *       <AnimatedRow key={item.id} layoutId={item.id}>
 *         <MyRowContent item={item} />
 *       </AnimatedRow>
 *     ))}
 *   </AnimatedList>
 */
export function AnimatedList({ children, className, style }: AnimatedListProps) {
  const pref = useMotionPreference();

  // Under reduced motion: render children directly, no AnimatePresence overhead.
  // MotionConfig already collapses framer transforms, but skipping AnimatePresence
  // removes the exit-hold delay that would otherwise stall DOM removal.
  if (pref === "reduced") {
    return <div className={className} style={style}>{children}</div>;
  }

  return (
    <div className={className} style={style}>
      <AnimatePresence initial={false}>{children}</AnimatePresence>
    </div>
  );
}

/**
 * staggerDelay — helper for callers that want index-based delay on enter.
 * Pass the result to the <AnimatedRow> wrapper's `transition` override.
 *
 * Usage:
 *   <AnimatedRow key={id} style={{}} transition={{ delay: staggerDelay(i) }}>
 *
 * The cap ensures a 100-item list never staggers beyond 250ms total.
 */
export function staggerDelay(index: number): number {
  return Math.min(index * STAGGER_STEP, MAX_STAGGER);
}
