// src/components/EverythingSearchBar.tsx
//
// EverythingSearchBar — a Listary-style floating search bar.
// Two modes:
//   overlayMode=false (default): renders inside the main app as a modal-style overlay,
//                                toggled by the "toggle-search-bar" Tauri event.
//   overlayMode=true:            renders as the sole content of the dedicated
//                                "search-overlay" transparent Tauri window.
//                                The window is shown/hidden by the global hotkey in lib.rs.

import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon, Spinner } from "@/components/ui/bp";
import { AnimatePresence, motion } from "framer-motion";
import type { AppSettings } from "../types/settings";
import { buildContentQueryArgs, contentHitToDisplayRow, dedupeContentRows, isNameOnlyMatch } from "@/lib/contentSearch";
import type { ContentDisplayRow } from "@/lib/contentSearch";
import type { ContentHit } from "@/types/wincmd-search";
import SearchResultContextMenu from "./SearchResultContextMenu";
import { useSearchResultContextMenu } from "@/hooks/useSearchResultContextMenu";
import "./EverythingSearchBar.css";

interface SearchResult {
  name: string;
  directory: string;
  full_path: string;
  size: string;
  modified: string;
  icon_data?: string | null;
}

interface SearchResponse {
  results: SearchResult[];
  total: number;
  query: string;
}

const esbIconCache = new Map<string, string | null>();

function getFallbackSearchIcon(result: SearchResult) {
  if (result.size === "0" || result.size === "") return { icon: "folder-close" as any, className: "esb-result-icon esb-icon-folder" };
  const ext = result.name.split(".").pop()?.toLowerCase() || "";
  if (["exe", "msi", "appx", "appxbundle", "msix", "lnk"].includes(ext)) return { icon: "application" as any, className: "esb-result-icon esb-icon-app" };
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "heic"].includes(ext)) return { icon: "media" as any, className: "esb-result-icon esb-icon-image" };
  if (["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm"].includes(ext)) return { icon: "video" as any, className: "esb-result-icon esb-icon-video" };
  if (["mp3", "wav", "flac", "m4a", "ogg", "aac"].includes(ext)) return { icon: "music" as any, className: "esb-result-icon esb-icon-audio" };
  if (["zip", "rar", "7z", "tar", "gz", "iso", "cab"].includes(ext)) return { icon: "compressed" as any, className: "esb-result-icon esb-icon-archive" };
  if (["js", "ts", "tsx", "jsx", "py", "rs", "go", "java", "cpp", "cs", "html", "css", "json", "yml", "yaml", "ps1", "bat", "cmd"].includes(ext)) return { icon: "code" as any, className: "esb-result-icon esb-icon-code" };
  return { icon: "document" as any, className: "esb-result-icon esb-icon-doc" };
}

function NativeSearchIcon({ result }: { result: SearchResult }) {
  const isDir = result.size === "0" || result.size === "";
  const [iconData, setIconData] = useState<string | null>(() => result.icon_data ?? esbIconCache.get(result.full_path) ?? null);
  const fallback = getFallbackSearchIcon(result);

  useEffect(() => {
    let cancelled = false;
    if (isDir) return;
    if (result.icon_data) {
      esbIconCache.set(result.full_path, result.icon_data);
      setIconData(result.icon_data);
      return;
    }
    if (esbIconCache.has(result.full_path)) {
      setIconData(esbIconCache.get(result.full_path) ?? null);
      return;
    }
    invoke<string | null>("get_file_icon_data", { path: result.full_path })
      .then(data => {
        esbIconCache.set(result.full_path, data);
        if (!cancelled) setIconData(data);
      })
      .catch(() => {
        esbIconCache.set(result.full_path, null);
        if (!cancelled) setIconData(null);
      });
    return () => { cancelled = true; };
  }, [isDir, result.full_path, result.icon_data]);

  if (iconData) return <img src={iconData} alt="" className="esb-native-icon" />;
  return <Icon icon={fallback.icon} size={14} className={fallback.className} />;
}

// Separator chars (space, dash, comma, dot) become * wildcards.
// A trailing * is always appended so "brave" becomes "brave*" — prefix match
// that finds BraveBrowser.exe, Brave Setup.exe, etc.
function normalizeSearchQuery(raw: string): string {
  const normalized = raw.trim().replace(/[\s\-,\.]+/g, "*");
  return normalized.endsWith("*") ? normalized : `${normalized}*`;
}

