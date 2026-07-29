// src/components/guide/SpotlightTour.tsx
//
// The spotlight overlay: a dimmed scrim with a gliding cut-out highlight over
// the active anchor, plus a placement-aware callout card. Pure renderer driven
// by useTour. Dynamic positioning (top/left/size from measured rects) is inline
// by necessity; all cosmetics live in SpotlightTour.css and V2 tokens.

import type { CSSProperties } from "react";
import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { Icon } from "../ui/icon";
import { SPRING, DURATION_S } from "../shared/motion";
import useTour from "../../hooks/useTour";
import type { TourStep, TourPlacement } from "../../content/guide/types";
import "./SpotlightTour.css";

const PAD = 8; // breathing room around the highlighted element
// Wide enough for embedded media/components (screenshot, live animation)
// without cramping — was 340, too tight once callouts started carrying
// media (2026-07-10 fix).
const CALLOUT_W = 380;
// Callout height is dynamic (media steps run much taller than text-only
// ones), so this is a conservative upper estimate for vertical clamping —
// paired with a CSS max-height/overflow-y safety net for anything taller.
const CALLOUT_MAX_H_ESTIMATE = 420;

interface SpotlightTourProps {
  steps: TourStep[];
  onClose: (completed: boolean) => void;
  /** False for the mandatory first-run tour, before it's ever been
   *  completed — hides the X button and disables Escape (via useTour) so it
   *  can't be dismissed early. Defaults to true. */
  dismissable?: boolean;
}

function calloutStyle(rect: DOMRect | null, placement: TourPlacement, calloutH = CALLOUT_MAX_H_ESTIMATE): CSSProperties {
  if (!rect) {
    return { top: "50%", left: "50%", transform: "translate(-50%, -50%)" };
  }
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const GAP = PAD + 12; // clearance kept between the target rect and the callout
  // Real measured callout height (falls back to the estimate on first paint).
  // Used for every vertical clamp so a media-heavy callout can't run off the
  // top or bottom edge (2026-07-20 fix: "modal getting clipped ... a little
  // bit lower"). h is bounded so a callout taller than the viewport still
  // pins to the top rather than producing a negative clamp range.
  const h = Math.min(calloutH, vh - 24);

  const spaceRight = vw - rect.right - GAP;
  const spaceLeft = rect.left - GAP;
  const spaceBottom = vh - rect.bottom - GAP;
  const spaceTop = rect.top - GAP;

  // Resolve to a side that genuinely has room, preferring the requested one
  // and flipping to whichever does when it doesn't fit — NOT just clamping
  // the preferred side's position back onto the target, which was the real
  // bug (2026-07-10 fix): a callout with no room on its "right" would get
  // slid left by the old clamp until it landed right back on top of the
  // target it was supposed to be pointing at.
  let resolved: TourPlacement =
    placement === "auto" ? (spaceBottom >= spaceTop ? "bottom" : "top") : placement;

  const lateralFits =
    resolved === "right" ? spaceRight >= CALLOUT_W : resolved === "left" ? spaceLeft >= CALLOUT_W : true;
  if (!lateralFits) {
    resolved = spaceRight >= CALLOUT_W ? "right" : spaceLeft >= CALLOUT_W ? "left" : (spaceBottom >= spaceTop ? "bottom" : "top");
  }
  const verticalFits =
    resolved === "bottom" ? spaceBottom >= h : resolved === "top" ? spaceTop >= h : true;
  if (!verticalFits) {
    resolved = spaceBottom >= h ? "bottom"
      : spaceTop >= h ? "top"
      : spaceRight >= CALLOUT_W ? "right"
      : spaceLeft >= CALLOUT_W ? "left"
      // Nothing fits cleanly (a target close to viewport-filling in both
      // directions, e.g. a very tall wide card) — pick whichever side has
      // the MOST room rather than blindly keeping the current pick. The old
      // behavior kept falling through to "bottom" here even when e.g.
      // "right" had far more space, then clamped the callout's top back
      // into the target's own vertical range — i.e. directly on top of it
      // (2026-07-10 fix: "modal is overlapping"). This is still a
      // last-resort case (the CSS max-height/overflow-y safety net on
      // .spotlight-callout is what actually keeps it on-screen), but at
      // least points toward open space instead of away from it.
      //
      // left/right are only real candidates here if they actually fit the
      // callout's WIDTH — for a wide anchor (e.g. a full-width card like
      // Browser Hardening) spaceLeft/spaceRight can be small but still
      // larger than the insufficient spaceTop/spaceBottom, which picked
      // "left"/"right" anyway even though the callout structurally can't
      // fit there, landing it jammed against the anchor's edge (2026-07-20
      // fix: "modal is coming at the left side ... should be at the top").
      // top/bottom always pass the width check trivially (see lateralFits
      // above), so they stay in the running regardless.
      : ([
          { side: "bottom" as const, space: spaceBottom },
          { side: "top" as const, space: spaceTop },
          ...(spaceRight >= CALLOUT_W ? [{ side: "right" as const, space: spaceRight }] : []),
          ...(spaceLeft >= CALLOUT_W ? [{ side: "left" as const, space: spaceLeft }] : []),
        ].sort((a, b) => b.space - a.space)[0].side);
  }

  const clampLeftEdge = (x: number) => Math.min(Math.max(12, x), vw - CALLOUT_W - 12);
  // For no-transform placements the returned `top` IS the callout's top edge,
  // so clamp it to [12, vh - h - 12] using the real height.
  const clampTop = (y: number) => Math.min(Math.max(12, y), vh - h - 12);

  switch (resolved) {
    case "top":
      // translateY(-100%) makes the returned `top` the callout's BOTTOM edge,
      // so its visual top = top - h. Clamp so the visual top stays >= 12
      // (never clipped at the viewport top — the reported bug) and the bottom
      // stays <= vh - 12; when there isn't room above, this drops the callout
      // lower so it overlaps the anchor's top rather than running off-screen.
      return { top: Math.min(Math.max(rect.top - PAD - 10, h + 12), vh - 12), left: clampLeftEdge(rect.left), transform: "translateY(-100%)" };
    case "right":
      return { top: clampTop(rect.top), left: clampLeftEdge(rect.right + PAD + 12) };
    case "left":
      // translateX(-100%) makes `left` the callout's RIGHT edge, so the
      // floor here is "leave room for the box's own width", not the plain
      // 12px used everywhere else (2026-07-10 fix — this was wrong before).
      return { top: clampTop(rect.top), left: Math.max(12 + CALLOUT_W, rect.left - PAD - 12), transform: "translateX(-100%)" };
    case "bottom":
    default:
      return { top: clampTop(rect.bottom + PAD + 10), left: clampLeftEdge(rect.left) };
  }
}

