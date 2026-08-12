// SPDX-License-Identifier: AGPL-3.0-or-later
// Fetches the native Windows shell icon (data URL) for a file path via
// `get_file_icon_data`, cached per path for the app session.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

const iconCache = new Map<string, string | null>();

/** Returns a data-URL icon for `path`, or null while loading / when the
 *  shell has none (callers render their fallback icon). `initial` is an
 *  icon already delivered with the search result, if any. */
export function useFileIconData(path: string, isDir: boolean, initial?: string | null): string | null {
  const [iconData, setIconData] = useState<string | null>(() => initial ?? iconCache.get(path) ?? null);

  useEffect(() => {
    let cancelled = false;
    if (isDir) return;
    if (initial) {
      iconCache.set(path, initial);
      setIconData(initial);
      return;
    }
    if (iconCache.has(path)) {
      setIconData(iconCache.get(path) ?? null);
      return;
    }
    invoke<string | null>("get_file_icon_data", { path })
      .then(data => {
        iconCache.set(path, data);
        if (!cancelled) setIconData(data);
      })
      .catch(() => {
        iconCache.set(path, null);
        if (!cancelled) setIconData(null);
      });
    return () => { cancelled = true; };
  }, [isDir, path, initial]);

  return iconData;
}