function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
}

function nameWithoutExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return (dot >= 0 ? name.slice(0, dot) : name).toLowerCase();
}

const APP_EXTS = new Set(["exe", "msi", "appx", "msix", "lnk"]);

// Returns sort priority — lower = shown first.
// Apps (any .exe/.lnk/.msi/.appx/.msix) ALWAYS beat data files, regardless
// of where they live on disk. The previous path-based scoring missed apps
// installed under non-standard locations like %LOCALAPPDATA%\GitHubDesktop,
// %LOCALAPPDATA%\Discord, etc. Now we trust the extension: if it's an app
// extension, it ranks above any data file.
//
//   0: exact basename match on an app extension (typing "brave" → brave.exe)
//   1: basename starts with the query, app extension (BraveBeta.exe for "brave")
//   2: any .lnk (Start Menu shortcuts and similar)
//   3: any .exe / .msi / .appx / .msix anywhere else
//   4: everything else (data files, images, logs)
function appSortScore(result: SearchResult, queryLower: string): number {
  const ext = extOf(result.name);
  const isApp = APP_EXTS.has(ext);
  if (isApp) {
    const base = nameWithoutExt(result.name);
    if (base === queryLower) return 0;
    if (base.startsWith(queryLower)) return 1;
  }
  if (ext === "lnk") return 2;
  if (ext === "exe" || ext === "msi" || ext === "appx" || ext === "msix") return 3;
  return 4;
}

// Hard-coded entries for Windows built-in apps that aren't reliably indexed
// as regular files (UWP apps live in WindowsApps which Everything skips by
// default; some shell-only entry-points like Settings have no real file at
// all — they're URI-launchable). When the user's query matches any of an
// entry's keywords, we inject the entry at the top of results so File
// Explorer / Settings / Calculator etc. always show up.
interface BuiltinApp {
  name: string;       // Display name shown in the result row
  keywords: string[]; // Lowercase keywords that should match
  path: string;       // What we hand to open_path (file path or shell URI)
  iconExt?: string;   // Hint for the fallback icon picker
}
const BUILTIN_APPS: BuiltinApp[] = [
  { name: "File Explorer", keywords: ["file explorer", "explorer", "files"], path: "C:\\Windows\\explorer.exe", iconExt: "exe" },
  { name: "Settings", keywords: ["settings", "windows settings"], path: "ms-settings:", iconExt: "exe" },
  { name: "Control Panel", keywords: ["control panel", "control"], path: "C:\\Windows\\System32\\control.exe", iconExt: "exe" },
  { name: "Task Manager", keywords: ["task manager", "taskmgr"], path: "C:\\Windows\\System32\\Taskmgr.exe", iconExt: "exe" },
  { name: "Command Prompt", keywords: ["cmd", "command prompt"], path: "C:\\Windows\\System32\\cmd.exe", iconExt: "exe" },
  { name: "PowerShell", keywords: ["powershell", "pwsh", "ps"], path: "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe", iconExt: "exe" },
  { name: "Calculator", keywords: ["calc", "calculator"], path: "calculator:", iconExt: "exe" },
  { name: "Notepad", keywords: ["notepad"], path: "C:\\Windows\\System32\\notepad.exe", iconExt: "exe" },
  { name: "Paint", keywords: ["paint", "mspaint"], path: "C:\\Windows\\System32\\mspaint.exe", iconExt: "exe" },
  { name: "Snipping Tool", keywords: ["snip", "snipping tool", "screenshot"], path: "C:\\Windows\\System32\\SnippingTool.exe", iconExt: "exe" },
  { name: "Registry Editor", keywords: ["regedit", "registry"], path: "C:\\Windows\\regedit.exe", iconExt: "exe" },
  { name: "Run", keywords: ["run"], path: "C:\\Windows\\System32\\rundll32.exe", iconExt: "exe" },
  { name: "Device Manager", keywords: ["device manager", "devmgmt"], path: "C:\\Windows\\System32\\devmgmt.msc", iconExt: "exe" },
  { name: "Disk Management", keywords: ["disk management", "diskmgmt"], path: "C:\\Windows\\System32\\diskmgmt.msc", iconExt: "exe" },
  { name: "Services", keywords: ["services", "services.msc"], path: "C:\\Windows\\System32\\services.msc", iconExt: "exe" },
  { name: "System Configuration", keywords: ["msconfig", "system configuration"], path: "C:\\Windows\\System32\\msconfig.exe", iconExt: "exe" },
];

