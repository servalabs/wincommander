import { Toaster } from "./ui/sonner";
import { useAppState } from "../context/AppContext";

/*
 * AppToaster — mounts the Sonner toaster. Alerts are persisted in the
 * title-bar bell; optional popups are pinned near that bell instead of
 * the old bottom-left corner so the visual cue points back to the
 * notification center.
 *
 * Motion: Sonner is CSS-class driven via [data-sonner-toast] and the
 * data-[x-position] / data-[y-position] attributes it sets on each toast.
 * We inject a tiny CSS block (scoped to [data-sonner-toaster]) that:
 *   • On enter  — slides in from the right edge + fades in.
 *   • Stacking  — Sonner shifts older toasts via transform:translateY
 *                 internally; we let it own that so we only animate
 *                 opacity + translateX (never left/width/height → no reflow).
 *   • On exit   — slides back toward the edge + fades out.
 * All durations/curves come from the CSS token SSOT (v2-theme.css):
 *   var(--dur-normal), var(--dur-fast), var(--ease).
 * The global html.wc-no-motion block in v2-theme.css collapses these
 * to 0.01ms automatically — no JS branch needed here.
 */
const TOAST_STYLES = `
[data-sonner-toaster] [data-sonner-toast] {
  /* Compositor hint — only transform+opacity animate, no reflow. */
  will-change: transform, opacity;
}

/* ── Enter: slide in from the right edge + fade ── */
[data-sonner-toaster][data-x-position="right"] [data-sonner-toast][data-mounted="true"] {
  animation: wc-toast-enter var(--dur-normal, 200ms) var(--ease, cubic-bezier(0.22,0.61,0.36,1)) both;
}

/* ── Exit: slide back toward right + fade ── */
[data-sonner-toaster][data-x-position="right"] [data-sonner-toast][data-removed="true"] {
  animation: wc-toast-exit var(--dur-fast, 150ms) var(--ease, cubic-bezier(0.22,0.61,0.36,1)) both;
}

@keyframes wc-toast-enter {
  from { transform: translateX(calc(100% + 24px)); opacity: 0; }
  to   { transform: translateX(0);                 opacity: 1; }
}

@keyframes wc-toast-exit {
  from { transform: translateX(0);                 opacity: 1; }
  to   { transform: translateX(calc(100% + 24px)); opacity: 0; }
}
`;

export default function AppToaster() {
  const { appSettings } = useAppState();
  const n = appSettings?.app?.notifications;
  const duration = typeof n?.timeout === "number" && n.timeout > 0 ? n.timeout : undefined;
  return (
    <>
      {/* Scoped CSS for toast slide+fade — references token vars only, never bare ms/curves. */}
      <style>{TOAST_STYLES}</style>
      {/* expand: show every stacked toast's content. Sonner's default collapses
          extra toasts into a stack where only the front one shows text and the
          ones behind render as blank cards that expand ONLY on hover — which
          reads as a "blank notification" bug. */}
      <Toaster position="top-right" duration={duration} expand />
    </>
  );
}
