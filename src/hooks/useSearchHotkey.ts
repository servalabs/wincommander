// SPDX-License-Identifier: AGPL-3.0-or-later
// Quick-search (Ctrl+Space overlay) hotkey configuration — reads/persists
// `app.searchHotkey` and re-registers the global shortcut on change.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppState } from "../context/AppContext";

export interface SearchHotkeyState {
  hotkey: string;
  recording: boolean;
  startRecording: () => void;
  stopRecording: () => void;
  onRecordKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void;
}

export function useSearchHotkey(): SearchHotkeyState {
  const { appSettings, patchAppSettings } = useAppState();
  const [hotkey, setHotkey] = useState<string>(appSettings?.app?.searchHotkey ?? "Ctrl+Space");
  const [recording, setRecording] = useState(false);

  useEffect(() => {
    const saved = appSettings?.app?.searchHotkey;
    if (saved) {
      setHotkey(saved);
      invoke("update_search_hotkey", { hotkey: saved }).catch(console.error);
    }
  }, [appSettings?.app?.searchHotkey]);

  const onRecordKeyDown = useCallback((e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    e.stopPropagation();
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("Ctrl");
    if (e.shiftKey) parts.push("Shift");
    if (e.altKey) parts.push("Alt");
    const key = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
    parts.push(key);
    const combo = parts.join("+");
    setRecording(false);
    setHotkey(combo);
    // Same shape the panel always sent — the settings patch type is wider
    // than this partial, matching the original call site.
    patchAppSettings({ app: { searchHotkey: combo } } as Parameters<typeof patchAppSettings>[0]).catch(console.error);
    invoke("update_search_hotkey", { hotkey: combo }).catch(console.error);
  }, [patchAppSettings]);

  return {
    hotkey,
    recording,
    startRecording: useCallback(() => setRecording(true), []),
    stopRecording: useCallback(() => setRecording(false), []),
    onRecordKeyDown,
  };
}
