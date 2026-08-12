// src/components/shared/EmbeddedWebView.tsx
//
// Universal embedded WebView component using Tauri v2 multiwebview (native WebView2).
//
// WHY NATIVE WEBVIEW: iframes are blocked by X-Frame-Options / CSP frame-ancestors
// on most self-hosted apps (Immich, Nextcloud, Syncthing, etc.).
// Tauri's native child webview bypasses this entirely.
//
// USAGE:
//   <EmbeddedWebView
//     group="server-app"
//     id="my-app"
//     url="http://192.168.1.10:8080"
//     customCss="body { display: none }"     ← optional injection CSS
//     label="My App"                          ← shown in loading state
//   />
//
// The component renders an empty <div> whose bounds are measured and passed
// to Rust (open_server_app / resize_server_app). The native webview renders
// ON TOP of DOM at those exact pixel coordinates.
//
// CRITICAL LAYOUT RULE: Native child webviews render above ALL DOM/CSS.
// Never place overlapping DOM elements (sidebars, dropdowns, tooltips) at
// the same z-coordinates as the webview bounds — they'll be covered.
// The caller is responsible for keeping the content div free of overlaps.
//
// LEARNING: A small 100ms stabilize delay before opening is required.
// Without it, rect.width/height can be 0 on first mount during panel animation.
//
// LEARNING: resize_server_app must use requestAnimationFrame to debounce
// ResizeObserver callbacks, otherwise you get multiple rapid fire calls on
// theme changes / window resize.

import { useRef, useEffect, useCallback, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Spinner, NonIdealState, Button } from "@/components/ui/bp";

interface EmbeddedWebViewProps {
  /** Tauri webview group name (used for bulk hide/close operations) */
  group: string;
  /** Unique webview ID within the group */
  id: string;
  /** URL to load */
  url: string;
  /** CSS to inject into the webview page (runs before page JS via initialization_script) */
  customCss?: string | null;
  /**
   * JS to inject into the webview page for branding fixes. Body of a
   * function that runs on DOMContentLoaded AND on every DOM mutation
   * (so SPA / React route changes get re-branded). Must be idempotent.
   * Used by the mesh-login flow to swap "Tailscale" → "Private Mesh"
   * in the live page text, which pure CSS cannot do.
   */
  customJs?: string | null;
  /**
   * Route this webview's storage to a separate per-group data dir and
   * erase it on each open. Used by mesh-login so a stale auth cookie
   * doesn't make the next sign-in silently auto-complete without
   * showing the login form. Default false (persistent cookies, the
   * normal behavior for server-apps like Immich / Nextcloud).
   */
  ephemeral?: boolean;
  /** Human-readable label shown in loading state */
  label?: string;
  /** Optional className applied to the outer container div */
  className?: string;
  /** Optional inline styles applied to the outer container div (e.g. for border/border-radius) */
  style?: CSSProperties;
  /** Called when the webview successfully opens */
  onOpen?: () => void;
  /** Called when opening fails */
  onError?: (err: string) => void;
}