function builtinMatches(queryLower: string): SearchResult[] {
  if (!queryLower) return [];
  return BUILTIN_APPS
    .filter(app => app.keywords.some(k => k.startsWith(queryLower) || queryLower.startsWith(k)))
    .map(app => ({
      name: app.name + (app.iconExt ? "." + app.iconExt : ""),
      directory: app.path.startsWith("ms-") || app.path.endsWith(":") ? "Windows shell" : (app.path.lastIndexOf("\\") > 0 ? app.path.slice(0, app.path.lastIndexOf("\\")) : ""),
      full_path: app.path,
      size: "",
      modified: "",
      icon_data: null,
    }));
}

export default function EverythingSearchBar({ overlayMode = false }: { overlayMode?: boolean }) {
  // In overlay mode the Rust side shows/hides the window — always render.
  // In normal mode we gate rendering via `visible`.
  const [visible, setVisible] = useState(overlayMode);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [results, setResults] = useState<SearchResult[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [inputKey, setInputKey] = useState(0);
  const [contentRows, setContentRows] = useState<ContentDisplayRow[]>([]);

  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const contentDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const focusPollRef = useRef<number | null>(null);
  const lastShowTimeRef = useRef(0);
  const unlockKeywordRef = useRef("unlock");
  const lockKeywordRef = useRef("lock");

  const resetState = useCallback(() => {
    setQuery("");
    setResults([]);
    setContentRows([]);
    setError(null);
    setSelectedIndex(0);
    setInputKey(k => k + 1);
  }, []);

  // Focus the input with a polled retry loop. Single .focus() calls race
  // against Windows' foreground-window promotion when the global hotkey
  // shows the overlay — the renderer's first attempt sometimes lands
  // before the webview is the foreground window, and the OS silently
  // drops the focus request. We poll every 30ms for up to ~1s, calling
  // window.focus() first to nudge the webview foreground, and stop the
  // moment the input is the active element.
  const focusInputUntilStuck = useCallback(() => {
    if (focusPollRef.current !== null) {
      clearInterval(focusPollRef.current);
      focusPollRef.current = null;
    }
    const tryFocus = () => {
      const el = inputRef.current;
      if (!el) return false;
      try { window.focus(); } catch { /* */ }
      el.focus();
      el.select();
      return document.activeElement === el && document.hasFocus();
    };
    requestAnimationFrame(tryFocus);
    let attempts = 0;
    focusPollRef.current = window.setInterval(() => {
      attempts += 1;
      if (tryFocus() || attempts > 50) {
        if (focusPollRef.current !== null) {
          clearInterval(focusPollRef.current);
          focusPollRef.current = null;
        }
      }
    }, 30);
  }, []);

  const close = useCallback(() => {
    if (overlayMode) {
      // Hide the dedicated overlay window without touching the main app window.
      // If hide() is denied by capabilities or otherwise fails, fall back to
      // minimize so the overlay disappears regardless.
      const win = getCurrentWindow();
      win.hide().catch(() => {
        win.minimize().catch(() => {});
      });
      resetState();
      // visible stays true — next show() will already have the card ready
      return;
    }
    setVisible(false);
    resetState();
  }, [overlayMode, resetState]);

  useEffect(() => {
    if (!overlayMode) return;
    const onWindowKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        close();
        return;
      }
      if (e.key === " " && e.ctrlKey) {
        e.preventDefault();
        e.stopPropagation();
        close();
        return;
      }
    };
    window.addEventListener("keydown", onWindowKeyDown, true);
    return () => window.removeEventListener("keydown", onWindowKeyDown, true);
  }, [overlayMode, close]);

  // ── OVERLAY MODE: reset & focus whenever the window is shown ──
  // Uses the window-level focus event (fired by Rust's overlay.show() + set_focus()).
  // We hammer focus at three offsets because on Windows the OS focus can race
  // the renderer when the global hotkey fires — a single attempt sometimes
  // lands before the webview is the foreground window.
  useEffect(() => {
    if (!overlayMode) return;
    const win = getCurrentWindow();
    let unlistenFocus: (() => void) | null = null;
    let unlistenBlur: (() => void) | null = null;
    let unlistenExplicitFocus: (() => void) | null = null;

    const triggerOverlayFocus = () => {
      // Record the timestamp so the blur guard (below) can ignore OS-level
      // blur events that fire during the foreground-promotion race window.
      lastShowTimeRef.current = Date.now();
      resetState();
      setVisible(true);
      
      // Focus immediately
      if (inputRef.current) {
        inputRef.current.focus();
        inputRef.current.select();
      }
      
      // Focus on next paint
      requestAnimationFrame(() => {
        if (inputRef.current) {
          inputRef.current.focus();
          inputRef.current.select();
        }
      });

      // Focus via poll
      focusInputUntilStuck();
    };

    win.listen("tauri://focus", () => {
      triggerOverlayFocus();
    }).then(fn => { unlistenFocus = fn; });
    
    win.listen("tauri://blur", () => {
      // Ignore blur events that fire during the OS foreground-promotion
      // race (first 600 ms after the overlay is shown). Windows sometimes
      // sends a spurious blur to a window that is in the middle of being
      // activated, which would otherwise close the overlay immediately.
      if (Date.now() - lastShowTimeRef.current < 600) return;
      close();
    }).then(fn => { unlistenBlur = fn; });
    
    // Explicit "focus the input" cue from Rust's handle_search_hotkey.
    // tauri://focus can no-op when Windows refuses SetForegroundWindow,
    // so this is the reliable path the global hotkey always fires.
    win.listen("focus-search-input", () => {
      triggerOverlayFocus();
    }).then(fn => { unlistenExplicitFocus = fn; });

    // Native browser focus / visibility events fire when the WebView2
    // child HWND actually becomes active inside the native window — this
    // happens AFTER Tauri's window-level events on Windows, and is the
    // last reliable signal that the input is allowed to take focus.
    const onWinFocus = () => {
      if (inputRef.current) {
        inputRef.current.focus();
        inputRef.current.select();
      }
      focusInputUntilStuck();
    };
    const onVisibilityChange = () => {
      if (!document.hidden) {
        if (inputRef.current) {
          inputRef.current.focus();
          inputRef.current.select();
        }
        focusInputUntilStuck();
      }
    };
    window.addEventListener("focus", onWinFocus);
    document.addEventListener("visibilitychange", onVisibilityChange);

    // Initial mount (first open of the lazy-created overlay window).
    triggerOverlayFocus();
    
    return () => {
      unlistenFocus?.();
      unlistenBlur?.();
      unlistenExplicitFocus?.();
      window.removeEventListener("focus", onWinFocus);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      if (focusPollRef.current !== null) {
        clearInterval(focusPollRef.current);
        focusPollRef.current = null;
      }
    };
  }, [overlayMode, resetState, close, focusInputUntilStuck]);

  // ── NON-OVERLAY MODE: listen for "toggle-search-bar" Tauri event ──
  useEffect(() => {
    if (overlayMode) return;
    const unlisten = listen("toggle-search-bar", () => {
      setVisible(prev => {
        if (!prev) {
          resetState();
          focusInputUntilStuck();
        }
        return !prev;
      });
    });
    return () => { unlisten.then(fn => fn()); };
  }, [overlayMode, resetState, focusInputUntilStuck]);

  useEffect(() => {
    if (overlayMode) return;
    const unlisten = listen("open-search-files-panel", () => {
      setVisible(false);
      resetState();
      window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "search-files" }));
    });
    return () => { unlisten.then(fn => fn()); };
  }, [overlayMode, resetState]);

  // Allow frontend code to open it via custom DOM event (Sidebar button, etc.)
  useEffect(() => {
    if (overlayMode) return;
    const handler = () => {
      setVisible(true);
      resetState();
      focusInputUntilStuck();
    };
    window.addEventListener("open-search-bar", handler);
    return () => window.removeEventListener("open-search-bar", handler);
  }, [overlayMode, resetState, focusInputUntilStuck]);

  // Force focus input whenever visible changes to true
  useEffect(() => {
    if (!visible) return;
    invoke<AppSettings>("get_settings")
      .then((settings) => {
        unlockKeywordRef.current = settings.app?.unlockKeyword?.trim().toLowerCase() || "unlock";
        lockKeywordRef.current = settings.app?.lockKeyword?.trim().toLowerCase() || "lock";
      })
      .catch(() => {
        unlockKeywordRef.current = "unlock";
        lockKeywordRef.current = "lock";
      });
    
    // Focus immediately
    if (inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
    
    // Also focus after a frame
    const raf = requestAnimationFrame(() => {
      if (inputRef.current) {
        inputRef.current.focus();
        inputRef.current.select();
      }
    });

    // Also focus after a short timeout to catch any async mounting/rendering races
    const timer = setTimeout(() => {
      if (inputRef.current) {
        inputRef.current.focus();
        inputRef.current.select();
      }
      focusInputUntilStuck();
    }, 50);

    return () => {
      cancelAnimationFrame(raf);
      clearTimeout(timer);
    };
  }, [visible, focusInputUntilStuck]);

  // Click-outside handler (non-overlay only — overlay uses backdrop click)
  useEffect(() => {
    if (overlayMode || !visible) return;
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        close();
      }
    };
    document.addEventListener("mousedown", handler, true);
    return () => document.removeEventListener("mousedown", handler, true);
  }, [visible, close, overlayMode]);

  // Search logic
  //
  // Two queries in parallel so apps ALWAYS appear at the top, regardless
  // of Everything's default alphabetical sort. A single query for
  // "brave*" can return 20 results that are all "brave-icon.png" /
  // "brave-update.log" (alphabetically before Brave.lnk), pushing the
  // actual app off the visible list. The first query restricts to app
  // extensions (.exe / .lnk / .msi / .appx / .msix); the second is the
  // general query for everything else, deduped by full path.
  //
  // The case-sensitive toggle is intentionally ignored: Everything's
  // case-sensitive mode hides apps with capitalised names when the user
  // types lowercase ("brave" misses "Brave.lnk"), which would defeat
  // the priority below. Toggle UI is preserved but does nothing here.
  const performSearch = useCallback(async (q: string, _cs: boolean) => {
    if (!q.trim()) {
      setResults([]);
      setError(null);
      setSelectedIndex(0);
      return;
    }
    setIsSearching(true);
    try {
      const normalized = normalizeSearchQuery(q.trim());
      const queryLower = q.trim().toLowerCase();
      const appsQuery = `${normalized} ext:exe;lnk;msi;appx;msix`;

      const [appsResp, allResp] = await Promise.all([
        invoke<SearchResponse>("search_everything", {
          query: appsQuery,
          maxResults: 30,
        }).catch(() => ({ results: [], total: 0, query: appsQuery } as SearchResponse)),
        invoke<SearchResponse>("search_everything", {
          query: normalized,
          maxResults: 50,
        }),
      ]);

      const sortByScore = (a: SearchResult, b: SearchResult) => {
        const sd = appSortScore(a, queryLower) - appSortScore(b, queryLower);
        if (sd !== 0) return sd;
        return a.name.length - b.name.length;
      };

      const seen = new Set<string>();
      const merged: SearchResult[] = [];

      // Built-in Windows apps (File Explorer, Settings, etc.) always lead
      // when their keywords match — these aren't always indexed by Everything.
      for (const r of builtinMatches(queryLower)) {
        if (!seen.has(r.full_path)) { seen.add(r.full_path); merged.push(r); }
      }
      for (const r of [...appsResp.results].sort(sortByScore)) {
        if (!seen.has(r.full_path)) { seen.add(r.full_path); merged.push(r); }
      }
      for (const r of [...allResp.results].sort(sortByScore)) {
        if (!seen.has(r.full_path)) { seen.add(r.full_path); merged.push(r); }
      }

      setResults(merged.slice(0, 10));
      setSelectedIndex(0);
      setError(null);
    } catch (err: any) {
      setError(String(err));
      setResults([]);
    } finally {
      setIsSearching(false);
    }
  }, []);

  // Best-effort content search alongside the filename search.
  // Debounced (~275ms) and gated at 2+ chars so it doesn't fire invoke() on
  // every keystroke. Silent on error — the bar must not break if the search
  // engine isn't ready.
  useEffect(() => {
    if (contentDebounceRef.current) clearTimeout(contentDebounceRef.current);
    const trimmed = query.trim();
    if (trimmed.length < 2) { setContentRows([]); return; }
    let cancelled = false;
    contentDebounceRef.current = setTimeout(() => {
      invoke<ContentHit[]>("search_content", buildContentQueryArgs(query, 5) as unknown as Record<string, unknown>)
        .then((hits) => { if (!cancelled) setContentRows(hits.map(contentHitToDisplayRow)); })
        .catch(() => { /* best-effort */ });
    }, 275);
    return () => {
      cancelled = true;
      if (contentDebounceRef.current) clearTimeout(contentDebounceRef.current);
    };
  }, [query]);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    setQuery(val);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => performSearch(val, caseSensitive), 200);
  }, [performSearch, caseSensitive]);

  // Re-search when case-sensitive toggle changes (if there's already a query)
  useEffect(() => {
    if (query.trim()) performSearch(query, caseSensitive);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [caseSensitive]);

  const openResult = useCallback(async (result: SearchResult) => {
    try {
      await invoke("open_path", { path: result.full_path });
    } catch { }
    close();
  }, [close]);

  const openPathFromMenu = useCallback(async (path: string) => {
    await invoke("open_path", { path });
    close();
  }, [close]);

  const { target: contextTarget, openMenu, closeMenu, runAction } = useSearchResultContextMenu({
    openPath: openPathFromMenu,
    closeSearch: close,
    reportError: setError,
  });

  // Same file matched by name and content lists once — filename row wins.
  const dedupedContentRows = useMemo(
    () => dedupeContentRows(contentRows, results.map((r) => r.full_path)),
    [contentRows, results],
  );

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") { close(); return; }
    const totalRows = results.length + dedupedContentRows.length;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex(i => (totalRows > 0 ? Math.min(i + 1, totalRows - 1) : i));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(i => Math.max(i - 1, 0));
      return;
    }
    if (e.key === "Enter") {
      // Unlock/lock keywords always take priority over row activation.
      const cmd = query.trim().toLowerCase();
      if (cmd === unlockKeywordRef.current || cmd === lockKeywordRef.current) {
        e.preventDefault();
        e.stopPropagation();
        const eventName = cmd === unlockKeywordRef.current ? "hidden-panels-unlock" : "hidden-panels-lock";
        window.dispatchEvent(new Event(eventName));
        emit(eventName).catch(() => {});
        close();
        return;
      }
      if (selectedIndex < results.length) {
        if (results[selectedIndex]) openResult(results[selectedIndex]);
      } else {
        // Content row: open path directly then close.
        const contentIdx = selectedIndex - results.length;
        const row = dedupedContentRows[contentIdx];
        if (row) { invoke("open_path", { path: row.path }).catch(() => {}); close(); }
      }
      return;
    }
  }, [close, query, results, dedupedContentRows, selectedIndex, openResult]);

  const isDir = (r: SearchResult) => r.size === "0" || r.size === "";

  // ── Search card (shared between both modes) ──
  // onMouseDown re-asserts focus on the input — if the OS race left the
  // caret invisible (window.set_focus didn't promote the webview to
  // foreground), a single click anywhere on the card recovers it.
  const card = (
    <div
      ref={containerRef}
      className="esb-container"
      onClick={e => e.stopPropagation()}
      onMouseDown={() => focusInputUntilStuck()}
      onContextMenu={e => e.preventDefault()}
    >
      <div className="esb-input-row">
        <div className="esb-search-tile">
          <Icon icon="search" size={14} className="esb-search-icon" />
        </div>
        <input
          key={inputKey}
          ref={inputRef}
          className="esb-input"
          placeholder="Search files and contents…"
          value={query}
          onChange={handleChange}
          onKeyDown={handleKeyDown}
          autoComplete="off"
          spellCheck={false}
          autoFocus
        />
        {isSearching && <Spinner size={14} className="esb-spinner" />}
        {!isSearching && query && (
          <button
            className="esb-clear"
            onClick={() => { setQuery(""); setResults([]); inputRef.current?.focus(); }}
            tabIndex={-1}
          >
            <Icon icon="cross" size={12} />
          </button>
        )}
      </div>

      <div className="esb-hint">
        <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
        <span><kbd>↵</kbd> open</span>
        <span><kbd>Esc</kbd> close</span>
        <span className="esb-hint-sep" />
        <button
          className={`esb-case-toggle${caseSensitive ? " active" : ""}`}
          onClick={() => setCaseSensitive(v => !v)}
          title="Toggle case-sensitive search"
          tabIndex={-1}
        >
          Aa
        </button>
      </div>

      {error && (() => {
        // Detect the "search engine not installed / not running" error
        // so we can show a clean message instead of the raw Rust error
        // string (which used to leak the underlying tool's name to the
        // UI). Anything else is shown verbatim — those are usually real
        // bugs the user should see.
        const lower = error.toLowerCase();
        const engineMissing = lower.includes("search engine not installed")
          || lower.includes("search engine service is not running")
          || lower.includes("not found")
          || lower.includes("ipc not found");
        if (engineMissing) {
          return (
            <div className="esb-error">
              <Icon icon="warning-sign" size={12} />
              <span>Search engine not available. Install it from the Packages panel to enable file search.</span>
            </div>
          );
        }
        return (
          <div className="esb-error">
            <Icon icon="warning-sign" size={12} />
            <span>{error}</span>
          </div>
        );
      })()}

      <AnimatePresence initial={false}>
        {results.length > 0 && (
          <motion.div
            className="esb-results"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.12 }}
          >
            {results.map((r, i) => (
              <div
                key={r.full_path}
                className={`esb-result-item${i === selectedIndex ? " esb-selected" : ""}`}
                onMouseEnter={() => setSelectedIndex(i)}
                onClick={() => openResult(r)}
                onContextMenu={(event) => openMenu(event, r.full_path, r.name)}
              >
                <NativeSearchIcon result={r} />
                <div className="esb-result-text">
                  <span className="esb-result-name">{r.name}</span>
                  <span className="esb-result-path">{r.directory}</span>
                </div>
                {!isDir(r) && r.size && r.size !== "0" && (
                  <span className="esb-result-size">
                    {(() => {
                      const n = parseInt(r.size, 10);
                      if (isNaN(n) || n === 0) return "";
                      if (n < 1024) return `${n}B`;
                      if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)}KB`;
                      if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)}MB`;
                      return `${(n / 1024 / 1024 / 1024).toFixed(2)}GB`;
                    })()}
                  </span>
                )}
              </div>
            ))}
          </motion.div>
        )}
      </AnimatePresence>

      {/* "Inside files" content results — best-effort, shown below filename results */}
      {dedupedContentRows.length > 0 && (
        <div className="esb-content-section">
          <div className="esb-content-divider">Inside files</div>
          {dedupedContentRows.map((row, ci) => {
            const globalIdx = results.length + ci;
            return (
              <div
                key={row.docId}
                className={`esb-content-item${globalIdx === selectedIndex ? " esb-selected" : ""}`}
                onMouseEnter={() => setSelectedIndex(globalIdx)}
                onClick={() => { invoke("open_path", { path: row.path }).catch(() => {}); close(); }}
                onContextMenu={(event) => openMenu(event, row.path, row.name)}
              >
                <NativeSearchIcon result={{ name: row.name, directory: "", full_path: row.path, size: "1", modified: "" }} />
                <div className="esb-content-text">
                  <span className="esb-content-name">{row.name}</span>
                  {isNameOnlyMatch(row) && (
                    <span className="esb-name-match-badge" title="Matches the file name">name</span>
                  )}
                  {/* Accessible snippet from pre-parsed segments — avoids dangerouslySetInnerHTML. */}
                  <span className="esb-content-snippet">
                    {/* last-resort: snippet segments are purely positional tokens; text can repeat */}
                    {row.snippetSegs.map((seg, si) =>
                      seg.highlighted
                        // eslint-disable-next-line react/no-array-index-key
                        ? <mark key={`${seg.text}-${si}`}>{seg.text}</mark>
                        // eslint-disable-next-line react/no-array-index-key
                        : <span key={`${seg.text}-${si}`}>{seg.text}</span>
                    )}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {!isSearching && query.trim() && results.length === 0 && dedupedContentRows.length === 0 && !error && (
        <div className="esb-no-results">No files found</div>
      )}

      {contextTarget && (
        <SearchResultContextMenu
          target={contextTarget}
          onAction={runAction}
          onClose={closeMenu}
        />
      )}
    </div>
  );

  // ── OVERLAY MODE: fullscreen transparent backdrop, card centered ──
  if (overlayMode) {
    if (!visible) return null;
    return (
      <div
        className="esb-overlay-fullscreen"
        onMouseDown={e => { if (e.target === e.currentTarget) close(); }}
      >
        {card}
      </div>
    );
  }

  // ── NON-OVERLAY MODE: animated modal inside main app ──
  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          className="esb-backdrop"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
        >
          <motion.div
            className="esb-container-wrapper"
            initial={{ opacity: 0, scale: 0.95, y: -12 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.95, y: -8 }}
            transition={{ duration: 0.15, ease: [0.16, 1, 0.3, 1] }}
          >
            {card}
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