/** Where a line from (x1,y1) toward a rect's center first crosses the
 *  rect's own border — i.e. the near edge, not the center. Standard
 *  parametric line/box clipping (slab method): walk the line as
 *  P(t) = (x1,y1) + t·(dx,dy) for t in [0,1]; the entry point is at the
 *  larger of the two axes' entry parameters. */
function lineToRectEdge(x1: number, y1: number, target: DOMRect): { x: number; y: number } {
  const cx = target.left + target.width / 2;
  const cy = target.top + target.height / 2;
  const dx = cx - x1;
  const dy = cy - y1;
  if (dx === 0 && dy === 0) return { x: cx, y: cy };

  let tEnterX = -Infinity;
  if (dx !== 0) {
    const t1 = (target.left - x1) / dx;
    const t2 = (target.left + target.width - x1) / dx;
    tEnterX = Math.min(t1, t2);
  }
  let tEnterY = -Infinity;
  if (dy !== 0) {
    const t1 = (target.top - y1) / dy;
    const t2 = (target.top + target.height - y1) / dy;
    tEnterY = Math.min(t1, t2);
  }
  const tEnter = Math.min(Math.max(tEnterX, tEnterY, 0), 1);
  return { x: x1 + dx * tEnter, y: y1 + dy * tEnter };
}

/** Dashed line from the ring's center to the hero modal's near edge (not
 *  its center — a line running INTO the card looked wrong and got lost
 *  under it) (2026-07-10 fix). Purely decorative — skipped when there's no
 *  rect or the modal hasn't been measured yet. */
