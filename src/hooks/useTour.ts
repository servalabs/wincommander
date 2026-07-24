// src/hooks/useTour.ts
//
// Runtime controller for the spotlight guide. Owns the current step, resolves
// the anchor element's on-screen rect (retrying while a freshly-navigated panel
// mounts), drives panel switches, and wires keyboard control. Pure step
// ordering lives in src/lib/tour.ts; this is the stateful half.

import { useCallback, useEffect, useRef, useState } from "react";
import type { TourStep } from "../content/guide/types";

interface UseTourArgs {
  steps: TourStep[];
  /** Called when the tour ends. `completed` is true only if the user walked
   *  to the final step's "Done"; false on skip/Escape/backdrop. */
  onClose: (completed: boolean) => void;
  /** False for the mandatory first-run tour, before it's ever been
   *  completed — Escape is disabled so it can't be interrupted (SpotlightTour
   *  separately hides its own X button). Defaults to true. */
  dismissable?: boolean;
}

interface UseTourResult {
  index: number;
  total: number;
  step: TourStep | null;
  /** Anchor rect in viewport coords, or null if the anchor wasn't found. */
  rect: DOMRect | null;
  /** `step.secondaryAnchor`'s rect, if the step declares one and it resolved. */
  secondaryRect: DOMRect | null;
  /** The left nav rail's rect, kept clear of the dim scrim throughout the
   *  tour — a multi-panel flow (Dashboard → Privacy → Cleanup) otherwise
   *  leaves no visible clue which section you're currently in. Null if the
   *  rail isn't in the DOM (shouldn't happen; it's global chrome). */
  sidebarRect: DOMRect | null;
  /** True once the current step's `requiresAction` event has fired (always
   *  true for steps without a `requiresAction`). */
  actionDone: boolean;
  /** True as soon as the anchor's `data-tour-state` reads "scanning" or
   *  "done" the moment this step is shown — i.e. the action was already
   *  underway (or finished) before the tour got here. Drives the
   *  `alreadyStarted*` copy swap in SpotlightTour. */
  actionPreStarted: boolean;
  next: () => void;
  back: () => void;
  skip: () => void;
  /** Force-advance past a `requiresAction` step without the action having
   *  happened. Distinct from `skip`, which exits the whole tour. */
  skipStep: () => void;
}

const MAX_FIND_FRAMES_SAME_PANEL = 90; // ~1.5s at 60fps — anchor already mounted
// Cross-panel steps (navigateTo set) can require a lazy chunk fetch for a
// panel never opened this session, plus its own async data loads (e.g.
// Privacy Settings detecting installed browsers) before the target section
// even exists in the DOM. 1.5s was giving up mid-mount, which is exactly
// what left the tour stuck on the full-screen dim fallback with no anchor
// and no scroll (2026-07-10 fix). ~8s covers a cold dev-mode panel switch.
const MAX_FIND_FRAMES_CROSS_PANEL = 480;

// Initial-scroll debounce: wait for the anchor's measured rect to stop
// moving for this many consecutive frames before trusting it enough to
// scroll — an anchor found the instant its panel mounts can still be
// mid-layout (e.g. Browser Hardening's card starts short and grows once
// async browser detection resolves), and scrolling against that transient
// short box is what left it off-center (2026-07-10 fix: "browser hardening
// ... before this was at the center ... now it is not").
const SCROLL_STABLE_FRAMES = 5;
// Upper bound on how long we'll wait for stability before scrolling anyway
// — a slow/never-settling layout shouldn't leave the tour scroll-frozen.
const SCROLL_SETTLE_CAP_FRAMES = 40;
// After the initial scroll, keep correcting (once) for this long if the
// target drifts out of view — covers content that finishes loading AFTER
// the debounced scroll already fired. Delayed by SCROLL_CORRECTION_DELAY_MS
// so it never fires mid-animation of the initial smooth scroll itself.
const SCROLL_CORRECTION_DELAY_MS = 500;
const SCROLL_CORRECTION_WINDOW_MS = 2500;
// Keeps a re-centered/corrected target clear of the app's own sticky top
// bar (health/theme/notification icons).
const SAFE_TOP_MARGIN = 88;
const SAFE_BOTTOM_MARGIN = 24;

const SIDEBAR_SELECTOR = '[data-tour="sidebar"]';

