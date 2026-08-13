// src/components/EverythingSearchBar.tsx
//
// EverythingSearchBar — a Listary-style floating search bar.
// Two modes:
//   overlayMode=false (default): renders inside the main app as a modal-style overlay,
//                                toggled by the "toggle-search-bar" Tauri event.
//   overlayMode=true:            renders as the sole content of the dedicated
//                                "search-overlay" transparent Tauri window.
//                                The window is shown/hidden by the global hotkey in lib.rs.
//
// The query is a QueryState (chips + text), not a string. Chips are pills that
// sit immediately LEFT of the caret, Gmail-recipient style — the spatial link
// between the word the user typed and the pill that replaced it is what makes
// Backspace read as undo instead of as something random. All state and IPC live
// in useChipSearch; this file renders and translates keystrokes.
//
// The window lifecycle below (focusInputUntilStuck, the tauri://focus + blur
// listeners, the 600ms blur guard) encodes real Windows foreground-activation
// bugs. Do not simplify it.

import { useState, useCallback, useEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Icon, Spinner } from "@/components/ui/bp";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { AnimatePresence, motion } from "framer-motion";
import type { AppSettings } from "../types/settings";
import { dedupeContentRows, isNameOnlyMatch } from "@/lib/contentSearch";
import { formatResultSize, isDirectoryResult, isEngineMissingError, sfExtOf } from "@/lib/fileNameSearch";
import type { SearchResult } from "@/lib/fileNameSearch";
import { recordOpen, topPaths } from "@/lib/frecency";
import { describeQuery } from "@/lib/searchQueryPlan";
import { parseKnownFolderScope, parseSearchStorageLocation, recentSearchFolders } from "@/lib/searchStorageLocation";
import { addChip, CHIP_DEFS, chipDef, cycleChipStrict, demoteLastChip, promoteChip, removeChipAt, suggestChip } from "@/lib/searchTokens";
import type { Chip, ChipKind, QueryState } from "@/lib/searchTokens";
import { useChipSearch, useReducedMotionPref } from "@/hooks/useChipSearch";
import { useContentPreview } from "@/hooks/useContentPreview";
import type { BrowseResult } from "@/hooks/useChipSearch";
import SearchResultContextMenu from "./SearchResultContextMenu";
import { useSearchResultContextMenu } from "@/hooks/useSearchResultContextMenu";
import "./EverythingSearchBar.css";

const esbIconCache = new Map<string, string | null>();
const SEARCH_FILES_HANDOFF_KEY = "wincommander.search-files-query";
const QUICK_RESULT_LIMIT = 300;

const TYPE_FILTERS: readonly { kind: ChipKind; label: string; icon: string }[] = [
  { kind: "pdf", label: "PDF", icon: "document" }, { kind: "excel", label: "Excel", icon: "document" },
  { kind: "word", label: "Word", icon: "document" }, { kind: "text", label: "Text", icon: "document" },
  { kind: "slides", label: "Slides", icon: "document" }, { kind: "images", label: "Images", icon: "media" },
  { kind: "videos", label: "Video", icon: "video" }, { kind: "audio", label: "Audio", icon: "music" },
  { kind: "archives", label: "Archives", icon: "compressed" }, { kind: "code", label: "Code", icon: "code" },
  { kind: "apps", label: "Apps", icon: "application" }, { kind: "folders", label: "Folders", icon: "folder-close" },
];
const STORAGE_ROOTS = ["C:\\", "D:\\", "E:\\"];
type KnownSearchFolder = { label: string; path: string };
type ResultTab = "files" | "contents";

function knownFolderIcon(label: string) {
  if (label === "Desktop") return "desktop" as const;
  if (label === "Downloads") return "import" as const;
  if (label === "Pictures") return "media" as const;
  if (label === "Videos") return "video" as const;
  return "document" as const;
}

