import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "../../lib/utils"

const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-[var(--r-sm)] px-1.5 py-0.5 text-[10.5px] font-semibold uppercase tracking-wider font-[family-name:var(--font-mono)] border",
  {
    variants: {
      tone: {
        neutral: "bg-[var(--surface-3)] text-[var(--text-dim)] border-[var(--border)]",
        accent: "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-line)]",
        success:
          "bg-[color-mix(in_srgb,var(--ok)_15%,transparent)] text-[var(--ok)] border-[color-mix(in_srgb,var(--ok)_30%,transparent)]",
        warning:
          "bg-[color-mix(in_srgb,var(--warn)_15%,transparent)] text-[var(--warn)] border-[color-mix(in_srgb,var(--warn)_30%,transparent)]",
        danger:
          "bg-[color-mix(in_srgb,var(--danger)_15%,transparent)] text-[var(--danger)] border-[color-mix(in_srgb,var(--danger)_30%,transparent)]",
        info:
          "bg-[color-mix(in_srgb,var(--color-info)_15%,transparent)] text-[var(--color-info)] border-[color-mix(in_srgb,var(--color-info)_30%,transparent)]",
      },
    },
    defaultVariants: {
      tone: "neutral",
    },
  }
)

function Badge({
  className,
  tone,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return (
    <span
      data-slot="badge"
      className={cn(badgeVariants({ tone }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }
