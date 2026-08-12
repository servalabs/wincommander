import { describe, expect, test } from "bun:test";
import { computeScrimPanels, padToHole, type ScrimHole, type ScrimPanel } from "./SpotlightTour";

// Regression coverage for the 2026-07-20 bug: an SVG-mask-based scrim let
// backdrop-filter blur the "highlighted" hole itself (masking only hid the
// dark fill color, not the blur). These panels are the fix — they must be
// geometrically incapable of covering a hole, not just visually excluded.

function overlaps(panel: ScrimPanel, hole: ScrimHole): boolean {
  return panel.x < hole.right && panel.x + panel.w > hole.left &&
    panel.y < hole.bottom && panel.y + panel.h > hole.top;
}

function totalArea(rects: Array<{ w: number; h: number }>): number {
  return rects.reduce((sum, r) => sum + r.w * r.h, 0);
}

function holeArea(holes: ScrimHole[]): number {
  return holes.reduce((sum, h) => sum + (h.right - h.left) * (h.bottom - h.top), 0);
}

describe("computeScrimPanels", () => {
  test("no panel overlaps a single centered hole", () => {
    const holes: ScrimHole[] = [{ left: 400, top: 300, right: 800, bottom: 500 }];
    const panels = computeScrimPanels(1920, 1080, holes);
    for (const p of panels) {
      for (const h of holes) expect(overlaps(p, h)).toBe(false);
    }
  });

  test("no panel overlaps any of three holes (anchor + secondary + sidebar)", () => {
    const holes: ScrimHole[] = [
      { left: 0, top: 0, right: 260, bottom: 1080 }, // sidebar: full-height left strip
      { left: 700, top: 550, right: 1360, bottom: 640 }, // main anchor
      { left: 1200, top: 560, right: 1300, bottom: 600 }, // secondary anchor nested inside it
    ];
    const panels = computeScrimPanels(1920, 1080, holes);
    for (const p of panels) {
      for (const h of holes) expect(overlaps(p, h)).toBe(false);
    }
  });

  test("panels + holes exactly tile the viewport (no gaps, no double-coverage)", () => {
    const vw = 1920;
    const vh = 1080;
    const holes: ScrimHole[] = [
      { left: 0, top: 0, right: 260, bottom: vh },
      { left: 700, top: 550, right: 1360, bottom: 640 },
    ];
    const panels = computeScrimPanels(vw, vh, holes);
    expect(totalArea(panels) + holeArea(holes)).toBe(vw * vh);
  });

  test("hole partially off-screen is clamped to the viewport, not left as a gap", () => {
    const holes: ScrimHole[] = [{ left: -50, top: -50, right: 200, bottom: 200 }];
    const panels = computeScrimPanels(1920, 1080, holes);
    // The clamped hole area (0,0)-(200,200) plus panels must equal the full viewport —
    // proves the out-of-bounds hole edges didn't leave an unaccounted strip.
    const clamped: ScrimHole = { left: 0, top: 0, right: 200, bottom: 200 };
    expect(totalArea(panels) + holeArea([clamped])).toBe(1920 * 1080);
  });

  test("no holes at all still covers the full viewport", () => {
    const panels = computeScrimPanels(1920, 1080, []);
    expect(totalArea(panels)).toBe(1920 * 1080);
  });

  test("padToHole expands a rect by the given padding on all sides", () => {
    const rect = { left: 100, top: 100, width: 50, height: 20 } as DOMRect;
    const hole = padToHole(rect, 8)!;
    expect(hole).toEqual({ left: 92, top: 92, right: 158, bottom: 128 });
  });

  test("padToHole returns null for a null rect", () => {
    expect(padToHole(null, 8)).toBeNull();
  });
});
