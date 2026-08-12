// src/components/dashboard/RecentDownloadsCard.tsx
//
// Dashboard card showing the most recent files in the user's Downloads
// folder. Seeds from `get_recent_downloads` on mount, then live-refreshes
// whenever the Rust watcher emits `downloads://changed`.
//
// Clicking a row reveals the file in Explorer; the footer opens the
// Downloads folder. The list is read-only — no file contents cross IPC,
// only name / size / mtime metadata.

import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Download, ChevronUp, ChevronDown } from "lucide-react";

interface DownloadEntry {
  name: string;
  path: string;
  sizeBytes: number;
  modifiedAt: number; // epoch seconds
}

const MAX_SHOWN = 5;

// Module-level cache of the last fetched downloads so remounting the dashboard
// shows the previous entries instantly instead of flashing empty while the
// backend re-scans. Kept in sync with state on every refresh + change event.
let cachedDownloads: DownloadEntry[] = [];

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

function formatRelative(epochSecs: number): string {
  if (!epochSecs) return "";
  const deltaSec = Math.max(0, Math.floor(Date.now() / 1000 - epochSecs));
  if (deltaSec < 60) return "just now";
  const min = Math.floor(deltaSec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  return `${days}d ago`;
}

function parentDir(path: string): string {
  const idx = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return idx > 0 ? path.slice(0, idx) : path;
}

export default function RecentDownloadsCard() {
  // Seed from the module-level cache so a revisit renders immediately.
  const [downloads, setDownloads] = useState<DownloadEntry[]>(cachedDownloads);
  const [expanded, setExpanded] = useState(true);

  const refresh = useCallback(() => {
    invoke<DownloadEntry[]>("get_recent_downloads")
      .then((entries) => {
        cachedDownloads = entries;
        setDownloads(entries);
      })
      .catch(() => { /* watcher idle / no folder — leave empty */ });
  }, []);

  useEffect(() => {
    refresh();
    const unlistenP = listen("downloads://changed", refresh);
    return () => { unlistenP.then((un) => un()).catch(() => {}); };
  }, [refresh]);

  const openFolder = useCallback((path: string) => {
    invoke("open_path", { path }).catch(() => {});
  }, []);

  const sevenDaysAgoSecs = Math.floor(Date.now() / 1000) - 7 * 24 * 60 * 60;
  const shown = downloads
    .filter((d) => d.name.toLowerCase() !== "desktop.ini" && d.modifiedAt > sevenDaysAgoSecs)
    .slice(0, MAX_SHOWN);

  return (
    <div className="downloads-card">
      <button
        type="button"
        className="downloads-card-header downloads-card-toggle"
        onClick={() => setExpanded((o) => !o)}
        aria-expanded={expanded}
      >
        <Download size={12} />
        <span>RECENT DOWNLOADS</span>
        {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
      </button>

      {expanded && (<>
      {shown.length === 0 ? (
        <div className="downloads-empty">No downloads detected yet</div>
      ) : (
        <div className="downloads-list">
          {shown.map((d) => (
            <button
              key={d.path}
              type="button"
              className="downloads-row"
              onClick={() => openFolder(parentDir(d.path))}
              title={`${d.path}\nClick to open containing folder`}
            >
              <span className="downloads-name">{d.name}</span>
              <span className="downloads-meta">
                {formatSize(d.sizeBytes)}
                {d.modifiedAt ? ` · ${formatRelative(d.modifiedAt)}` : ""}
              </span>
            </button>
          ))}
        </div>
      )}

      <button
        type="button"
        className="downloads-footer"
        onClick={() => openFolder(shown.length > 0 ? parentDir(shown[0].path) : "shell:Downloads")}
      >
        Open Downloads folder →
      </button>
      </>)}
    </div>
  );
}