/** Resolves a possibly comma-separated, PRIORITY-ORDERED selector list —
 *  tries each in the order written and returns the first that matches,
 *  regardless of where either lands in the DOM. A plain `querySelector`
 *  call on the raw comma string does not have this property: the CSS spec
 *  has a selector list return the first match in DOCUMENT order, which
 *  silently prefers whichever anchor happens to sit earlier in the markup
 *  over the one actually listed first (2026-07-10 fix: apps-tour-updates'
 *  button-group fallback kept winning over the real update cards because
 *  it sits above them in the DOM, even though both were present). */
function resolveAnchor(anchorList: string): HTMLElement | null {
  for (const sel of anchorList.split(",")) {
    const el = document.querySelector<HTMLElement>(sel.trim());
    if (el) return el;
  }
  return null;
}

function unionRect(rects: DOMRect[], clampViewport = true): DOMRect | null {
  if (rects.length === 0) return null;
  let left = Infinity, top = Infinity, right = -Infinity, bottom = -Infinity;
  for (const r of rects) {
    if (r.width === 0 && r.height === 0) continue; // skip degenerate/empty nodes
    left = Math.min(left, r.left);
    top = Math.min(top, r.top);
    right = Math.max(right, r.right);
    bottom = Math.max(bottom, r.bottom);
  }
  if (left === Infinity) return null;
  // Unclamped mode (clampViewport === false) is used only for ANCHOR SELECTION
  // — "does this anchor wrap any real content, anywhere in the document" —
  // independent of whether it's currently scrolled into view. This is what
  // lets the Apps update-cards grid win over the on-screen "Update All" button
  // even while it's still below the fold, so the tour actually scrolls DOWN to
  // it instead of locking onto the fallback (a below-fold display:contents
  // anchor otherwise clamps to null and always loses the selection race).
  if (!clampViewport) {
    return new DOMRect(left, top, right - left, bottom - top);
  }
  // Clamp to the current viewport for the HIGHLIGHT rect. A display:contents
  // anchor can wrap DOZENS of real children (e.g. the Apps catalog's 30+
  // install cards) — most of them scrolled out of view at any given moment, so
  // their raw union can span several screens' worth of content with wildly
  // negative/oversized top/bottom values. An unbounded rect like that breaks
  // calloutStyle's space math (every side reads as having no room) and can push
  // the ring and callout far outside the visible viewport — the tour then LOOKS
  // fully stuck (2026-07-10 fix: "packages app... blank screen"). Clamping keeps
  // the highlight meaningful — "everything currently in view".
  const vw = window.innerWidth, vh = window.innerHeight;
  const clTop = Math.max(top, 0);
  const clLeft = Math.max(left, 0);
  const clBottom = Math.min(bottom, vh);
  const clRight = Math.min(right, vw);
  if (clBottom <= clTop || clRight <= clLeft) return null; // union is fully offscreen
  return new DOMRect(clLeft, clTop, clRight - clLeft, clBottom - clTop);
}

/** `getBoundingClientRect()` on a `display: contents` element always
 *  returns a degenerate 0×0 rect — such an element generates no box of its
 *  own by design. A couple of tour anchors deliberately use
 *  `display: contents` wrappers so a grouping `<div>` can carry a
 *  `data-tour` attribute without becoming an extra CSS Grid cell (Apps
 *  panel's update-cards / catalog groupings). Detect that case and fall
 *  back to the union of the element's own children's rects instead
 *  (2026-07-10 fix: the anchor WAS found, its rect was just always empty,
 *  which read as "update available grid... not showing"). */
function measureRect(el: HTMLElement, clampViewport = true): DOMRect | null {
  if (getComputedStyle(el).display !== "contents") {
    return el.getBoundingClientRect();
  }
  const childRects: DOMRect[] = [];
  for (const child of Array.from(el.children)) {
    const r = measureRect(child as HTMLElement, clampViewport);
    if (r) childRects.push(r);
  }
  return unionRect(childRects, clampViewport);
}

/** `Element.scrollIntoView()` needs a real layout box — calling it on a
 *  `display: contents` element (which generates none) is undefined/
 *  unreliable across browsers. Walk down to the first descendant that
 *  actually has a box and scroll that instead; the browser still finds the
 *  correct scrollable ancestor and containing layout on its own from there.
 *  Falls back to the original element if truly nothing qualifies (e.g. an
 *  empty display:contents wrapper). */
