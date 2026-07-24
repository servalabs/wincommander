// src/content/guide/index.ts
//
// Public surface for the guide content SSOT: the topic list + the lookup
// helper the spotlight tour's title-bar entry point needs (the help-center
// search/lookup helpers were removed along with the help center itself).
// Pure functions live in src/lib/tour.ts (tour resolution) so they stay
// trivially testable.

import { GUIDE_TOPICS } from "./topics";
import type { GuideTopic } from "./types";

export type {
  GuideTopic,
  TourMembership,
  TourPlacement,
  TourStep,
} from "./types";
export { GUIDE_TOPICS } from "./topics";

/** All topics, registry order. */
export function allTopics(): GuideTopic[] {
  return GUIDE_TOPICS;
}