function Connector({ rect, modalRect }: { rect: DOMRect; modalRect: DOMRect | null }) {
  const x1 = rect.left + rect.width / 2;
  const y1 = rect.top + rect.height / 2;
  const end = modalRect
    ? lineToRectEdge(x1, y1, modalRect)
    : { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  return (
    <svg className="spotlight-connector-svg" width="100%" height="100%" aria-hidden="true">
      <line className="spotlight-connector-line" x1={x1} y1={y1} x2={end.x} y2={end.y} />
    </svg>
  );
}

export interface ScrimHole { left: number; top: number; right: number; bottom: number; }
export interface ScrimPanel { x: number; y: number; w: number; h: number; }

export function padToHole(rect: DOMRect | null, pad: number): ScrimHole | null {
  if (!rect) return null;
  return { left: rect.left - pad, top: rect.top - pad, right: rect.left + rect.width + pad, bottom: rect.top + rect.height + pad };
}

const clampTo = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v));

/** Decomposes the viewport minus a set of hole rectangles into the minimal
 *  set of axis-aligned panels needed to dim/blur everything EXCEPT the
 *  holes. Each panel is geometrically outside every hole (a sweep-line grid
 *  over the holes' edges, keeping only cells whose center isn't inside any
 *  hole, merged horizontally per row) — so backdrop-filter on a panel can
 *  never reach a hole, unlike the previous single-rect-with-an-SVG-mask
 *  approach, where backdrop-filter blurred the whole element's backdrop
 *  within its bounding box regardless of the mask (only the fill color
 *  respected the mask — the hole showed no dark tint but was still blurred,
 *  2026-07-20 bug). */
export function computeScrimPanels(vw: number, vh: number, holes: ScrimHole[]): ScrimPanel[] {
  const xs = Array.from(new Set([0, vw, ...holes.flatMap((h) => [clampTo(h.left, 0, vw), clampTo(h.right, 0, vw)])])).sort((a, b) => a - b);
  const ys = Array.from(new Set([0, vh, ...holes.flatMap((h) => [clampTo(h.top, 0, vh), clampTo(h.bottom, 0, vh)])])).sort((a, b) => a - b);

  const panels: ScrimPanel[] = [];
  for (let yi = 0; yi < ys.length - 1; yi++) {
    const y0 = ys[yi];
    const y1 = ys[yi + 1];
    const cy = (y0 + y1) / 2;
    let runStartX: number | null = null;
    for (let xi = 0; xi < xs.length - 1; xi++) {
      const x0 = xs[xi];
      const x1 = xs[xi + 1];
      const cx = (x0 + x1) / 2;
      const inHole = holes.some((h) => cx > h.left && cx < h.right && cy > h.top && cy < h.bottom);
      if (inHole) {
        if (runStartX !== null) {
          panels.push({ x: runStartX, y: y0, w: x0 - runStartX, h: y1 - y0 });
          runStartX = null;
        }
      } else if (runStartX === null) {
        runStartX = x0;
      }
    }
    if (runStartX !== null) {
      panels.push({ x: runStartX, y: y0, w: xs[xs.length - 1] - runStartX, h: y1 - y0 });
    }
  }
  return panels;
}

/** The dim+blur layer, built from panels that never overlap the active
 *  anchor, an optional secondary anchor, or the left nav rail — so those
 *  stay perfectly crisp no matter what filter the panels carry. Corners at
 *  a hole's edge are square (panels are plain rects), not rounded to
 *  holeRadius like the old SVG mask — .spotlight-ring's own border/box-
 *  shadow sits on top of that seam and already reads as the rounded edge. */
function Scrim({
  holeRect,
  secondaryHoleRect,
  sidebarRect,
}: {
  holeRect: DOMRect | null;
  /** A second, always-rectangular cutout nested inside `holeRect` — e.g.
   *  Secure Storage's live MB counter, double-highlighted inside the whole
   *  Disk Clean-Up card. Purely additive to holeRect's own cutout. */
  secondaryHoleRect: DOMRect | null;
  sidebarRect: DOMRect | null;
}) {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const holes = [padToHole(holeRect, PAD), padToHole(secondaryHoleRect, PAD), padToHole(sidebarRect, 0)]
    .filter((h): h is ScrimHole => h !== null);
  const panels = computeScrimPanels(vw, vh, holes);
  return (
    <>
      {panels.map((p, i) => (
        <div key={i} className="spotlight-scrim-panel" style={{ left: p.x, top: p.y, width: p.w, height: p.h }} />
      ))}
    </>
  );
}

