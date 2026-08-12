// Sliding pill treatment: single framer-motion element with layoutId
// slides between options. Simpler than tabs because value is a direct prop.
// WHY: same reasoning as tabs — CSS background swap is instant; layoutId
// springs the pill geometry to give users spatial orientation of their choice.
// Under reduced motion: MotionConfig(reducedMotion="always") in App.tsx
// collapses layout animation to instant.
import * as React from "react"
import { motion } from "framer-motion"

import { SPRING } from "../shared/motion"
import { cn } from "../../lib/utils"

interface SegmentedOption {
  value: string
  label: React.ReactNode
}

interface SegmentedProps {
  value: string
  onValueChange: (value: string) => void
  options: SegmentedOption[]
  size?: "sm" | "default"
  className?: string
}

function Segmented({
  value,
  onValueChange,
  options,
  size = "default",
  className,
}: SegmentedProps) {
  // framer-motion treats layoutId as document-global, so two mounted Segmented
  // groups would fling the pill across the screen into each other. Scope it per
  // instance.
  const pillId = React.useId()
  return (
    <div
      role="tablist"
      data-slot="segmented"
      className={cn(
        "inline-flex gap-0.5 rounded-[var(--r)] bg-[var(--surface-2)] p-1",
        className,
      )}
    >
      {options.map((option) => {
        const selected = option.value === value
        return (
          // position:relative contains the absolute pill within each option.
          <button
            key={option.value}
            type="button"
            role="tab"
            aria-pressed={selected}
            aria-selected={selected}
            onClick={() => onValueChange(option.value)}
            className={cn(
              "relative rounded-[var(--r-sm)] font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)]",
              size === "sm" ? "px-2 py-0.5 text-[11.5px]" : "px-3 py-1 text-[12.5px]",
              selected ? "text-[var(--accent)]" : "text-[var(--text-dim)] hover:text-[var(--text)]",
            )}
          >
            {/* Pill mounts only on the selected option. framer animates
                the shared geometry when it moves between options. */}
            {selected && (
              <motion.span
                layoutId={`segmented-pill-${pillId}`}
                className="absolute inset-0 rounded-[var(--r-sm)] bg-[var(--accent-soft)]"
                style={{ zIndex: 0 }}
                transition={SPRING.snappy}
                aria-hidden
              />
            )}
            {/* Label sits above the pill. */}
            <span className="relative" style={{ zIndex: 1 }}>
              {option.label}
            </span>
          </button>
        )
      })}
    </div>
  )
}

export { Segmented }