function firstRealElement(el: HTMLElement): HTMLElement | null {
  if (getComputedStyle(el).display !== "contents") return el;
  for (const child of Array.from(el.children)) {
    const found = firstRealElement(child as HTMLElement);
    if (found) return found;
  }
  return null;
}

/** Like resolveAnchor, but a selector only "counts" as a match if it also
 *  MEASURES to something real — not just that the element exists in the
 *  DOM. This is what makes a "prefer X, else Y" anchor list (e.g.
 *  apps-tour-updates: prefer the real update cards, else the Update All
 *  button) correctly skip an anchor that's permanently mounted but
 *  currently empty, instead of racing whether it happens to be conditionally
 *  rendered at all. Apps' update-cards wrapper is deliberately ALWAYS in the
 *  DOM now (its content, not its presence, depends on whether there are any
 *  updates) specifically so this check — "does it measure to anything" —
 *  is the one and only source of truth for which anchor wins, with no
 *  dependency on async data-load timing (2026-07-10 fix: "if apps need an
 *  update, show that section... but if no update, just show the Update All
 *  button" — kept losing this race under the old DOM-presence-only
 *  resolveAnchor). */
function resolveVisibleAnchor(anchorList: string): { el: HTMLElement; rect: DOMRect } | null {
  for (const sel of anchorList.split(",")) {
    const el = document.querySelector<HTMLElement>(sel.trim());
    if (!el) continue;
    // Selection: unclamped — an anchor wins as soon as it wraps real content,
    // even if it's still below the fold. Without this, a below-fold
    // display:contents anchor (Apps update grid) would clamp to null and the
    // resolver would fall through to the on-screen fallback (Update All
    // button) forever, so the tour never scrolls down to the updates section.
    const raw = measureRect(el, false);
    if (!raw) continue;
    // Ring: prefer the viewport-clamped rect; if the anchor is currently
    // offscreen (clamped null), hand back the raw rect so tracking latches on
    // and doScroll brings it into view — subsequent ticks then re-measure
    // clamped once it's visible.
    return { el, rect: measureRect(el, true) ?? raw };
  }
  return null;
}

