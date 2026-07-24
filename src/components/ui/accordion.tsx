// Height + fade treatment for accordion open/close.
// WHY: the bare Radix Content element snaps because it has no animation;
// tw-animate-css ships accordion-down/up keyframes that read
// --radix-accordion-content-height so height never needs to be hardcoded.
// Fade is layered on the inner wrapper (opacity only — GPU composited).
// Under reduced motion the wc-no-motion block collapses animation-duration
// to 0.01ms, giving an instant result without JS branching.
import * as React from "react"
import * as AccordionPrimitive from "@radix-ui/react-accordion"
import { ChevronDown } from "lucide-react"

import { cn } from "../../lib/utils"

function Accordion({
  ...props
}: React.ComponentProps<typeof AccordionPrimitive.Root>) {
  return <AccordionPrimitive.Root data-slot="accordion" {...props} />
}

function AccordionItem({
  className,
  ...props
}: React.ComponentProps<typeof AccordionPrimitive.Item>) {
  return (
    <AccordionPrimitive.Item
      data-slot="accordion-item"
      className={cn("border-b border-[var(--border)]", className)}
      {...props}
    />
  )
}

function AccordionTrigger({
  className,
  children,
  ...props
}: React.ComponentProps<typeof AccordionPrimitive.Trigger>) {
  return (
    <AccordionPrimitive.Header className="flex">
      <AccordionPrimitive.Trigger
        data-slot="accordion-trigger"
        className={cn(
          "flex flex-1 items-center justify-between py-3 text-left text-[13px] font-medium text-[var(--text)] transition-colors duration-150 hover:text-[var(--accent)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] disabled:pointer-events-none disabled:opacity-50 [&[data-state=open]>svg]:rotate-180",
          className,
        )}
        {...props}
      >
        {children}
        <ChevronDown className="size-4 shrink-0 text-[var(--text-mute)] transition-transform duration-200 [transition-timing-function:var(--ease)]" />
      </AccordionPrimitive.Trigger>
    </AccordionPrimitive.Header>
  )
}

function AccordionContent({
  className,
  children,
  ...props
}: React.ComponentProps<typeof AccordionPrimitive.Content>) {
  return (
    // accordion-down/up animate height via --radix-accordion-content-height.
    // fill-mode-forwards keeps height:0 after close so content doesn't flash.
    <AccordionPrimitive.Content
      data-slot="accordion-content"
      className="overflow-hidden text-[13px] text-[var(--text-dim)] data-[state=closed]:animate-accordion-up data-[state=open]:animate-accordion-down fill-mode-forwards"
      {...props}
    >
      {/* Inner wrapper fades content independently of the height clip.
          group-data-[state] reads the state from AccordionContent (the parent),
          which Radix marks with data-state=open|closed. */}
      <div
        className={cn(
          "pb-3 transition-opacity duration-200 [transition-timing-function:var(--ease)]",
          "[[data-state=closed]_&]:opacity-0 [[data-state=open]_&]:opacity-100",
          className,
        )}
      >
        {children}
      </div>
    </AccordionPrimitive.Content>
  )
}

export { Accordion, AccordionItem, AccordionTrigger, AccordionContent }
