// SPDX-License-Identifier: AGPL-3.0-or-later
// Fetches the native Windows shell icon (data URL) for a file path via
// `get_file_icon_data`, cached per path for the app session.

import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FileIconQueue } from "@/lib/fileIconQueue";

const iconQueue = new FileIconQueue((path) => invoke<string | null>("get_file_icon_data", { path }));

/** Returns a data-URL icon for `path`, or null while loading / when the
 *  shell has none (callers render their fallback icon). `initial` is an
 *  icon already delivered with the search result, if any. */
export function useFileIconData(
  path: string,
  isDir: boolean,
  initial?: string | null,
  enabled: boolean = true,
  priority: number = 0,
): string | null {
  const priorityRef = useRef(priority);
  priorityRef.current = priority;
  const [iconData, setIconData] = useState<string | null>(() => initial ?? iconQueue.get(path) ?? null);

  useEffect(() => {
    if (initial) iconQueue.prime(path, initial);
    const cached = initial ?? iconQueue.get(path) ?? null;
    setIconData(cached);
    if (isDir || initial || !enabled) return;
    if (iconQueue.get(path) !== undefined) {
      return;
    }
    return iconQueue.request(path, priorityRef.current, setIconData);
  }, [enabled, initial, isDir, path]);

  return iconData;
}
