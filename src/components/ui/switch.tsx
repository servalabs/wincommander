import * as React from "react"
import * as SwitchPrimitive from "@radix-ui/react-switch"
import { motion } from "framer-motion"
import { cn } from "../../lib/utils"
import { SPRING } from "../shared/motion"
import useMotionPreference from "../../hooks/useMotionPreference"

// Canonical switch for the whole app (the former shared/WCSwitch was folded in).
// Radix provides role="switch", aria-checked and Space/Enter keyboard handling.

type SwitchProps = React.ComponentProps<typeof SwitchPrimitive.Root> & {
  /** `md` = 46x27 (default), `sm` = 38x22 for dense rows. */
  size?: "md" | "sm";
  /**
   * Stop click bubbling so a switch nested in a clickable card/row does not
   * also fire the parent's onClick (double-toggle).
   */
  stopClickPropagation?: boolean;
};

// Track/knob geometry per size. Knob travel keeps a 3px inset on both ends.
const SIZES = {
  md: { track: "h-[27px] w-[46px]", knob: "size-5", off: 3, on: 22 },
  sm: { track: "h-[22px] w-[38px]", knob: "size-4", off: 3, on: 19 },
} as const;

function Switch({
  className,
  checked,
  defaultChecked,
  onCheckedChange,
  size = "md",
  stopClickPropagation = false,
  onClick,
  ...props
}: SwitchProps) {
  const dims = SIZES[size];
  const pref = useMotionPreference();

  // Mirror the Radix checked state so we can drive framer-motion from it.
  // Defaults to defaultChecked for uncontrolled usage.
  const [isOn, setIsOn] = React.useState<boolean>(
    checked ?? defaultChecked ?? false
  );

  // Keep in sync when the controlled prop changes externally.
  React.useEffect(() => {
    if (checked !== undefined) setIsOn(checked);
  }, [checked]);

  // Under reduced motion: instant snap (no spring). MotionConfig in App.tsx
  // also fires but this ensures correct behaviour if rendered outside the tree.
  const knobTransition =
    pref === "reduced" ? { duration: 0 } : SPRING.knob;

  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      checked={checked}
      defaultChecked={defaultChecked}
      onCheckedChange={(next) => {
        setIsOn(next);
        onCheckedChange?.(next);
      }}
      onClick={(e) => {
        if (stopClickPropagation) e.stopPropagation();
        onClick?.(e);
      }}
      className={cn(
        // Track crossfade uses CSS var tokens (SSOT: v2-theme.css --dur-normal).
        // transition-colors replaced with explicit property list so we don't
        // accidentally transition layout properties via the Tailwind shorthand.
        // overflow-hidden: SPRING.knob is underdamped by design (physical
        // overshoot) — clip the track so the thumb never pokes out past the
        // pill edge mid-animation.
        dims.track,
        "inline-flex shrink-0 items-center overflow-hidden rounded-full border border-[var(--switch-off-border)] bg-[var(--switch-off-bg)] [transition:background_var(--dur-normal,200ms)_var(--ease),border-color_var(--dur-normal,200ms)_var(--ease),filter_var(--dur-fast,150ms)_var(--ease)] data-[state=checked]:bg-[var(--accent)] data-[state=checked]:border-[var(--accent-line)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] hover:brightness-110 disabled:opacity-50 disabled:pointer-events-none cursor-pointer",
        className
      )}
      {...props}
    >
      {/* SwitchPrimitive.Thumb is purely visual (no a11y role). We replace it
          with a motion.span so framer drives translateX with SPRING.knob.
           */}
      <motion.span
        data-slot="switch-thumb"
        aria-hidden
        className={cn(
          dims.knob,
          "block rounded-full shadow [transition:background-color_var(--dur-normal,200ms)_var(--ease)]",
          isOn ? "bg-[var(--accent-contrast)]" : "bg-[var(--switch-off-thumb)]"
        )}
        animate={{ x: isOn ? dims.on : dims.off }}
        transition={knobTransition}
        style={{ willChange: "transform" }}
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
