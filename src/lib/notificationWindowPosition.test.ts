import { describe, expect, test } from "bun:test";
import { getNotificationWindowBounds } from "./notificationWindowPosition";

describe("getNotificationWindowBounds", () => {
  test("anchors to the physical work-area bottom-right at 150% DPI", () => {
    expect(getNotificationWindowBounds({
      position: { x: 0, y: 0 },
      size: { width: 2560, height: 1400 },
    }, 1.5)).toEqual({
      x: 1882,
      y: 602,
      width: 660,
      height: 780,
    });
  });

  test("keeps the window inside a monitor with negative desktop coordinates", () => {
    expect(getNotificationWindowBounds({
      position: { x: -1920, y: -120 },
      size: { width: 1920, height: 1040 },
    }, 1)).toEqual({
      x: -452,
      y: 388,
      width: 440,
      height: 520,
    });
  });

  test("shrinks both dimensions when the work area is smaller than the toast window", () => {
    expect(getNotificationWindowBounds({
      position: { x: 100, y: 200 },
      size: { width: 300, height: 240 },
    }, 1)).toEqual({
      x: 112,
      y: 212,
      width: 276,
      height: 216,
    });
  });

  test("falls back to scale factor one for an invalid scale", () => {
    expect(getNotificationWindowBounds({
      position: { x: 0, y: 0 },
      size: { width: 800, height: 600 },
    }, Number.NaN)).toEqual({
      x: 348,
      y: 68,
      width: 440,
      height: 520,
    });
  });
});
