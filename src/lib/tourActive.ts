// src/lib/tourActive.ts
//
// "Is a spotlight tour running right now" — a module store rather than context
// because the two consumers sit on opposite sides of the app: a panel deciding
// whether to render a tour-anchored section, and App.tsx's plain
// handlePanelChange callback, which needs to read this without re-subscribing.
//
// WHY IT EXISTS: tour steps anchor to DOM elements that some surfaces hide
// outside Expert density (Browser Hardening) or behind a disabled module
// (System Cleanup's Scan All, off by default for the Casual persona). When the
// anchor never mounts, useTour polls for ~8s, gives up, and the step renders as
// a centred callout over the wrong panel — the tour visibly breaks for exactly
// the Guided/Casual users it exists to help (2026-07-26 fix). Revealing those
// anchors FOR THE DURATION OF THE TOUR keeps the walkthrough whole without
// changing what either mode shows normally.

import { useSyncExternalStore } from "react";

let tourActive = false;
const subscribers = new Set<() => void>();

function subscribe(onStoreChange: () => void): () => void {
  subscribers.add(onStoreChange);
  return () => subscribers.delete(onStoreChange);
}

/** Non-reactive read, for event handlers that must not re-subscribe. */
export function isTourActive(): boolean {
  return tourActive;
}

export function setTourActive(active: boolean): void {
  if (tourActive === active) return;
  tourActive = active;
  subscribers.forEach((notify) => notify());
}

/** Reactive read — re-renders the caller when a tour starts or ends. */
export function useTourActive(): boolean {
  return useSyncExternalStore(subscribe, isTourActive, isTourActive);
}
