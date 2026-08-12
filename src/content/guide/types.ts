// src/content/guide/types.ts
//
// Types for the in-app guide content SSOT. The spotlight tour (a topic's
// `tour` block) is the only surface that reads a GuideTopic today — the
// help center (browsable articles) and the per-panel contextual "?" were
// both removed. `panelId`/`keywords`/`related`/`body` are legacy fields from
// those two surfaces, kept only because they're still populated on the
// handful of topics that also carry a `tour` block. See
// docs/superpowers/specs/2026-06-28-in-app-guide-onboarding-design.md.

import type { ComponentType } from "react";
import type { PanelId } from "../../types/panels";
import type { Density } from "../../types/persona";

export type TourPlacement = "top" | "bottom" | "left" | "right" | "auto";

/** Membership of a topic in a named tour, with its position + audience. */
export interface TourMembership {
  /** Tour id, e.g. "welcome". */
  id: string;
  /** Sort order within the tour (gaps allowed). */
  order: number;
  /** Densities this stop is shown for. Absent = shown for all densities.
   *  Lets the welcome tour run fewer stops for Expert users. */
  densities?: Density[];
}

/** Runtime facts resolveTourSteps can gate a whole step on — the dynamic
 *  counterpart to a membership's static `densities` filter. Kept flat (not a
 *  generic context bag) so a new conditional step only needs one more
 *  optional field here, not a new filtering mechanism. */
export interface TourContext {
  /** Mirrors useBraveInstalled() — gates the "install Brave" step so it only
   *  shows when Brave isn't already on the machine. */
  braveInstalled?: boolean;
  /** Whether the Lockdown action is actually available in the right sidebar.
   *  New installs commonly keep it hidden until self-destruct is opted in, so
   *  the walkthrough must not stop on an absent control. */
  lockdownVisible?: boolean;
}

