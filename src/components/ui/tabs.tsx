// Sliding pill treatment: a single framer-motion element with layoutId
// slides between active triggers. Mirrors the Sidebar active-indicator pattern.
// WHY: CSS background-swap creates a harsh cut; layoutId animates the pill's
// geometry (position + size) as a spring, no DOM measurement needed.
// Under reduced motion: MotionConfig(reducedMotion="always") in App.tsx
// collapses the layout animation to instant — no branching needed here.
import * as React from "react"
import * as TabsPrimitive from "@radix-ui/react-tabs"
import { motion } from "framer-motion"

import { SPRING } from "../shared/motion"
import { cn } from "../../lib/utils"

// Shared context so TabsTrigger knows which value is currently active and which
// per-root pill layoutId to use. Populated by the Tabs wrapper; avoids
// prop-drilling through Radix primitives. The pillId scopes the framer-motion
// layoutId per Tabs instance (it is document-global otherwise, so two mounted
// Tabs groups would fling the pill across the screen into each other).
const TabsValueContext = React.createContext<{ activeValue: string | undefined; pillId: string }>({
  activeValue: undefined,
  pillId: "",
})

function Tabs({
  className,
  value,
  defaultValue,
  onValueChange,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Root>) {
  const pillId = React.useId()
  // Track active value locally so context stays current for both controlled
  // and uncontrolled usage patterns.
  const [activeValue, setActiveValue] = React.useState<string | undefined>(
    value ?? defaultValue,
  )

  const handleValueChange = React.useCallback(
    (next: string) => {
      setActiveValue(next)
      onValueChange?.(next)
    },
    [onValueChange],
  )

  // Keep in sync when controlled value changes externally.
  React.useEffect(() => {
    if (value !== undefined) setActiveValue(value)
  }, [value])

  return (
    <TabsValueContext.Provider value={{ activeValue, pillId }}>
      <TabsPrimitive.Root
        data-slot="tabs"
        className={cn("flex flex-col gap-2", className)}
        value={value}
        defaultValue={defaultValue}
        onValueChange={handleValueChange}
        {...props}
      />
    </TabsValueContext.Provider>
  )
}

function TabsList({
  className,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      data-slot="tabs-list"
      className={cn(
        "inline-flex items-center gap-1 rounded-[var(--r)] bg-[var(--surface-2)] p-1",
        className,
      )}
      {...props}
    />
  )
}

function TabsTrigger({
  className,
  value,
  children,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Trigger>) {
  const { activeValue, pillId } = React.useContext(TabsValueContext)
  const isActive = activeValue === value

  return (
    // Trigger is position:relative so the pill (absolute) is contained within it.
    <TabsPrimitive.Trigger
      data-slot="tabs-trigger"
      value={value}
      className={cn(
        "relative inline-flex items-center justify-center rounded-[var(--r-sm)] px-3 py-1.5 text-[13px] font-medium text-[var(--text-dim)] transition-colors duration-150 [transition-timing-function:var(--ease)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] disabled:pointer-events-none disabled:opacity-50 data-[state=active]:text-[var(--accent)]",
        className,
      )}
      {...props}
    >
      {/* Pill mounts only on the active trigger. When it unmounts here and
          mounts on another trigger, framer animates the shared geometry. */}
      {isActive && (
        <motion.span
          layoutId={`tabs-pill-${pillId}`}
          className="absolute inset-0 rounded-[var(--r-sm)] bg-[var(--accent-soft)]"
          style={{ zIndex: 0 }}
          transition={SPRING.snappy}
          aria-hidden
        />
      )}
      {/* Label sits above the pill via z-index. */}
      <span className="relative" style={{ zIndex: 1 }}>
        {children}
      </span>
    </TabsPrimitive.Trigger>
  )
}

function TabsContent({
  className,
  ...props
}: React.ComponentProps<typeof TabsPrimitive.Content>) {
  return (
    <TabsPrimitive.Content
      data-slot="tabs-content"
      className={cn("mt-3 outline-none", className)}
      {...props}
    />
  )
}

export { Tabs, TabsList, TabsTrigger, TabsContent }
