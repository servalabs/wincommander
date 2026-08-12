// src/components/shared/PanelBase.tsx
//
// ═══════════════════════════════════════════════════════════════════════
// PanelBase — Standardized wrapper for every panel in the app
// ═══════════════════════════════════════════════════════════════════════
//
// WHAT THIS DOES:
// Every panel duplicates the same boilerplate: panel-container div,
// panel-grid div, optional header, refresh button, loading skeleton.
// PanelBase provides all of this in one place.
//
// ANALOGY: Think of PanelBase as a "picture frame." Every painting
// (panel) is different, but they all hang on the wall the same way.
// The frame handles mounting — the artist just paints the canvas.
//
// USAGE:
//   <PanelBase title="System Tweaks" subtitle="Registry and OS-level optimizations.">
//     <SectionCard title="UI Tweaks" icon="style">
//       ...toggles...
//     </SectionCard>
//   </PanelBase>
//
// HOW IT HELPS:
//   - Consistent padding, spacing, and max-width across all panels
//   - Optional title/subtitle header (some panels like Dashboard don't need one)
//   - Two layout modes: "grid" (2-column for toggle panels) and "flow" (single column)
//   - className passthrough for panel-specific CSS overrides

import type { ReactNode } from "react";

interface PanelBaseProps {
  /** Panel content — SectionCards, toggles, custom UI, anything */
  children: ReactNode;
  /** Optional panel title shown as a header. Omit for panels that render their own header. */
  title?: string;
  /** Optional subtitle below the title */
  subtitle?: string;
  /**
   * Layout mode:
   * - "grid" = 2-column grid (default, used by privacy/tweaks)
   * - "flow" = single-column vertical stack (used by network, apps, etc.)
   */
  layout?: "grid" | "flow";
  /** Extra CSS class applied to the outermost div */
  className?: string;
}

export default function PanelBase({
  children,
  title,
  subtitle,
  layout = "grid",
  className = "",
}: PanelBaseProps) {
  return (
    <div className={`panel-container ${className}`}>
      {/* Optional header — only renders if title is provided */}
      {title && (
        <header className="mb-8 border-b border-[var(--color-border)] pb-4">
          <h1 className="text-3xl font-bold text-[var(--color-text-primary)] font-mono tracking-tight uppercase">
            {title}
          </h1>
          {subtitle && (
            <p className="text-[var(--color-text-secondary)] mt-2 font-mono text-sm">
              {subtitle}
            </p>
          )}
        </header>
      )}

      {/* Content area — layout mode controls grid vs stack */}
      <div className={layout === "grid" ? "panel-grid" : "flex flex-col gap-4"}>
        {children}
      </div>
    </div>
  );
}