function getFallbackSearchIcon(result: SearchResult) {
  if (isDirectoryResult(result)) return { icon: "folder-close" as const, className: "esb-result-icon esb-icon-folder" };
  const ext = sfExtOf(result.name);
  if (["exe", "msi", "appx", "appxbundle", "msix", "lnk"].includes(ext)) return { icon: "application" as const, className: "esb-result-icon esb-icon-app" };
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "heic"].includes(ext)) return { icon: "media" as const, className: "esb-result-icon esb-icon-image" };
  if (["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm"].includes(ext)) return { icon: "video" as const, className: "esb-result-icon esb-icon-video" };
  if (["mp3", "wav", "flac", "m4a", "ogg", "aac"].includes(ext)) return { icon: "music" as const, className: "esb-result-icon esb-icon-audio" };
  if (["zip", "rar", "7z", "tar", "gz", "iso", "cab"].includes(ext)) return { icon: "compressed" as const, className: "esb-result-icon esb-icon-archive" };
  if (["js", "ts", "tsx", "jsx", "py", "rs", "go", "java", "cpp", "cs", "html", "css", "json", "yml", "yaml", "ps1", "bat", "cmd"].includes(ext)) return { icon: "code" as const, className: "esb-result-icon esb-icon-code" };
  return { icon: "document" as const, className: "esb-result-icon esb-icon-doc" };
}