export interface GuideTopic {
  /** Stable slug — unique across all topics. */
  id: string;
  /** Legacy: linked the topic to a panel for the now-removed per-panel "?".
   *  Unread by any current surface. */
  panelId?: PanelId;
  /** Short heading shown in the tour callout. */
  title: string;
  /** One-line summary shown in the tour callout. */
  summary: string;
  /** Legacy: markdown body for the now-removed help center reader. Unread by
   *  any current surface. */
  body: string;
  /** Legacy: search terms for the now-removed help center search. Unread by
   *  any current surface. */
  keywords?: string[];
  /** Legacy: related topic ids for the now-removed help center reader.
   *  Unread by any current surface. */
  related?: string[];
  /** Present only when this topic is also a spotlight-tour stop. */
  tour?: {
    /** CSS selector for the element to highlight, e.g. '[data-tour="nav-privacy"]'.
     *  A comma-separated list is tried in the order written (first match
     *  wins) — NOT the raw CSS "first in document order" semantics, since
     *  useTour resolves each selector individually for exactly this reason:
     *  a written-first fallback anchor can otherwise win just by sitting
     *  earlier in the DOM than the preferred one. */
    anchor: string;
    /** Optional second highlighted rect shown at the same time as `anchor`
     *  — e.g. the whole card PLUS a bolder inner ring around one specific
     *  number inside it. Purely additive; the step still gates/navigates
     *  off the primary `anchor` only. */
    secondaryAnchor?: string;
    /** Preferred callout side relative to the anchor. */
    placement?: TourPlacement;
    /** Switch to this panel before showing the step. */
    navigateTo?: PanelId;
    /** Optional UI event repeatedly dispatched until the anchor mounts. Use
     * this for a tab or disclosure that must open after its panel loads. */
    openEvent?: string;
    /** Which tours include this stop. */
    tours: TourMembership[];
    /** Looping muted video/image shown alongside the copy. Mutually exclusive
     *  with `component` — use whichever fits (a recorded asset vs. reusing a
     *  live in-app animation). */
    media?: TourMedia;
    /** A live component to mount directly inside the hero modal — e.g.
     *  Privacy Shield's own animation, reused as-is instead of shipping a
     *  separate recording or opening a separate dialog. Zero props. */
    component?: ComponentType;
    /** "callout" (default) is the small anchored card. "hero" is a circular
     *  ring around the anchor plus a centered modal with a connector line —
     *  for standout features (Scrub, Lockdown, Privacy Shield) that deserve
     *  a bigger moment. Degrades gracefully to a centered-only modal if the
     *  anchor isn't in the DOM (e.g. Lockdown when the feature isn't
     *  enabled yet). */
    variant?: "callout" | "hero";
    /** When true, the anchor element is clicked once the step is shown —
     *  for opening a real in-app dialog/action as part of the step. */
    autoTrigger?: boolean;
    /** Turns this into a do-it-yourself step: Next stays disabled until a
     *  window CustomEvent of this name fires. An explicit "Skip this step"
     *  control is always shown alongside so the user is never stuck. */
    requiresAction?: {
      eventName: string;
      /** Shown as a warning note before the user acts (e.g. "this may clear
       *  existing clipboard history"). */
      warning?: string;
      /** Swapped in for `title`/`summary` when the anchor's own
       *  `data-tour-state` attribute reads "scanning" or "done" the moment
       *  this step is shown — i.e. the user already started (or finished)
       *  the action before the tour got here, so "click this button" copy
       *  would be stale/confusing. The anchor element opts in by setting
       *  `data-tour-state` itself; useTour treats its absence as "idle" and
       *  leaves the normal copy in place. */
      alreadyStartedTitle?: string;
      alreadyStartedSummary?: string;
      /** Keep the dim/blur backdrop up during this do-it-yourself step
       *  instead of suppressing it. A requiresAction step normally clears
       *  the scrim so the target section is fully legible while the user
       *  acts (2026-07-10); this opts back INTO the blurred backdrop for
       *  steps where the point is to spotlight one region and blur the rest
       *  until the user clicks (e.g. Fix All — 2026-07-20). */
      keepDim?: boolean;
      /** Do not retain a destructive-action warning when the action was
       *  already complete before this step (for example, all findings were
       *  fixed or intentionally ignored). */
      hideWarningWhenPreStarted?: boolean;
    };
    /** Excludes this step from every tour when it returns false against the
     *  context resolveTourSteps is given (default {} — a step without this
     *  is always included, same default as an absent `densities`). For
     *  steps gated on runtime app state rather than density. */
    showWhen?: (ctx: TourContext) => boolean;
    /** A secondary action button in the callout, beyond Next/Back/Skip —
     *  dispatches a window CustomEvent when clicked. For steps needing a
     *  cross-panel deep link (e.g. installing a missing app) rather than a
     *  real element already in the DOM to click through to. */
    action?: {
      label: string;
      eventName: string;
      eventDetail?: Record<string, unknown>;
    };
  };
}

export interface TourMedia {
  type: "video" | "image";
  /** Bundled asset path/URL (import it so Vite resolves it). */
  src: string;
  /** Required for type "image" (accessibility). */
  alt?: string;
}

/** A resolved, ordered tour stop produced by resolveTourSteps. */
export interface TourStep {
  topicId: string;
  title: string;
  summary: string;
  anchor: string;
  secondaryAnchor?: string;
  placement: TourPlacement;
  navigateTo?: PanelId;
  openEvent?: string;
  order: number;
  media?: TourMedia;
  component?: ComponentType;
  variant: "callout" | "hero";
  autoTrigger?: boolean;
  requiresAction?: {
    eventName: string;
    warning?: string;
    alreadyStartedTitle?: string;
    alreadyStartedSummary?: string;
    keepDim?: boolean;
    hideWarningWhenPreStarted?: boolean;
  };
  action?: {
    label: string;
    eventName: string;
    eventDetail?: Record<string, unknown>;
  };
}
