// Spring scale treatment on press: dips to ~0.9 and springs back.
// WHY: the old active:scale-[.985] uses CSS transition (no spring physics);
// framer whileTap with SPRING.snappy gives a physical snap-back feel.
// asChild (Slot) bypasses motion — keeps the rare composition case simple
// and avoids wrapping arbitrary children in a motion element.
// Under reduced motion: MotionConfig(reducedMotion="always") in App.tsx
// collapses whileTap to instant (scale stays 1), so no branching needed here.
import type * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { motion, type HTMLMotionProps } from "framer-motion"
import { cva, type VariantProps } from "class-variance-authority"

import { SPRING } from "../shared/motion"
import { cn } from "../../lib/utils"

const buttonVariants = cva(
  // active:scale-[.985] removed — spring scale via whileTap below.
  // gpu-clip added: isolation + will-change:transform prevents Tauri corner-bleed
  // on scale transforms (same compositing fix used in SovereigntyRadar).
  "gpu-clip inline-flex items-center justify-center gap-2 rounded-[var(--r)] font-semibold whitespace-nowrap transition-[background,color,box-shadow] duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] disabled:opacity-50 disabled:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default:
          "bg-[var(--surface-2)] text-[var(--text)] border border-[var(--border)] hover:bg-[var(--surface-3)] hover:border-[var(--border-strong)]",
        primary:
          "bg-[var(--accent)] text-[var(--accent-contrast)] shadow-[0_0_0_1px_var(--accent-line),0_6px_18px_-6px_var(--glow)] hover:bg-[var(--accent-2)] hover:shadow-[0_0_0_1px_var(--accent-line),0_10px_26px_-8px_var(--glow)]",
        ghost:
          "bg-transparent text-[var(--text-dim)] hover:bg-[var(--surface-2)] hover:text-[var(--text)]",
        danger:
          "bg-transparent text-[var(--danger)] border border-[color-mix(in_srgb,var(--danger)_40%,transparent)] hover:bg-[color-mix(in_srgb,var(--danger)_12%,transparent)]",
        outline:
          "border border-[var(--border-strong)] bg-transparent text-[var(--text)] hover:bg-[var(--surface-2)]",
      },
      size: {
        default: "h-9 px-[18px] text-[13px]",
        sm: "h-8 px-3 text-xs",
        lg: "h-10 px-5 text-sm",
        icon: "size-9 p-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
)

// Subtle press scale — dips to 0.9 then springs back via SPRING.snappy.
// Kept subtle: 0.9 is noticeable but not jarring on small buttons.
const PRESS_SCALE = 0.9

type ButtonVariantProps = VariantProps<typeof buttonVariants> & {
  className?: string
  asChild?: boolean
}

type ButtonProps = ButtonVariantProps &
  Omit<HTMLMotionProps<"button">, "ref" | "className">

type SlotButtonProps = ButtonVariantProps &
  Omit<React.ComponentPropsWithoutRef<"button">, "className">

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: ButtonProps) {
  // asChild composes into an arbitrary child element via Slot — skip motion
  // wrapping to avoid interfering with the caller's element type.
  if (asChild) {
    const slotProps = props as SlotButtonProps

    return (
      <Slot
        data-slot="button"
        className={cn(buttonVariants({ variant, size, className }))}
        {...slotProps}
      />
    )
  }

  return (
    <motion.button
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      whileTap={{ scale: PRESS_SCALE }}
      transition={SPRING.snappy}
      {...props}
    />
  )
}

export { Button, buttonVariants }
