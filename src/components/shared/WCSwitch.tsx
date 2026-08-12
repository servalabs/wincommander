import { memo } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { DURATION_S, SPRING } from "./motion";
import useMotionPreference from "../../hooks/useMotionPreference";
import "./WCSwitch.css";

interface WCSwitchProps {
  /** Controlled on/off state. */
  checked: boolean;
  /** Fired with the next value when the user flips it. */
  onChange?: (next: boolean) => void;
  disabled?: boolean;
  /** `md` = 42×24 (default), `sm` = 38×22 for dense rows. */
  size?: "md" | "sm";
  /** Accessible label (the switch renders no visible text). */
  label?: string;
  className?: string;
}

// Knob travel in px for each size variant.
// WHY separate constants: avoids re-computing inside render; keeps CSS and
// JS positions in sync without coupling to layout-property animation.
const KNOB_TRAVEL = { md: 18, sm: 16 } as const;

/**
 * WCSwitch — the universal cyan pill switch (picked 2026-06).
 * A drop-in, house-styled replacement for the BlueprintJS `<Switch>` thumb:
 * inset track, white knob, cyan glow when on. Use inside list rows, cards,
 * and anywhere a bare on/off control is needed.
 *
 * Knob animates via transform:translateX (compositor-only, no layout thrash).
 * SPRING.knob gives a slight overshoot for physical feel.
 * Under reduced motion: instant snap, no spring.
 *
 * TODO (call-site consolidation): WCSwitch and ui/switch.tsx are both active.
 * Both now share identical spring-knob motion. A future application phase
 * should pick one as the canonical switch and migrate all call sites.
 */
// memo: this is the leaf-most, most-repeated toggle primitive in the app —
// skip its framer-motion tree entirely on renders where checked/disabled/size
// didn't change (e.g. a sibling toggle's write cascading through appSettings).
function WCSwitch({
  checked,
  onChange,
  disabled = false,
  size = "md",
  label,
  className = "",
}: WCSwitchProps) {
  const pref = useMotionPreference();
  const travel = KNOB_TRAVEL[size];

  // Under reduced motion: instant snap (duration:0). Spring is suppressed
  // because <MotionConfig reducedMotion="always"> is set in App.tsx, but
  // we also explicitly zero the duration here so the CSS crossfade collapses.
  const knobTransition =
    pref === "reduced" ? { duration: 0 } : SPRING.knob;

  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={`wc-switch${checked ? " is-on" : ""}${size === "sm" ? " wc-switch--sm" : ""} ${className}`.trim()}
      onClick={(e) => {
        // WCSwitch is often nested inside a larger clickable card (ToggleTile).
        // Stop propagation so a click on the switch doesn't also fire the
        // card's own onClick and double-toggle.
        e.stopPropagation();
        if (!disabled) onChange?.(!checked);
      }}
    >
      {/* Knob: translateX only — no left, no width, no height tweening. */}
      <motion.span
        className="wc-switch__knob"
        animate={{ x: checked ? travel : 0 }}
        transition={knobTransition}
      >
        {/* Tick fades in/out with the knob state. AnimatePresence keeps the
            exit animation playing even after the node is removed from the tree. */}
        <AnimatePresence initial={false}>
          {checked && (
            <motion.span
              key="tick"
              className="wc-switch__tick-wrap"
              aria-hidden
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={pref === "reduced" ? { duration: 0 } : { duration: DURATION_S.instant }}
            />
          )}
        </AnimatePresence>
      </motion.span>
    </button>
  );
}

export default memo(WCSwitch);
