// src/lib/tour.ts
//
// Pure tour-resolution logic for the spotlight guide. Kept separate from the
// React controller (src/hooks/useTour.ts) and the registry so it is trivially
// unit-testable: given the topic list + a tour id (+ density), produce the
// ordered list of stops to walk through.

import type { Density } from "../types/persona";
import type { GuideTopic, TourContext, TourStep } from "../content/guide/types";

/** Resolve the ordered stops for a named tour.
 *
 *  - keeps only topics that declare membership in `tourId`;
 *  - drops stops whose `densities` excludes the active density (so the welcome
 *    tour runs fewer stops for Expert users);
 *  - drops stops whose `showWhen` rejects the given `context` (e.g. the
 *    "install Brave" step when Brave is already installed);
 *  - sorts ascending by the per-tour `order`.
 *
 *  Returns `[]` for an unknown tour id. */
export function resolveTourSteps(
  topics: GuideTopic[],
  tourId: string,
  density?: Density,
  context: TourContext = {},
): TourStep[] {
  const steps: TourStep[] = [];
  for (const topic of topics) {
    const tour = topic.tour;
    if (!tour) continue;
    const membership = tour.tours.find((m) => m.id === tourId);
    if (!membership) continue;
    if (density && membership.densities && !membership.densities.includes(density)) {
      continue;
    }
    if (tour.showWhen && !tour.showWhen(context)) continue;
    steps.push({
      topicId: topic.id,
      title: topic.title,
      summary: topic.summary,
      anchor: tour.anchor,
      secondaryAnchor: tour.secondaryAnchor,
      placement: tour.placement ?? "auto",
      navigateTo: tour.navigateTo,
      openEvent: tour.openEvent,
      order: membership.order,
      media: tour.media,
      component: tour.component,
      variant: tour.variant ?? "callout",
      autoTrigger: tour.autoTrigger,
      requiresAction: tour.requiresAction,
      action: tour.action,
    });
  }
  return steps.sort((a, b) => a.order - b.order);
}

/** The tour id for a panel's "Replay tour" button, by convention
 *  `tour-<panelId>` (see tour-dashboard, tour-privacy in topics.ts). Falls
 *  back to "welcome" if that panel has no tour of its own — checked against
 *  the real topic registry, not a hand-maintained list, so newly added
 *  per-panel tours pick this up automatically as long as they follow the
 *  same naming convention. */
export function tourIdForPanel(topics: GuideTopic[], panelId: string): string {
  const candidate = `tour-${panelId}`;
  const hasTour = topics.some((t) => t.tour?.tours.some((m) => m.id === candidate));
  return hasTour ? candidate : "welcome";
}