function NativeSearchIcon({ result }: { result: SearchResult }) {
  const isDir = isDirectoryResult(result);
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

// ── Chip presentation ────────────────────────────────────────────────────────
// A time chip's two states must be legible without a legend, so the LABEL says
// which one it is: "Today first" ranks, "Only today" removes. The strict form
// also swaps to the filter glyph, because that is what it is now doing.

function chipLabel(chip: Chip): string {
  const def = chipDef(chip.kind);
  if (chip.kind === "in") return `In ${chip.pathLabel ?? chip.path ?? "folder"}`;
  if (!def.supportsStrict) return def.label;
  return chip.strict ? `Only ${def.label.toLowerCase()}` : `${def.label} first`;
}

function chipIconName(chip: Chip): string {
  const def = chipDef(chip.kind);
  return def.supportsStrict && chip.strict === true ? "filter" : def.icon;
}

function chipAriaLabel(chip: Chip): string {
  const def = chipDef(chip.kind);
  if (!def.supportsStrict) return `${chipLabel(chip)} filter. Activate to remove.`;
  return chip.strict === true
    ? `${def.label}: filtering to that date range only. Activate to remove.`
    : `${def.label}: ranking those first, nothing removed. Activate to filter instead.`;
}

export default function EverythingSearchBar({ overlayMode = false }: { overlayMode?: boolean }) {
  // In overlay mode the Rust side shows/hides the window — always render.
  // In normal mode we gate rendering via `visible`.
  const [visible, setVisible] = useState(overlayMode);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [inputKey, setInputKey] = useState(0);
  const [resultTab, setResultTab] = useState<ResultTab>("files");
  const [storageRoots, setStorageRoots] = useState<string[]>(STORAGE_ROOTS);
  const [knownFolders, setKnownFolders] = useState<KnownSearchFolder[]>([]);

  const search = useChipSearch(visible);
  const {
    query, setQuery, suggestion,
    primary, contentRows, reset: resetSearch, setError,
  } = search;
  const reduceMotion = useReducedMotionPref();
  const contentPreview = useContentPreview(query.text);

  const inputRef = useRef<HTMLInputElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const focusPollRef = useRef<number | null>(null);
  const lastShowTimeRef = useRef(0);
  const unlockKeywordRef = useRef("unlock");
  const lockKeywordRef = useRef("lock");
  // Caret position to apply once React has written the demoted text.
  const pendingCaretRef = useRef<number | null>(null);
  const skipStoragePromotionRef = useRef<string | null>(null);
  // Natural-language storage scopes are promoted while the user types. The
  // next Backspace is a true undo of that promotion; later edits are ordinary
  // query editing and must not resurrect a scope the user has removed.
  const autoStorageUndoRef = useRef<string | null>(null);
  const queryTextRef = useRef("");
  // Escape is handled by a window-level CAPTURE listener that cannot read state
  // directly, so the current chip count is mirrored here for it.
  const chipCountRef = useRef(0);
  useEffect(() => { chipCountRef.current = query.chips.length; }, [query.chips.length]);
  useEffect(() => { queryTextRef.current = query.text; }, [query.text]);
  useEffect(() => {
    const location = parseSearchStorageLocation(query.text);
    if (!location) return;
    if (skipStoragePromotionRef.current === query.text) {
      skipStoragePromotionRef.current = null;
      return;
    }
    // Wait for a pause so typing `D:\\Projects` remains one location instead
    // of turning `D:` into a chip before the user can enter the rest of it.
    const timer = window.setTimeout(() => {
      setSelectedIndex(0);
      setQuery((previous) => {
        const current = parseSearchStorageLocation(previous.text);
        if (!current) return previous;
        return addChip(
          { chips: previous.chips.filter((chip) => chip.kind !== "in"), text: "" },
          "in",
          { path: current.path, pathLabel: current.path, source: current.source },
        );
      });
    }, 180);
    return () => window.clearTimeout(timer);
  }, [query.text, setQuery]);
  useEffect(() => {
    if (!visible) return;
    invoke<string[]>("list_search_storage_roots").then((roots) => {
      if (roots.length > 0) setStorageRoots(roots);
    }).catch(() => {});
    invoke<KnownSearchFolder[]>("list_search_known_folders").then(setKnownFolders).catch(() => {});
  }, [visible]);

  // KT: depend on `resetSearch` (a stable useCallback), never on the whole
  // `search` object — that gets a new identity every render, and resetState
  // feeds the overlay-focus effect's dep list. A per-render identity there
  // re-registers the tauri listeners and re-triggers the focus poll forever.
  const resetState = useCallback(() => {
    resetSearch();
    setSelectedIndex(0);
    setInputKey(k => k + 1);
    autoStorageUndoRef.current = null;
  }, [resetSearch]);

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
      // A result context menu owns focus while it is open (especially its
      // inline Rename input). End any foreground-promotion retry that began
      // on the result row's preceding right-click instead of stealing the
      // caret back into the search field.
      if (document.querySelector(".esb-context-menu")) return true;
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

  // Escape is STAGED: the first press drops the chips, the second closes the
  // bar. A query the user spent time assembling must never die to one keystroke.
  // Returns true when it consumed the press.
  const handleEscape = useCallback((): boolean => {
    if (chipCountRef.current === 0) return false;
    setSelectedIndex(0);
    setQuery(prev => ({ chips: [], text: prev.text }));
    return true;
  }, [setQuery]);

  useEffect(() => {
    if (!overlayMode) return;
    const onWindowKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (!handleEscape()) close();
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
  }, [overlayMode, close, handleEscape]);

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

    // A second Ctrl+Space press asks Rust to open the complete Search Files
    // panel. A Tauri acknowledgement carries the exact text to the main
    // WebView; localStorage remains a fallback for the in-window "View all"
    // button and for a panel that mounts after navigation.
    let unlistenHandoff: (() => void) | null = null;
    win.listen("handoff-search-query", () => {
      const text = queryTextRef.current.trim();
      if (text) window.localStorage.setItem(SEARCH_FILES_HANDOFF_KEY, text);
      // Rust registers its one-shot listener *before* requesting this event.
      // Do not rely on a fixed timeout here: on a busy WebView the old 40 ms
      // delay could hide the overlay and change panels before this renderer
      // had a chance to write the query.
      emit("search-query-handoff-ready", { query: text }).catch(() => {
        // The localStorage fallback still covers in-window navigation if a
        // native event cannot be delivered during shutdown.
      });
    }).then(fn => { unlistenHandoff = fn; });

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
      unlistenHandoff?.();
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

  // Backspace-demote restores the chip's source word; the caret belongs at the
  // END of it so a second Backspace edits that word instead of eating the next
  // chip. React has not written the new value during the keydown, so the caret
  // move waits for this post-commit pass.
  useEffect(() => {
    const pos = pendingCaretRef.current;
    if (pos === null) return;
    pendingCaretRef.current = null;
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.setSelectionRange(pos, pos);
  }, [query]);

  const applyQuery = useCallback((next: QueryState) => {
    setSelectedIndex(0);
    setQuery(next);
  }, [setQuery]);

  const openPath = useCallback(async (path: string, isFolder = false) => {
    try {
      await invoke("open_path", { path });
      // Only real activations feed the frecency ranking. Recording a failed
      // open would teach the launcher to keep offering a file that cannot be
      // opened, which is exactly backwards.
      // Preserve the fact that this was a folder so it can be offered as a
      // recent storage scope rather than being mistaken for a file's parent.
      recordOpen(isFolder && !/[\\/]$/.test(path) ? `${path}\\` : path);
    } catch { /* the bar closes either way; a dead path is not worth a toast */ }
    close();
  }, [close]);

  const openPathFromMenu = useCallback(async (path: string) => {
    await invoke("open_path", { path });
    recordOpen(path);
    close();
  }, [close]);

  const { target: contextTarget, openMenu, closeMenu, runAction } = useSearchResultContextMenu({
    openPath: openPathFromMenu,
    closeSearch: close,
    reportError: setError,
  });

  // Same file matched by name and content lists once — filename row wins.
  const dedupedContentRows = useMemo(
    () => dedupeContentRows(contentRows, primary.map((r) => r.full_path)),
    [contentRows, primary],
  );

  const visibleRows = resultTab === "files" ? primary : dedupedContentRows;
  const visibleRowCount = visibleRows.length;
  const activeIndex = Math.min(selectedIndex, Math.max(0, visibleRowCount - 1));
  // The count request is intentionally best-effort. A full quick-search page
  // still means there may be more results even when that slower request timed
  // out, so never hide the complete-results escape hatch in that case.
  const hasMoreResults = search.totalCount !== null
    ? search.totalCount > primary.length
    : primary.length >= QUICK_RESULT_LIMIT;

  // The ghost is the ONE thing Tab acts on: a trailing-word chip candidate.
  // The shortcut launcher must not silently inherit Explorer's active folder:
  // showing an unexpected "In <folder>" pill looks like a hidden search scope
  // and makes a global search appear to be broken. Folder jumps remain explicit
  // through the documented ">folder" query syntax.
  const ghost = useMemo(() => {
    if (suggestion) {
      const def = chipDef(suggestion.chip.kind);
      return {
        icon: def.icon,
        label: def.supportsStrict ? `${def.label} first` : def.label,
      };
    }
    return null;
  }, [suggestion]);

  const promoteGhost = useCallback(() => {
    setSelectedIndex(0);
    if (suggestion) { setQuery(promoteChip(query, suggestion)); return; }
  }, [suggestion, query, setQuery]);

  const activeTypeChips = query.chips.filter((chip) => TYPE_FILTERS.some((type) => type.kind === chip.kind));
  const storageChip = query.chips.find((chip) => chip.kind === "in");
  const typeLabel = activeTypeChips.length ? activeTypeChips.map(chipLabel).join(" + ") : "All types";
  const storageLabel = storageChip?.pathLabel ?? "All drives";
  const recentFolders = useMemo(() => recentSearchFolders(topPaths(24)), []);
  const storageFolders = useMemo(() => {
    const paths = new Set<string>();
    return [...knownFolders, ...recentFolders].filter((folder) => {
      const key = folder.path.toLocaleLowerCase();
      if (paths.has(key)) return false;
      paths.add(key);
      return true;
    });
  }, [knownFolders, recentFolders]);

  const toggleType = useCallback((kind: ChipKind) => {
    setSelectedIndex(0);
    setQuery((previous) => {
      const existing = previous.chips.findIndex((chip) => chip.kind === kind);
      return existing >= 0 ? removeChipAt(previous, existing) : addChip(previous, kind);
    });
  }, [setQuery]);

  const selectStorage = useCallback((folder?: KnownSearchFolder) => {
    autoStorageUndoRef.current = null;
    setSelectedIndex(0);
    setQuery((previous) => {
      const withoutStorage = previous.chips.filter((chip) => chip.kind !== "in");
      if (!folder) return { chips: withoutStorage, text: previous.text };
      return addChip({ chips: withoutStorage, text: previous.text }, "in", {
        path: folder.path,
        pathLabel: folder.label,
        source: "",
      });
    });
  }, [setQuery]);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const text = e.target.value;
    // Once another input event has happened, the automatic promotion is no
    // longer the immediately preceding action, so Backspace edits normally.
    autoStorageUndoRef.current = null;
    setSelectedIndex(0);
    setQuery((previous) => {
      let next: QueryState = { chips: previous.chips, text };
      const folderScope = parseKnownFolderScope(next.text, storageFolders);
      if (folderScope) {
        autoStorageUndoRef.current = folderScope.source;
        next = addChip({ chips: next.chips.filter((chip) => chip.kind !== "in"), text: folderScope.query }, "in", {
          path: folderScope.folder.path,
          pathLabel: folderScope.folder.label,
          source: folderScope.source,
        });
      }
      const recognized = suggestChip(next);
      const isExactTypeWord = recognized && CHIP_DEFS.some((definition) => definition.kind === recognized.chip.kind && definition.triggers.some((trigger) => trigger === recognized.consumed.toLowerCase()));
      return recognized && isExactTypeWord
        ? promoteChip(next, recognized)
        : next;
    });
  }, [setQuery, storageFolders]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      if (!handleEscape()) close();
      return;
    }
    // Tab promotes the ghost — and ONLY when there is one, so Tab still reaches
    // the chips' own dismiss buttons when nothing is being offered.
    if (e.key === "Tab" && !e.shiftKey && !e.ctrlKey && !e.altKey && ghost) {
      e.preventDefault();
      promoteGhost();
      return;
    }
    if (e.key === "Backspace" && query.chips.length > 0) {
      const el = e.currentTarget;
      const immediatelyPromotedScope = autoStorageUndoRef.current;
      if (immediatelyPromotedScope) {
        e.preventDefault();
        autoStorageUndoRef.current = null;
        const text = query.text ? `${query.text} ${immediatelyPromotedScope}` : immediatelyPromotedScope;
        skipStoragePromotionRef.current = text;
        pendingCaretRef.current = text.length;
        applyQuery({ chips: query.chips.filter((chip) => chip.kind !== "in"), text });
        return;
      }
      const immediatelyAfterPromotion = el.selectionStart === query.text.length && el.selectionEnd === query.text.length && /\s$/.test(query.text);
      if ((el.selectionStart === 0 && el.selectionEnd === 0) || immediatelyAfterPromotion) {
        const last = query.chips[query.chips.length - 1];
        // A scope removed after the user has resumed editing is a filter
        // removal, not an undo: leave their remaining query exactly as-is.
        const next = last?.kind === "in"
          ? { chips: query.chips.slice(0, -1), text: query.text }
          : demoteLastChip(query);
        if (next) {
          e.preventDefault();
          skipStoragePromotionRef.current = next.text;
          pendingCaretRef.current = next.text.length;
          applyQuery(next);
        }
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex(visibleRowCount > 0 ? Math.min(activeIndex + 1, visibleRowCount - 1) : 0);
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex(Math.max(activeIndex - 1, 0));
      return;
    }
    if (e.key === "Enter") {
      // Unlock/lock keywords always take priority over row activation.
      const cmd = query.text.trim().toLowerCase();
      if (cmd === unlockKeywordRef.current || cmd === lockKeywordRef.current) {
        e.preventDefault();
        e.stopPropagation();
        const eventName = cmd === unlockKeywordRef.current ? "hidden-panels-unlock" : "hidden-panels-lock";
        window.dispatchEvent(new Event(eventName));
        emit(eventName).catch(() => {});
        close();
        return;
      }
      const fileResult = resultTab === "files" ? primary[activeIndex] : undefined;
      const row = fileResult?.full_path ?? (resultTab === "contents" ? dedupedContentRows[activeIndex]?.path : undefined);
      if (row) void openPath(row, fileResult ? isDirectoryResult(fileResult) : false);
      return;
    }
  }, [close, handleEscape, ghost, promoteGhost, query, applyQuery, visibleRowCount, activeIndex, primary, dedupedContentRows, openPath, resultTab]);

  const renderChip = (chip: Chip, index: number) => {
    const label = chipLabel(chip);
    const aria = chipAriaLabel(chip);
    const cls = `esb-chip${chip.strict === true ? " esb-chip-strict" : ""}`;
    // Time chips carry two actions (cycle + remove) so they need two buttons;
    // every other chip is one button whose only job is to go away.
    if (!chipDef(chip.kind).supportsStrict) {
      return (
        <button type="button" className={`${cls} esb-chip-solo`} onClick={() => applyQuery(removeChipAt(query, index))} aria-label={aria} title={aria}>
          <Icon icon={chipIconName(chip)} size={12} className="esb-chip-icon" />
          <span className="esb-chip-label">{label}</span>
          <span className="esb-chip-x" aria-hidden="true"><Icon icon="cross" size={9} /></span>
        </button>
      );
    }
    return (
      <span className={cls}>
        <button type="button" className="esb-chip-main" onClick={() => applyQuery(cycleChipStrict(query, index))} aria-label={aria} title={aria}>
          <Icon icon={chipIconName(chip)} size={12} className="esb-chip-icon" />
          <span className="esb-chip-label">{label}</span>
        </button>
        <button type="button" className="esb-chip-x esb-chip-x-btn" onClick={() => applyQuery(removeChipAt(query, index))} aria-label={`Remove the ${label} filter`} title="Remove">
          <Icon icon="cross" size={9} />
        </button>
      </span>
    );
  };

  const sectionLabel = search.isBrowse
    ? (query.chips.length > 0 ? describeQuery(query) : "Recent")
    : null;

  // Announced on CHIP changes only. handleChange reuses the previous `chips`
  // array by reference, so typing never invalidates this memo — re-reading the
  // whole query aloud on every keystroke would make the box unusable with a
  // screen reader, and the typed text is already announced by the input itself.
  const liveDescription = useMemo(
    () => describeQuery({ chips: query.chips, text: "" }),
    [query.chips],
  );

  // ── Search card (shared between both modes) ──
  // onMouseDown re-asserts focus on the input — if the OS race left the
  // caret invisible (window.set_focus didn't promote the webview to
  // foreground), a single click anywhere on the card recovers it.
  const card = (
    <div
      ref={containerRef}
      className="esb-container"
      onClick={e => e.stopPropagation()}
      onMouseDown={(event) => {
        // Buttons and the inline Rename field inside the context menu must be
        // allowed to retain focus. The retry loop also has the same guard for
        // a poll that started just before the menu mounted.
        if ((event.target as Element).closest(".esb-context-menu")) return;
        focusInputUntilStuck();
      }}
      onContextMenu={e => e.preventDefault()}
    >
      <div className="esb-input-row">
        <div className="esb-search-control">
        <div className="esb-search-tile">
          <Icon icon="search" size={14} className="esb-search-icon" />
        </div>

        <div className="esb-field">
          <AnimatePresence initial={false}>
            {query.chips.map((chip, index) => (
              !TYPE_FILTERS.some((type) => type.kind === chip.kind) && chip.kind !== "in" && (
              <motion.span
                key={chip.kind}
                className="esb-chip-wrap"
                initial={reduceMotion ? false : { opacity: 0, scale: 0.86 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={reduceMotion ? { opacity: 0 } : { opacity: 0, scale: 0.86 }}
                transition={{ duration: reduceMotion ? 0 : 0.14, ease: [0.16, 1, 0.3, 1] }}
              >
                {renderChip(chip, index)}
              </motion.span>
              )
            ))}
          </AnimatePresence>

          {ghost && (
            <button
              type="button"
              className="esb-ghost"
              onClick={promoteGhost}
              aria-label={`Add the ${ghost.label} filter. Press Tab.`}
              title={`Tab to filter by ${ghost.label}`}
            >
              <Icon icon={ghost.icon} size={12} className="esb-chip-icon" />
              <span className="esb-chip-label">{ghost.label}</span>
              <kbd className="esb-ghost-kbd" aria-hidden="true">Tab</kbd>
            </button>
          )}

          {/* Same scheme as the Search Files panel: focus never leaves the input,
              rows are role="option" with ids esb-opt-<flatIndex>, and the content
              rows continue that flat index from primary.length. */}
          <input
            key={inputKey}
            ref={inputRef}
            className="esb-input"
            placeholder={query.chips.length > 0 ? "Add a name…" : "Search files and contents…"}
            value={query.text}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            role="combobox"
            aria-expanded={visibleRowCount > 0}
            aria-controls={primary.length > 0 ? "esb-result-list" : undefined}
            aria-activedescendant={visibleRowCount > 0 ? `esb-opt-${activeIndex}` : undefined}
            autoComplete="off"
            spellCheck={false}
            autoFocus
          />
        </div>

        {search.isSearching && <Spinner size={14} className="esb-spinner" />}
        {!search.isSearching && (query.text || query.chips.length > 0) && (
          <button
            className="esb-clear"
            onClick={() => { applyQuery({ chips: [], text: "" }); inputRef.current?.focus(); }}
            aria-label="Clear the search"
            title="Clear"
            tabIndex={-1}
          >
            <Icon icon="cross" size={12} />
          </button>
        )}
        </div>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className="esb-filter-control" aria-label={`File type: ${typeLabel}`}>
              <Icon icon="filter" size={13} />
              <span className="esb-filter-control-label">Type</span>
              <strong>{typeLabel}</strong>
              <Icon icon="chevron-down" size={12} />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="esb-filter-menu">
            <DropdownMenuLabel>File type — choose any</DropdownMenuLabel>
            <DropdownMenuItem onSelect={() => setQuery((previous) => ({ chips: previous.chips.filter((chip) => !TYPE_FILTERS.some((type) => type.kind === chip.kind)), text: previous.text }))}>
              <Icon icon="th" /> All types
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {TYPE_FILTERS.map((type) => (
              <DropdownMenuCheckboxItem key={type.kind} checked={activeTypeChips.some((chip) => chip.kind === type.kind)} onCheckedChange={() => toggleType(type.kind)} onSelect={(event) => event.preventDefault()}>
                <span className={`esb-extension-icon esb-extension-icon--${type.kind}`} aria-hidden="true">{type.label.slice(0, 3).toUpperCase()}</span>{type.label}
              </DropdownMenuCheckboxItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className="esb-filter-control" aria-label={`Storage: ${storageLabel}`} title={storageChip?.path ? `Storage path: ${storageChip.path}` : undefined}>
              <Icon icon="hard-drive" size={13} />
              <span className="esb-filter-control-label">Storage</span>
              <strong>{storageLabel}</strong>
              <Icon icon="chevron-down" size={12} />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="esb-filter-menu">
            <DropdownMenuLabel>Storage</DropdownMenuLabel>
            <DropdownMenuItem onSelect={() => selectStorage()}><Icon icon="hard-drive" /> All drives</DropdownMenuItem>
            {knownFolders.length > 0 && <DropdownMenuSeparator />}
            {knownFolders.map((folder) => <DropdownMenuItem key={folder.path} onSelect={() => selectStorage(folder)} title={folder.path}><Icon icon={knownFolderIcon(folder.label)} /> {folder.label}</DropdownMenuItem>)}
            {recentFolders.length > 0 && <>
              <DropdownMenuSeparator />
              <DropdownMenuLabel>Recent folders</DropdownMenuLabel>
              {recentFolders.map((folder) => <DropdownMenuItem key={folder.path} onSelect={() => selectStorage(folder)} title={folder.path}><Icon icon="folder-open" /> {folder.label}</DropdownMenuItem>)}
            </>}
            <DropdownMenuSeparator />
            {storageRoots.map((path) => <DropdownMenuItem key={path} onSelect={() => selectStorage({ path, label: path.replace(/[\\/]+$/, "") })} title={path}><Icon icon="hard-drive" /> {path}</DropdownMenuItem>)}
          </DropdownMenuContent>
        </DropdownMenu>

      </div>

      {/* Announces the assembled query in plain English whenever chips change. */}
      <div className="esb-live" aria-live="polite">{liveDescription}</div>

      <div className="esb-hint">
        {search.isJump && (
          <span className="esb-hint-mode"><Icon icon="folder-open" size={11} /> jump into folder</span>
        )}
        <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
        <span><kbd>↵</kbd> open</span>
        <span><kbd>Tab</kbd> filter</span>
        <span><kbd>⌫</kbd> undo chip</span>
        <span><kbd>Esc</kbd> close</span>
      </div>

      {search.error && (
        <div className="esb-error">
          <Icon icon="warning-sign" size={12} />
          <span>
            {isEngineMissingError(search.error)
              ? "Search engine not available. Install it from the Packages panel to enable file search."
              : search.error}
          </span>
        </div>
      )}

      {sectionLabel && primary.length > 0 && (
        <div className="esb-section-label">{sectionLabel}</div>
      )}

      {(primary.length > 0 || dedupedContentRows.length > 0) && (
        <>
          <div className="esb-result-tabs" role="tablist" aria-label="Search result kind">
            <button type="button" role="tab" aria-selected={resultTab === "files"} className={resultTab === "files" ? "is-active" : ""} onClick={() => { setResultTab("files"); setSelectedIndex(0); }}>
              Files <span>{primary.length}</span>
            </button>
            <button type="button" role="tab" aria-selected={resultTab === "contents"} className={resultTab === "contents" ? "is-active" : ""} onClick={() => { setResultTab("contents"); setSelectedIndex(0); if (!contentPreview.row && dedupedContentRows[0]) contentPreview.select(dedupedContentRows[0]); }}>
              Inside files <span>{dedupedContentRows.length}</span>
            </button>
          </div>
          <div className={`esb-result-area${resultTab === "contents" ? " esb-result-area-preview" : ""}`}>
            <AnimatePresence initial={false} mode="wait">
              {resultTab === "files" && primary.length > 0 && (
                <motion.div id="esb-result-list" className="esb-results" role="listbox" aria-label="File name search results" initial={reduceMotion ? false : { opacity: 0 }} animate={{ opacity: 1 }} exit={reduceMotion ? { opacity: 0 } : { opacity: 0 }} transition={{ duration: reduceMotion ? 0 : 0.12 }}>
                  {primary.map((r: BrowseResult, i) => (
                    <div key={r.full_path} id={`esb-opt-${i}`} role="option" aria-selected={i === activeIndex} className={`esb-result-item${i === activeIndex ? " esb-selected" : ""}`} onMouseEnter={() => setSelectedIndex(i)} onClick={() => void openPath(r.full_path, isDirectoryResult(r))} onContextMenu={(event) => openMenu(event, r.full_path, r.name)}>
                      <NativeSearchIcon result={r} />
                      <div className="esb-result-text"><span className="esb-result-name">{r.name}</span><span className="esb-result-path">{r.directory}</span></div>
                      {!isDirectoryResult(r) && !r.synthetic && r.size && <span className="esb-result-size">{formatResultSize(r.size)}</span>}
                    </div>
                  ))}
                </motion.div>
              )}
              {resultTab === "contents" && dedupedContentRows.length > 0 && (
                <motion.div id="esb-result-list" className="esb-results esb-content-results" role="listbox" aria-label="Text inside file search results" initial={reduceMotion ? false : { opacity: 0 }} animate={{ opacity: 1 }} exit={reduceMotion ? { opacity: 0 } : { opacity: 0 }} transition={{ duration: reduceMotion ? 0 : 0.12 }}>
                  {dedupedContentRows.map((row, index) => (
                    <div key={row.docId} id={`esb-opt-${index}`} role="option" aria-selected={index === activeIndex} className={`esb-content-item${index === activeIndex ? " esb-selected" : ""}`} onMouseEnter={() => setSelectedIndex(index)} onClick={() => contentPreview.select(row)} onDoubleClick={() => void openPath(row.path)} onContextMenu={(event) => openMenu(event, row.path, row.name)}>
                      <NativeSearchIcon result={{ name: row.name, directory: "", full_path: row.path, size: "1", modified: "" }} />
                      <div className="esb-content-text"><span className="esb-content-name">{row.name}</span>{isNameOnlyMatch(row) && <span className="esb-name-match-badge" title="Matches the file name">name</span>}<span className="esb-content-snippet">{row.snippetSegs.map((seg) => seg.highlighted ? <mark key={`${row.docId}-${seg.highlighted}-${seg.text}`}>{seg.text}</mark> : <span key={`${row.docId}-${seg.highlighted}-${seg.text}`}>{seg.text}</span>)}</span></div>
                    </div>
                  ))}
                </motion.div>
              )}
            </AnimatePresence>
            {resultTab === "contents" && (
              <aside className="esb-preview" aria-live="polite">
                {contentPreview.row ? <>
                  <div className="esb-preview-heading"><Icon icon="document" size={14} /><strong>{contentPreview.row.name}</strong></div>
                  <div className="esb-preview-path">{contentPreview.row.path}</div>
                  <div className="esb-preview-copy">{contentPreview.isLoading ? <Spinner size={16} /> : (contentPreview.text || "No readable text is available for this file.")}</div>
                  <div className="esb-preview-actions">
                    <button type="button" onClick={() => void openPath(contentPreview.row!.path)}>Open</button>
                    <button type="button" onClick={() => void invoke("search_open_containing_folder", { path: contentPreview.row!.path }).catch((error) => setError(String(error)))}>Folder</button>
                    <button type="button" className="esb-icon-action" aria-label="Copy file path" title="Copy file path" onClick={() => void invoke("search_copy_path", { path: contentPreview.row!.path }).catch((error) => setError(String(error)))}><Icon icon="clipboard" size={14} /></button>
                  </div>
                </> : <div className="esb-preview-empty">Select a result to preview its matched text.</div>}
              </aside>
            )}
          </div>
        </>
      )}

      {hasMoreResults && (
        <div className="esb-count">
          <span>
            {search.totalCount === null
              ? `Showing the first ${primary.length.toLocaleString()} results`
              : `Showing ${primary.length} of ${search.totalCount.toLocaleString()}`}
          </span>
          <button
            type="button"
            className="esb-open-full-search"
            onClick={() => {
              // The panel mounts after navigation, so retain the handoff until
              // its first effect can consume it instead of racing an event.
              window.localStorage.setItem(SEARCH_FILES_HANDOFF_KEY, query.text);
              close();
              window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "search-files" }));
            }}
          >
            View all results
          </button>
        </div>
      )}

      {!search.isSearching && visibleRowCount === 0 && !search.error && (
        <div className="esb-no-results">
          <span className="esb-no-results-title">
            {search.isBrowse ? "Nothing recent to show" : "Nothing matched"}
          </span>
          {/* The query in plain English, so an empty list explains itself
              instead of leaving the user to guess which chip was too tight. */}
          <span className="esb-no-results-sub">{describeQuery(query)}</span>
        </div>
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