export default function useTour({ steps, onClose, dismissable = true }: UseTourArgs): UseTourResult {
  const [index, setIndex] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const [secondaryRect, setSecondaryRect] = useState<DOMRect | null>(null);
  const [sidebarRect, setSidebarRect] = useState<DOMRect | null>(null);
  const [actionDone, setActionDone] = useState(true);
  const [actionPreStarted, setActionPreStarted] = useState(false);
  const triggeredRef = useRef(-1); // index already autoTrigger-clicked
  // Last panel we actually dispatched navigate-panel for — lets consecutive
  // steps that stay on the same panel (e.g. Browser Hardening → Privacy
  // Shield → RDP Idle, all navigateTo:"privacy") skip re-dispatching.
  const lastNavigatedPanelRef = useRef<string | undefined>(undefined);
  const total = steps.length;
  const step = steps[index] ?? null;

  const advance = useCallback(() => {
    setIndex((i) => {
      if (i >= total - 1) {
        onClose(true);
        return i;
      }
      return i + 1;
    });
  }, [total, onClose]);

  const next = useCallback(() => {
    if (step?.requiresAction && !actionDone) return; // gated — use skipStep
    advance();
  }, [step, actionDone, advance]);

  const skipStep = useCallback(() => advance(), [advance]);

  const back = useCallback(() => setIndex((i) => Math.max(0, i - 1)), []);
  const skip = useCallback(() => onClose(false), [onClose]);

  // A fresh step starts gated if it requires an action, open otherwise.
  // actionPreStarted is re-derived per step below, from the anchor's own
  // data-tour-state at the moment it's found.
  useEffect(() => {
    setActionDone(!step?.requiresAction);
    setActionPreStarted(false);
  }, [step]);

  // Listen for the current step's completion signal — the custom event PLUS
  // a DOM fallback (the anchor's own `disabled` attribute clearing). Belt
  // and suspenders: if the event dispatch is ever missed (e.g. a hook
  // instance mismatch, a dev-server hot-reload landing mid-scan), the
  // disabled-attribute observer still catches the real, user-visible signal
  // that the action finished — the button that said "Scanning…"/"disabled"
  // going back to normal (2026-07-10 fix: reports of Scan All never
  // unlocking Next even after the click actually completed).
  useEffect(() => {
    const anchor = step?.anchor;
    const eventName = step?.requiresAction?.eventName;
    if (!eventName || !anchor) return;
    const onDone = () => setActionDone(true);
    window.addEventListener(eventName, onDone);

    // Unlock Next/Back the instant the user CLICKS the action button, not
    // only when the operation finishes (the completion event / disabled-attr
    // clear below). Fix All / Scan All / Update run in the background and can
    // take a while; the user shouldn't be stuck on "Waiting for you…" the
    // whole time — one click and they can proceed (2026-07-20 fix). Document-
    // level delegation (capture) re-resolves the anchor per click so it works
    // even if the anchor element only appears after this effect ran; guarded
    // on a real <button> inside the anchor so a stray click on surrounding
    // card chrome (the Needs-Attention card is the Fix All anchor) doesn't
    // unlock.
    const onDocClick = (e: Event) => {
      const target = e.target as Element | null;
      if (!target?.closest("button")) return;
      const anchorEl = resolveAnchor(anchor);
      if (anchorEl && anchorEl.contains(target)) onDone();
    };
    document.addEventListener("click", onDocClick, true);

    let mo: MutationObserver | null = null;
    const el = resolveAnchor(anchor);
    if (el) {
      mo = new MutationObserver(() => {
        if (!(el as HTMLButtonElement).disabled) onDone();
      });
      mo.observe(el, { attributes: true, attributeFilter: ["disabled"] });
    }

    return () => {
      window.removeEventListener(eventName, onDone);
      document.removeEventListener("click", onDocClick, true);
      mo?.disconnect();
    };
  }, [step]);

  // Resolve the anchor rect for the active step, then keep it continuously
  // in sync for as long as the step is shown. Switches panels first, then
  // polls for the element (it may not exist until the new panel renders).
  useEffect(() => {
    if (!step) return;
    let cancelled = false;
    let raf = 0;
    let notFoundFrames = 0;

    // Only a REAL panel switch — not just any step that happens to declare
    // navigateTo, but one whose target differs from wherever the previous
    // step already left us — dispatches navigate-panel. Two consecutive
    // steps in the same panel (e.g. Browser Hardening → Privacy Shield →
    // RDP Idle, all navigateTo:"privacy") were each independently
    // re-dispatching navigate-panel; App.tsx's handlePanelChange treats
    // "navigate to the panel you're already on" as a request to jump the
    // scroll container back to its top (its normal behavior for
    // re-clicking an already-active sidebar item) — so every such step was
    // visibly scrolling to the top of the panel and then back down to the
    // real anchor (2026-07-10 fix: "after the privacy shield showcase...
    // goes to the top of privacy settings and then comes back to RDP
    // idle"). maxFrames deliberately stays keyed off step.navigateTo alone
    // (not "is this a real switch") — a same-panel step can still need the
    // long budget for its own async content (Process Review's list can take
    // longer to render than the short same-panel window, especially right
    // after Scan All's heavy work), and shortening it for same-panel steps
    // was a regression that intermittently made Process Review never appear
    // (2026-07-10 fix: "sometimes process review is not visible").
    const isRealPanelChange = Boolean(step.navigateTo) && step.navigateTo !== lastNavigatedPanelRef.current;
    const maxFrames = step.navigateTo ? MAX_FIND_FRAMES_CROSS_PANEL : MAX_FIND_FRAMES_SAME_PANEL;

    if (isRealPanelChange) {
      window.dispatchEvent(new CustomEvent("navigate-panel", { detail: step.navigateTo }));
    }
    if (step.navigateTo) lastNavigatedPanelRef.current = step.navigateTo;

    // Per-anchor tracking state, reset whenever the resolved element itself
    // changes (see "switched" below) — not just once at the top of the
    // effect.
    let currentEl: HTMLElement | null = null;
    let last: DOMRect | null = null;
    let stableFrames = 0;
    let settleFrames = 0;
    let scrolled = false;
    let scrolledAtMs = 0;
    let corrected = false;
    let stateChecked = false;

    const doScroll = () => {
      // scrollIntoView needs a real layout box — the anchor itself can be a
      // display:contents wrapper (Apps panel's catalog/update-card
      // groupings), which generates none (2026-07-10 fix: "packages app...
      // blank screen" — this either no-ops unpredictably or, in the worst
      // case some engines, throws, which as an uncaught error inside this
      // rAF chain would silently and permanently freeze tracking right
      // here).
      if (!currentEl) return;
      // Scroll the anchor toward the side OPPOSITE the callout so the
      // callout's side has room — a tall "top" callout above a big card
      // otherwise had nowhere to go and either clipped at the viewport top
      // or overlapped the card (2026-07-20 fix: Browser Hardening). "top"
      // callout → put the anchor LOW (block:"end"); "bottom" callout → put
      // it HIGH (block:"start"); otherwise centre it as before.
      const block: ScrollLogicalPosition =
        step.placement === "top" ? "end" : step.placement === "bottom" ? "start" : "center";
      (firstRealElement(currentEl) ?? currentEl).scrollIntoView({ block, inline: "nearest", behavior: "smooth" });
    };

    const tick = () => {
      if (cancelled) return;
      if (step.openEvent && !currentEl) {
        window.dispatchEvent(new Event(step.openEvent));
      }
      // Never let an unexpected throw here kill the chain — this runs
      // inside requestAnimationFrame, which React's error boundary cannot
      // catch; an uncaught error would silently and permanently stop
      // tracking, leaving the tour dimmed with a stale/absent highlight and
      // no way to recover except Escape (2026-07-10 fix: "packages app not
      // working properly... blank screen").
      try {
        // Re-resolve EVERY tick, not just once — a step.anchor with a
        // "prefer this, else fall back to that" comma list (e.g.
        // apps-tour-updates) can find the fallback FIRST simply because the
        // preferred anchor doesn't measure to anything YET (its content is
        // still loading), then permanently lock onto it even after the
        // preferred one gets real content moments later, since nothing ever
        // went back to check (2026-07-10 fix: "update app... not taking to
        // the update available section"). resolveVisibleAnchor requires a
        // REAL measured rect, not just DOM presence, to count as a match —
        // see its doc comment for why that's now the actual source of
        // truth for "is there anything to show here". Cheap: a handful of
        // querySelector + rect calls per frame.
        let resolved: { el: HTMLElement; rect: DOMRect } | null = null;
        try {
          resolved = resolveVisibleAnchor(step.anchor);
        } catch (err) {
          console.error("[useTour] resolveVisibleAnchor failed", err);
        }
        const resolvedEl = resolved?.el ?? null;

        if (resolvedEl !== currentEl) {
          // Switching to a newly-available (possibly higher-priority)
          // anchor, or finding one for the first time, or losing one
          // (resolvedEl === null) — reset all per-anchor state and start
          // fresh against whatever we have now.
          currentEl = resolvedEl;
          last = null;
          stableFrames = 0;
          settleFrames = 0;
          scrolled = false;
          scrolledAtMs = 0;
          corrected = false;
          stateChecked = false;

          if (currentEl && resolved) {
            notFoundFrames = 0;
            if (step.requiresAction && !stateChecked) {
              stateChecked = true;
              const dataState = currentEl.getAttribute("data-tour-state");
              if (dataState === "done") {
                setActionDone(true);
                setActionPreStarted(true);
              } else if (dataState === "scanning") {
                setActionPreStarted(true);
              }
            }
            last = resolved.rect;
            setRect(last);
            if (step.autoTrigger && triggeredRef.current !== index) {
              triggeredRef.current = index;
              currentEl.click();
            }
          } else {
            setRect(null);
          }
        }

        if (currentEl && resolved) {
          // Continuously re-measure for as long as this step is active — a
          // ResizeObserver on the anchor alone only catches the anchor's OWN
          // size changing, not a sibling/ancestor reflow that shifts its
          // position without resizing it (e.g. Needs Attention's list
          // shrinking as Fix All's items complete moves the button up, but
          // the button itself never resizes) (2026-07-10 fix: "if Fix All
          // is moved a little up or down, it should be tracked"). Comparing
          // against the last rect keeps it from calling setRect (and
          // re-rendering) when nothing's actually changed.
          const next = resolved.rect;
          const changed = !last || next.top !== last.top || next.left !== last.left || next.width !== last.width || next.height !== last.height;
          if (changed) {
            last = next;
            setRect(next);
            stableFrames = 0;
          } else {
            stableFrames += 1;
          }

          if (!scrolled) {
            // Debounced initial scroll: wait for the measured rect to hold
            // still for a few frames before trusting it, capped so a
            // never-settling layout doesn't freeze the scroll forever
            // (2026-07-10 fix — see SCROLL_STABLE_FRAMES above).
            settleFrames += 1;
            if (last && (stableFrames >= SCROLL_STABLE_FRAMES || settleFrames >= SCROLL_SETTLE_CAP_FRAMES)) {
              // "center": puts the target in the viewport's vertical middle,
              // clear of the app's sticky top bar that a top-alignment
              // scroll used to land targets directly under (2026-07-10 fix:
              // "Scan all and Update all are getting clipped... should be
              // at the center of the viewport"). The callout's own
              // collision-aware flip logic (SpotlightTour) finds room on
              // whichever side actually has it, so this doesn't
              // reintroduce the earlier "no room below" clipping either.
              doScroll();
              scrolled = true;
              scrolledAtMs = performance.now();
            }
          } else if (!corrected && scrolledAtMs && last) {
            // One-time correction window: if content that was still
            // loading when the debounced scroll fired ends up shifting the
            // target out of a safely-visible band, nudge it back once.
            // Delayed so it never fires mid-animation of the initial
            // smooth scroll, and bounded so it can never fight the user's
            // own later scrolling (2026-07-10 fix — same report as above).
            const elapsed = performance.now() - scrolledAtMs;
            if (elapsed > SCROLL_CORRECTION_DELAY_MS && elapsed < SCROLL_CORRECTION_WINDOW_MS) {
              const centerY = last.top + last.height / 2;
              if (centerY < SAFE_TOP_MARGIN || centerY > window.innerHeight - SAFE_BOTTOM_MARGIN) {
                doScroll();
                corrected = true;
              }
            } else if (elapsed >= SCROLL_CORRECTION_WINDOW_MS) {
              corrected = true; // window closed — stop checking
            }
          }
        } else {
          // Never found anything yet this step — give up after maxFrames,
          // same as before, so the callout/hero falls back to centered
          // rather than polling forever for an anchor that will never
          // exist.
          notFoundFrames += 1;
          if (notFoundFrames >= maxFrames) {
            setRect(null);
            cancelled = true; // stop polling — nothing left to find
            return;
          }
        }
      } catch (err) {
        console.error("[useTour] tick failed", err);
      } finally {
        if (!cancelled) raf = requestAnimationFrame(tick);
      }
    };

    setRect(null);
    raf = requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
    };
  }, [step, index]);

  // Secondary highlight rect (e.g. Secure Storage: the whole card via
  // `anchor`, PLUS a bolder inner ring around the live MB counter via
  // `secondaryAnchor`) — purely visual, so it just tracks continuously with
  // no scroll of its own; the primary anchor's scroll already brings both
  // into view since the secondary anchor is always nested inside it.
  useEffect(() => {
    setSecondaryRect(null);
    if (!step?.secondaryAnchor) return;
    let cancelled = false;
    let raf = 0;
    const selector = step.secondaryAnchor;
    const poll = () => {
      if (cancelled) return;
      try {
        const el = resolveAnchor(selector);
        setSecondaryRect(el ? measureRect(el) : null);
      } catch (err) {
        console.error("[useTour] secondary anchor poll failed", err);
      } finally {
        raf = requestAnimationFrame(poll);
      }
    };
    raf = requestAnimationFrame(poll);
    return () => {
      cancelled = true;
      cancelAnimationFrame(raf);
    };
  }, [step]);

  // Track the sidebar's own rect for the whole tour (not per-step — it's
  // global chrome, not something that comes and goes with navigateTo). A
  // ResizeObserver catches its collapse/expand toggle; window resize covers
  // the rest.
  useEffect(() => {
    const el = document.querySelector<HTMLElement>(SIDEBAR_SELECTOR);
    if (!el) return;
    const remeasure = () => setSidebarRect(el.getBoundingClientRect());
    remeasure();
    const ro = new ResizeObserver(remeasure);
    ro.observe(el);
    window.addEventListener("resize", remeasure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", remeasure);
    };
  }, []);

  // Keyboard control.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { if (dismissable) skip(); }
      else if (e.key === "ArrowRight" || e.key === "Enter") next();
      else if (e.key === "ArrowLeft") back();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [next, back, skip, dismissable]);

  return { index, total, step, rect, secondaryRect, sidebarRect, actionDone, actionPreStarted, next, back, skip, skipStep };
}
