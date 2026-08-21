import * as React from "react"
import * as SliderPrimitive from "@radix-ui/react-slider"
import { cn } from "../../lib/utils"

function Slider({
  className,
  defaultValue,
  value,
  thumbAriaLabel,
  ...props
}: React.ComponentProps<typeof SliderPrimitive.Root> & {
  thumbAriaLabel?: string | ((index: number) => string)
}) {
  const thumbValues = React.useMemo(
    () =>
      Array.isArray(value)
        ? value
        : Array.isArray(defaultValue)
          ? defaultValue
          : [0],
    [value, defaultValue]
  )

  return (
    <SliderPrimitive.Root
      data-slot="slider"
      defaultValue={defaultValue}
      value={value}
      className={cn(
        "relative flex w-full touch-none select-none items-center data-[disabled]:opacity-50 data-[orientation=vertical]:h-full data-[orientation=vertical]:min-h-44 data-[orientation=vertical]:w-auto data-[orientation=vertical]:flex-col",
        className
      )}
      {...props}
    >
      <SliderPrimitive.Track
        data-slot="slider-track"
        className={cn(
          "relative grow overflow-hidden rounded-full bg-[var(--surface-3)] data-[orientation=horizontal]:h-[6px] data-[orientation=horizontal]:w-full data-[orientation=vertical]:h-full data-[orientation=vertical]:w-[6px]"
        )}
      >
        <SliderPrimitive.Range
          data-slot="slider-range"
          className={cn(
            "absolute bg-[var(--accent)] data-[orientation=horizontal]:h-full data-[orientation=vertical]:w-full"
          )}
        />
      </SliderPrimitive.Track>
      {thumbValues.map((_, index) => (
        <SliderPrimitive.Thumb
          data-slot="slider-thumb"
          key={index}
          aria-label={typeof thumbAriaLabel === "function" ? thumbAriaLabel(index) : thumbAriaLabel}
          className="block size-4 shrink-0 rounded-full bg-[var(--accent)] border border-[var(--accent-line)] shadow-[0_0_0_4px_var(--accent-soft)] hover:shadow-[0_0_0_6px_var(--accent-soft)] transition-[box-shadow] duration-150 [transition-timing-function:var(--ease)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent-line)] disabled:pointer-events-none disabled:opacity-50"
        />
      ))}
    </SliderPrimitive.Root>
  )
}

export { Slider }
