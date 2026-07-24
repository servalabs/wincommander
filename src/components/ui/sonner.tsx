import { Toaster as Sonner } from "sonner"
import { useTheme } from "../../context/ThemeContext"

function Toaster({
  position = "top-right",
  duration,
  offset = { top: "60px", right: "148px" },
  mobileOffset = { top: "60px", right: "16px", left: "16px" },
  ...props
}: React.ComponentProps<typeof Sonner> & { duration?: number }) {
  const { theme } = useTheme()

  return (
    <Sonner
      theme={theme}
      className="toaster group"
      position={position}
      offset={offset}
      mobileOffset={mobileOffset}
      closeButton
      toastOptions={{
        duration,
        classNames: {
          toast:
            "wc-toast-near-bell rounded-[var(--r)] border border-[var(--border-strong)] bg-[var(--surface)] text-[var(--text)] shadow-[var(--shadow)]",
          description: "text-[var(--text-dim)]",
          actionButton: "bg-[var(--accent)] text-[var(--accent-contrast)]",
          cancelButton: "bg-[var(--surface-2)] text-[var(--text-dim)]",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