export default function EmbeddedWebView({
  group,
  id,
  url,
  customCss,
  customJs,
  ephemeral,
  label,
  className,
  style,
  onOpen,
  onError,
}: EmbeddedWebViewProps) {
  const contentRef = useRef<HTMLDivElement>(null);
  const lastBoundsRef = useRef<{ x: number; y: number; w: number; h: number } | null>(null);
  const hiddenByOverlayRef = useRef(false);
  // Tracks whether this instance is still mounted. The open path awaits a
  // stabilize delay and an async invoke; on a fast panel switch the unmount
  // cleanup (hide_all_server_apps) can run first, then the in-flight open
  // resolves and re-shows the native webview OVER the next panel. This guard
  // drops (and re-hides) any open that completes after unmount.
  const isMountedRef = useRef(true);
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const readStableBounds = useCallback(() => {
    const el = contentRef.current;
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    if (rect.width < 10 || rect.height < 10) return null;
    return {
      x: Math.round(rect.left),
      y: Math.round(rect.top),
      w: Math.round(rect.width),
      h: Math.round(rect.height),
    };
  }, []);

  const hasBoundsChanged = useCallback((next: { x: number; y: number; w: number; h: number }) => {
    const prev = lastBoundsRef.current;
    if (prev && prev.x === next.x && prev.y === next.y && prev.w === next.w && prev.h === next.h) {
      return false;
    }
    lastBoundsRef.current = next;
    return true;
  }, []);

  const openWebview = useCallback(async () => {
    // Guard: Don't open if a modal/overlay is currently visible. Detect both
    // the legacy Blueprint body class and Radix (V2) dialogs — Radix locks body
    // scroll via data-scroll-locked and mounts [data-state="open"] dialogs.
    if (
      document.body.classList.contains("bp6-overlay-open") ||
      document.body.hasAttribute("data-scroll-locked") ||
      document.querySelector('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]')
    ) {
      return;
    }

    // Stabilize delay: let DOM/panel animation settle before measuring bounds
    await new Promise(r => setTimeout(r, 100));
    // Bail if we unmounted during the delay (panel switched away).
    if (!isMountedRef.current) return;

    const bounds = readStableBounds();
    if (!bounds) return;
    lastBoundsRef.current = bounds;

    setIsLoading(true);
    setError(null);

    try {
      await invoke("open_server_app", {
        group,
        id,
        url,
        customCss: customCss || null,
        customJs: customJs || null,
        ephemeral: ephemeral || false,
        x: bounds.x,
        y: bounds.y,
        w: bounds.w,
        h: bounds.h,
      });
      // Unmounted while the open was in flight — hide it back so it can't sit
      // on top of the panel the user navigated to.
      if (!isMountedRef.current) {
        invoke("hide_all_server_apps", { group }).catch(() => { });
        return;
      }
      onOpen?.();
    } catch (err) {
      const msg = String(err);
      console.error(`[EmbeddedWebView] Failed to open ${id}:`, msg);
      setError(msg);
      onError?.(msg);
    } finally {
      setTimeout(() => setIsLoading(false), 800);
    }
  }, [group, id, url, customCss, customJs, ephemeral, onOpen, onError, readStableBounds]);

  // Open/re-open when url or CSS changes
  useEffect(() => {
    openWebview();
  }, [openWebview]);

  // ResizeObserver — keep native webview bounds synced with DOM element bounds
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;

    let rafId: number | null = null;
    let throttleId: ReturnType<typeof setTimeout> | null = null;

    const syncBounds = () => {
      throttleId = null;
      if (rafId) cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        const bounds = readStableBounds();
        if (!bounds || !hasBoundsChanged(bounds)) return;
        invoke("resize_server_app", {
          group,
          id,
          x: bounds.x,
          y: bounds.y,
          w: bounds.w,
          h: bounds.h,
        }).catch(() => { });
      });
    };

    const observer = new ResizeObserver(() => {
      if (throttleId) return;
      throttleId = setTimeout(syncBounds, 80);
    });

    observer.observe(el);
    return () => {
      observer.disconnect();
      if (throttleId) clearTimeout(throttleId);
      if (rafId) cancelAnimationFrame(rafId);
    };
  }, [group, id, readStableBounds, hasBoundsChanged]);

  // Hide webview when component unmounts (user navigates away)
  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      invoke("hide_all_server_apps", { group }).catch(() => { });
    };
  }, [group]);

  // Overlay detection — hide webview when a modal/dialog is open
  useEffect(() => {
    const checkOverlay = () => {
      const isOverlayOpen =
        document.body.classList.contains("bp6-overlay-open") ||
        document.body.hasAttribute("data-scroll-locked") ||
        !!document.querySelector('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]');
      if (isOverlayOpen) {
        if (hiddenByOverlayRef.current) return;
        hiddenByOverlayRef.current = true;
        invoke("hide_all_server_apps", { group }).catch(() => { });
      } else if (hiddenByOverlayRef.current) {
        hiddenByOverlayRef.current = false;
        openWebview();
      }
    };

    // Initial check
    checkOverlay();

    const observer = new MutationObserver((mutations) => {
      for (const mutation of mutations) {
        if (
          mutation.type === "attributes" &&
          (mutation.attributeName === "class" ||
            mutation.attributeName === "data-scroll-locked" ||
            mutation.attributeName === "style")
        ) {
          checkOverlay();
          break;
        }
      }
    });

    observer.observe(document.body, {
      attributes: true,
      attributeFilter: ["class", "data-scroll-locked", "style"],
    });
    return () => observer.disconnect();
  }, [group, openWebview]);

  return (
    <div
      ref={contentRef}
      className={className}
      style={{
        flex: 1,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--color-bg-primary)",
        position: "relative",
        minHeight: 0,
        width: "100%",
        height: "100%",
        ...style,
      }}
    >
      {isLoading && (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 16, color: "var(--color-text-secondary)" }}>
          <Spinner size={40} intent="primary" />
          <div style={{ fontFamily: "var(--font-mono)", fontSize: 11, opacity: 0.7 }}>
            Connecting to {label || url}...
          </div>
        </div>
      )}
      {error && !isLoading && (
        <NonIdealState
          icon="error"
          title="Connection Failed"
          description={error}
          action={
            <Button icon="refresh" onClick={() => { void openWebview(); }}>
              Retry
            </Button>
          }
        />
      )}
    </div>
  );
}
