import * as React from "react"
import { cn } from "../../lib/utils"

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "h-9 w-full rounded-[var(--r)] border border-[var(--border)] bg-[var(--surface-2)] px-3 text-[13px] text-[var(--text)] placeholder:text-[var(--text-mute)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] focus-visible:border-[var(--accent-line)] focus-visible:shadow-[0_0_16px_-3px_var(--glow)] transition-[color,background-color,border-color,box-shadow] duration-[var(--dur-fast)] [transition-timing-function:var(--ease)] disabled:opacity-50 disabled:pointer-events-none",
        className
      )}
      {...props}
    />
  )
}

export { Input }
