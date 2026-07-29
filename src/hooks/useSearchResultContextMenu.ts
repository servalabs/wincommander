import { useCallback, useState, type MouseEvent as ReactMouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";

export type SearchContextAction =
  | "open"
  | "open-folder"
  | "copy"
  | "cut"
  | "copy-path"
  | "vscode"
  | "rename"
  | "properties"
  | "delete"
  | "shred";

export interface SearchContextTarget {
  path: string;
  label: string;
  canUseFileActions: boolean;
  x: number;
  y: number;
}

interface Options {
  openPath: (path: string) => Promise<void>;
  closeSearch: () => void;
  reportError: (message: string) => void;
}

export function useSearchResultContextMenu({ openPath, closeSearch, reportError }: Options) {
  const [target, setTarget] = useState<SearchContextTarget | null>(null);

  const openMenu = useCallback((event: ReactMouseEvent, path: string, label: string) => {
    event.preventDefault();
    event.stopPropagation();
    setTarget({
      path,
      label,
      canUseFileActions: /^[a-zA-Z]:\\|^\\\\/.test(path),
      x: Math.max(8, Math.min(event.clientX, window.innerWidth - 244)),
      // ~10 action rows + separator + label now render taller than the old menu;
      // the estimate just needs to keep the menu mostly on-screen — esb-context-menu's
      // max-height + overflow-y:auto (EverythingSearchBar.css) is the real safety net.
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - 420)),
    });
  }, []);

  const closeMenu = useCallback(() => setTarget(null), []);

  const runAction = useCallback(async (action: SearchContextAction, payload?: string) => {
    if (!target) return;
    try {
      switch (action) {
        case "open":
          await openPath(target.path);
          return;
        case "open-folder":
          await invoke("search_open_containing_folder", { path: target.path });
          break;
        case "copy":
          await invoke("search_set_file_clipboard", { path: target.path, cut: false });
          break;
        case "cut":
          await invoke("search_set_file_clipboard", { path: target.path, cut: true });
          break;
        case "copy-path":
          await invoke("search_copy_path", { path: target.path });
          break;
        case "vscode":
          await invoke("search_open_in_vscode", { path: target.path });
          break;
        case "rename":
          if (!payload) return;
          await invoke("search_rename_file", { path: target.path, newName: payload });
          break;
        case "properties":
          await invoke("search_show_properties", { path: target.path });
          break;
        case "delete":
          await invoke("search_delete_to_recycle_bin", { path: target.path });
          break;
        case "shred":
          // The search result itself is the explicit target selection. Route
          // it through the same guarded direct-shred backend as Explorer, so
          // secure erase starts now and never opens the general in-app modal.
          await invoke("search_shred_direct", { path: target.path });
          break;
      }
      closeSearch();
    } catch (error) {
      reportError(String(error));
    } finally {
      setTarget(null);
    }
  }, [closeSearch, openPath, reportError, target]);

  return { target, openMenu, closeMenu, runAction };
}
