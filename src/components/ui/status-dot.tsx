import { cn } from "../../lib/utils"

type StatusTone = "ok" | "warn" | "danger" | "off" | "accent"

interface StatusDotProps {
  tone?: StatusTone
  /** Adds a soft expanding halo for states that are actively "on watch". */
  pulse?: boolean
  className?: string
}

const toneColor: Record<StatusTone, string> = {
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
  off: "var(--text-mute)",
  accent: "var(--accent)",
}

function StatusDot({ tone = "off", pulse = false, className }: StatusDotProps) {
  const color = toneColor[tone]
  return (
    <span
      data-slot="status-dot"
      className={cn("inline-block size-2 rounded-full", pulse && "wc-status-ping", className)}
      style={{
        background: color,
        // `color` feeds the ::after ping (currentColor); harmless otherwise.
        color,
        boxShadow: `0 0 0 3px color-mix(in srgb, ${color} 25%, transparent)`,
      }}
    />
  )
}

export { StatusDot }
