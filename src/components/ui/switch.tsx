import * as React from "react"
import * as SwitchPrimitive from "@radix-ui/react-switch"
import { motion } from "framer-motion"
import { cn } from "../../lib/utils"
import { SPRING } from "../shared/motion"
import useMotionPreference from "../../hooks/useMotionPreference"

// TODO (call-site consolidation): This Radix-backed Switch and WCSwitch share
// identical spring-knob motion now. A future application phase should pick one
// as the canonical implementation and migrate all call sites to it.

function Switch({
  className,
  checked,
  defaultChecked,
  onCheckedChange,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
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
      className={cn(
        // Track crossfade uses CSS var tokens (SSOT: v2-theme.css --dur-normal).
        // transition-colors replaced with explicit property list so we don't
        // accidentally transition layout properties via the Tailwind shorthand.
        // overflow-hidden: SPRING.knob is underdamped by design (physical
        // overshoot) — clip the track so the thumb never pokes out past the
        // pill edge mid-animation.
        "inline-flex h-[27px] w-[46px] shrink-0 items-center overflow-hidden rounded-full border border-[var(--switch-off-border)] bg-[var(--switch-off-bg)] [transition:background_var(--dur-normal,200ms)_var(--ease),border-color_var(--dur-normal,200ms)_var(--ease),filter_var(--dur-fast,150ms)_var(--ease)] data-[state=checked]:bg-[var(--accent)] data-[state=checked]:border-[var(--accent-line)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] hover:brightness-110 disabled:opacity-50 disabled:pointer-events-none cursor-pointer",
        className
      )}
      {...props}
    >
      {/* SwitchPrimitive.Thumb is purely visual (no a11y role). We replace it
          with a motion.span so framer drives translateX with SPRING.knob.
          This gives the same spring-knob feel as WCSwitch. */}
      <motion.span
        data-slot="switch-thumb"
        aria-hidden
        className={cn(
          "block size-5 rounded-full shadow [transition:background-color_var(--dur-normal,200ms)_var(--ease)]",
          isOn ? "bg-[var(--accent-contrast)]" : "bg-[var(--switch-off-thumb)]"
        )}
        animate={{ x: isOn ? 22 : 3 }}
        transition={knobTransition}
        style={{ willChange: "transform" }}
      />
    </SwitchPrimitive.Root>
  )
}

export { Switch }
