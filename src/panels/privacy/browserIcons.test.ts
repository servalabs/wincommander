import { describe, expect, test } from "bun:test";
import { resolveBrowserIconSlug, resolveBrowserIconUrl } from "./browserIcons";

describe("resolveBrowserIconSlug", () => {
  test("maps detected browser names to shared-asset filenames", () => {
    expect(resolveBrowserIconSlug("Google Chrome")).toBe("chrome");
    expect(resolveBrowserIconSlug("Microsoft Edge")).toBe("edge");
    expect(resolveBrowserIconSlug("Firefox")).toBe("firefox");
    expect(resolveBrowserIconSlug("Opera GX")).toBe("opera-gx");
  });

  test("returns undefined for unsupported browsers", () => {
    expect(resolveBrowserIconSlug("Arc")).toBeUndefined();
  });
});

describe("resolveBrowserIconUrl", () => {
  test("resolves browser logos from the shared assets map", () => {
    const logos = {
      "chrome.svg": "/assets/chrome.svg",
      "opera-gx.svg": "/assets/opera-gx.svg",
    };

    expect(resolveBrowserIconUrl("Google Chrome", logos)).toBe("/assets/chrome.svg");
    expect(resolveBrowserIconUrl("Opera GX", logos)).toBe("/assets/opera-gx.svg");
  });

  test("falls back to undefined when the shared asset is missing", () => {
    expect(resolveBrowserIconUrl("Microsoft Edge", {})).toBeUndefined();
  });
});