export default function SpotlightTour({ steps, onClose, dismissable = true }: SpotlightTourProps) {
  const { index, total, step, rect, secondaryRect, sidebarRect, actionDone, actionPreStarted, next, back, skip, skipStep } = useTour({ steps, onClose, dismissable });

  // Measure the hero modal itself so the connector line can stop at its
  // border instead of its center (2026-07-10 fix). Runs whenever the modal
  // is present; a ResizeObserver catches it changing size (media/component
  // content settling in) after the initial measurement.
  const modalRef = useRef<HTMLDivElement | null>(null);
  const [modalRect, setModalRect] = useState<DOMRect | null>(null);
  useEffect(() => {
    const el = modalRef.current;
    if (!el) {
      setModalRect(null);
      return;
    }
    const remeasure = () => setModalRect(el.getBoundingClientRect());
    remeasure();
    const ro = new ResizeObserver(remeasure);
    ro.observe(el);
    window.addEventListener("resize", remeasure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", remeasure);
    };
  }, [step]);

  // Measure the (non-hero) callout's own height so calloutStyle can clamp it
  // fully on-screen — a media-heavy callout is much taller than a text-only
  // one, and the position must account for the real height or it clips at the
  // top/bottom edge (2026-07-20 fix). Height is content-driven (stable per
  // step regardless of where it's positioned), so this converges in one extra
  // render with no feedback loop. Falls back to the estimate until measured.
  const calloutRef = useRef<HTMLDivElement | null>(null);
  const [calloutH, setCalloutH] = useState(CALLOUT_MAX_H_ESTIMATE);
  useEffect(() => {
    const el = calloutRef.current;
    if (!el) return;
    const remeasure = () => setCalloutH(el.offsetHeight);
    remeasure();
    const ro = new ResizeObserver(remeasure);
    ro.observe(el);
    window.addEventListener("resize", remeasure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", remeasure);
    };
  }, [step]);

  if (!step) return null;

  const isLast = index === total - 1;
  const isHero = step.variant === "hero";
  const gated = Boolean(step.requiresAction) && !actionDone;
  // A do-it-yourself step stays fully un-dimmed for its whole duration —
  // before the click (so the target and its surrounding context are clearly
  // visible, not read through a dark scrim) AND after (so the user can see
  // what actually changed before hitting Next) — not just while gated
  // (2026-07-10 fix: "unblur ... until they stay at that section").
  // Exception: a step can opt back INTO the dim via requiresAction.keepDim
  // when the whole point is to spotlight one region and blur the rest until
  // the user acts (Fix All — 2026-07-20), rather than showing the entire
  // panel clearly.
  const suppressDim = Boolean(step.requiresAction) && !step.requiresAction?.keepDim;
  // The action was already underway (or finished) before the tour reached
  // this step — swap in copy that acknowledges that instead of instructing
  // a click that already happened (2026-07-10 fix: "if it is already
  // scanning and 'Take the tour' is clicked, it should check").
  const showAlreadyStarted = actionPreStarted && Boolean(step.requiresAction?.alreadyStartedTitle);
  const title = showAlreadyStarted ? step.requiresAction!.alreadyStartedTitle! : step.title;
  const summary = showAlreadyStarted ? (step.requiresAction!.alreadyStartedSummary ?? step.summary) : step.summary;

  const body = (
    <>
      <div className="spotlight-callout-head">
        <span className="spotlight-counter">{index + 1} / {total}</span>
        {dismissable && (
          <button className="spotlight-x" onClick={skip} aria-label="Skip tour">
            <Icon icon="cross" size={13} />
          </button>
        )}
      </div>

      {step.media?.type === "video" && (
        <video
          className="spotlight-media"
          src={step.media.src}
          autoPlay
          loop
          muted
          playsInline
          disablePictureInPicture
        />
      )}
      {step.media?.type === "image" && (
        <img className="spotlight-media" src={step.media.src} alt={step.media.alt ?? ""} />
      )}
      {step.component && (
        <div className="spotlight-component-box">
          <step.component />
        </div>
      )}

      <h3 className="spotlight-title">{title}</h3>
      <p className="spotlight-summary">{summary}</p>

      {step.requiresAction?.warning && !(actionPreStarted && step.requiresAction.hideWarningWhenPreStarted) && (
        <div className="spotlight-warning">
          <Icon icon="warning-sign" size={14} />
          <span>{step.requiresAction.warning}</span>
        </div>
      )}

      {/* A cross-panel deep link (e.g. installing a missing app) rather than
          a real element already in the DOM to click through to — dispatches
          the named event and lets whatever already listens for it (see
          `action.eventName`'s call sites) do the rest. */}
      {step.action && (
        <button
          className="spotlight-btn primary"
          onClick={() => window.dispatchEvent(new CustomEvent(step.action!.eventName, { detail: step.action!.eventDetail }))}
        >
          <Icon icon="download" size={13} />
          {step.action.label}
        </button>
      )}

      <div className="spotlight-dots">
        {steps.map((s, i) => (
          <span key={s.topicId} className={`spotlight-dot ${i === index ? "active" : ""}`} />
        ))}
      </div>

      <div className="spotlight-actions">
        <div className="spotlight-nav">
          {gated && (
            <button className="spotlight-skip-step" onClick={skipStep}>
              Skip this step
            </button>
          )}
          {!gated && index > 0 && (
            <button className="spotlight-btn" onClick={back}>
              <Icon icon="chevron-left" size={13} /> Back
            </button>
          )}
          <button className="spotlight-btn primary" onClick={next} disabled={gated}>
            {gated ? "Waiting for you…" : isLast ? "Done" : "Next"}
            {!gated && !isLast && <Icon icon="chevron-right" size={13} />}
          </button>
        </div>
      </div>
    </>
  );

  return (
    // pointer-events:none on every step (2026-07-20 fix — was only for a
    // requiresAction step's whole duration) — the highlighted element should
    // never be "frozen" under this overlay: the user can click into it and
    // scroll it exactly like the rest of the app, on any step, not just the
    // do-it-yourself ones. The callout card re-enables pointer-events on
    // itself so Next/Back/Skip still work.
    <div
      className="spotlight-root"
      role="dialog"
      aria-modal="true"
      aria-label="Guided tour"
    >
      {/* Suppressed (not just click-through) for a requiresAction step's
          whole duration — the user needs full visual clarity of the target
          section to act and to see what changed, not a still-dimmed view
          they merely happen to be able to click through (2026-07-10 fix).
          Sidebar hole is skipped for every Dashboard step — it's where the
          tour starts, so "you are on Dashboard" isn't new information the
          way it is once the flow has actually navigated elsewhere
          (2026-07-10 fix — was previously keyed off isHero, which left it
          showing for Dashboard's own non-hero steps too). */}
      {!suppressDim && (
        <Scrim
          holeRect={rect}
          secondaryHoleRect={secondaryRect}
          sidebarRect={step.navigateTo === "dashboard" ? null : sidebarRect}
        />
      )}

      {/* Stays visible even when the dim/connector are suppressed during a
          requiresAction step — it's the one remaining guide to where the
          target is, once the darkened background is gone. */}
      {rect && (
        <motion.div
          className={`spotlight-ring ${isHero ? "spotlight-ring--hero" : ""}`}
          initial={false}
          animate={{ x: rect.left - PAD, y: rect.top - PAD, width: rect.width + PAD * 2, height: rect.height + PAD * 2 }}
          transition={SPRING.snappy}
        />
      )}

      {/* Bolder inner ring for a step's secondaryAnchor — e.g. the live MB
          counter double-highlighted inside the whole Disk Clean-Up card
          (2026-07-10 fix: "the space clearing should be more highlighted
          ... with another rectangle highlighting box"). */}
      {secondaryRect && (
        <motion.div
          className="spotlight-ring spotlight-ring--secondary"
          initial={false}
          animate={{ x: secondaryRect.left - PAD, y: secondaryRect.top - PAD, width: secondaryRect.width + PAD * 2, height: secondaryRect.height + PAD * 2 }}
          transition={SPRING.snappy}
        />
      )}

      {rect && isHero && !suppressDim && <Connector rect={rect} modalRect={modalRect} />}

      <motion.div
        key={index}
        ref={isHero ? modalRef : calloutRef}
        className={isHero ? "spotlight-callout spotlight-hero-modal" : "spotlight-callout"}
        // Opacity-only entrance: animating x/y/scale would set `transform` and
        // clobber the inline translate(-100%) used for top/left placements.
        style={isHero ? undefined : { width: CALLOUT_W, ...calloutStyle(rect, step.placement, calloutH) }}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: DURATION_S.fast }}
      >
        {body}
      </motion.div>
    </div>
  );
}

export type { TourStep };
