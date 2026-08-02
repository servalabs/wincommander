import { describe, expect, test } from "bun:test";
import componentSource from "./bp.tsx?raw";

describe("Dialog/AlertDialog overlay rendering", () => {
  test("modal overlays skip backdrop-blur to avoid full-viewport GPU recomposite artifacts", () => {
    // A full-screen backdrop-filter blur repaints every frame anything behind
    // it animates, and on WebView2/software-rendered VM hosts that GPU blur
    // pass can leave stale smeared regions on screen (looked like a broken
    // input in the RDP "Remote Connections" dialog). Keep the dimming via
    // opaque-ish background only, matching the fix already applied to
    // Sidebar/RightSidebar/dashboard chrome.
    const overlayMatches = componentSource.match(/<R(Dialog|AlertDialog)\.Overlay[\s\S]*?\/>/g) ?? [];
    expect(overlayMatches.length).toBeGreaterThanOrEqual(2);
    for (const overlay of overlayMatches) {
      expect(overlay).toContain("bg-black/70");
      expect(overlay).not.toContain("backdrop-blur");
    }
  });

  test("modal content carries gpu-clip to avoid corner-bleed on its zoom-in-95/zoom-out-95 scale transform", () => {
    // Removing backdrop-blur (above) fixed the overlay's own artifact, but the
    // Dialog/AlertDialog *Content* box also animates via a scale transform
    // (zoom-in-95 open / zoom-out-95 close) — the same class of transform that
    // needed the "gpu-clip" isolation+will-change fix on Button/Chip to stop
    // Tauri WebView2 corner-bleed. Content lacked it, which is what actually
    // produced the smeared "Remote Connections" dialog inputs, not the blur.
    const contentMatches = componentSource.match(/<R(Dialog|AlertDialog)\.Content[\s\S]*?className=\{cn\([\s\S]*?\)\}/g) ?? [];
    expect(contentMatches.length).toBeGreaterThanOrEqual(2);
    for (const content of contentMatches) {
      expect(content).toContain("gpu-clip");
      expect(content).toContain("zoom-in-95");
    }
  });
});

describe("Blueprint-compatible button semantics", () => {
  test("forwards declared native interaction and pressed-state props", () => {
    expect(componentSource).toContain('"aria-pressed": ariaPressed');
    expect(componentSource).toContain("aria-pressed={ariaPressed}");
    expect(componentSource).toContain("onBlur={onBlur}");
    expect(componentSource).toContain("id={id}");
    expect(componentSource).toContain("name={name}");
  });

  test("honors modern and legacy size aliases instead of silently dropping them", () => {
    expect(componentSource).toContain('size?: ButtonSize | "small" | "large"');
    expect(componentSource).toContain('size === "large" ? "lg" : size === "small" ? "sm" : size');
    expect(componentSource).toContain("requestedVariant ?? pickVariant");
  });
});
