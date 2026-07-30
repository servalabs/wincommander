import { describe, expect, test } from "bun:test";
import {
  errorMessage,
  importPanelWithRetry,
  isChunkLoadError,
  isViteHmrRuntimeError,
} from "./panelLoading";

describe("panel loading recovery", () => {
  test("recognises transient dynamic-import failures", () => {
    expect(isChunkLoadError(new TypeError("Failed to fetch dynamically imported module: /assets/panel.js"))).toBe(true);
    expect(isChunkLoadError(new Error("Rendered more hooks than during the previous render"))).toBe(false);
  });

  test("recognises stale Vite hot-update runtime failures", () => {
    expect(isViteHmrRuntimeError(new ReferenceError("__vite__updateStyle is not defined"))).toBe(true);
    expect(isViteHmrRuntimeError(new SyntaxError(
      "The requested module '/src/components/shared/WCSwitch.tsx' does not provide an export named 'default'",
    ))).toBe(true);
    expect(isViteHmrRuntimeError(new SyntaxError(
      "The requested module 'third-party-package' does not provide an export named 'default'",
    ))).toBe(false);
    expect(isViteHmrRuntimeError(new Error("Cannot read properties of undefined"))).toBe(false);
  });

  test("retries a transient chunk failure once", async () => {
    let attempts = 0;
    const load = importPanelWithRetry(async () => {
      attempts += 1;
      if (attempts === 1) throw new TypeError("Failed to fetch dynamically imported module");
      return { default: "panel" };
    }, 0);

    expect(await load()).toEqual({ default: "panel" });
    expect(attempts).toBe(2);
  });

  test("does not retry deterministic module errors", async () => {
    let attempts = 0;
    const failure = new Error("Module evaluation failed");
    const load = importPanelWithRetry(async () => {
      attempts += 1;
      throw failure;
    }, 0);

    let caught: unknown;
    try {
      await load();
    } catch (error) {
      caught = error;
    }
    expect(caught).toBe(failure);
    expect(attempts).toBe(1);
  });

  test("normalises non-Error values for diagnostics", () => {
    expect(errorMessage("plain failure")).toBe("plain failure");
    expect(errorMessage({ reason: "bad payload" })).toBe('{"reason":"bad payload"}');
  });
});
