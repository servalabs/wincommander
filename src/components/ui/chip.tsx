import * as React from "react"
import { motion } from "framer-motion"
import { SPRING } from "../shared/motion"
import { cn } from "../../lib/utils"

interface ChipProps {
  active?: boolean
  onClick?: () => void
  children: React.ReactNode
  className?: string
}

// Spring press via SPRING.snappy (same curve as Button) — chips dip slightly
// less (0.94 vs Button's 0.9) because they're smaller targets. gpu-clip mirrors
// Button's Tauri WebView2 corner-bleed fix on scale transforms. Under reduced
// motion MotionConfig (App.tsx) collapses whileTap to instant.
function Chip({ active = false, onClick, children, className }: ChipProps) {
  return (
    <motion.button
      type="button"
      data-slot="chip"
      aria-pressed={active}
      onClick={onClick}
      whileTap={{ scale: 0.94 }}
      transition={SPRING.snappy}
      className={cn(
        "gpu-clip rounded-full border px-3 py-1 text-[12.5px] transition-colors duration-[var(--dur-fast)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)]",
        active
          ? "bg-[var(--accent-soft)] border-[var(--accent-line)] text-[var(--accent)]"
          : "bg-[var(--surface-2)] border-[var(--border)] text-[var(--text-dim)] hover:text-[var(--text)]",
        className
      )}
    >
      {children}
    </motion.button>
  )
}

export { Chip }
