// src/panels/search-files/FileIcon.tsx
//
// Real Windows shell icon for a search result, with a colour-coded
// per-filetype fallback while the native icon loads (or when it can't).
// Pure renderer — fetching/caching lives in useFileIconData.

import { Icon } from "@/components/ui/icon";
import { useFileIconData } from "@/hooks/useFileIconData";

export function getFallbackFileIcon(name: string, isDir: boolean) {
  if (isDir) return { icon: "folder-close", className: "sr-file-icon sr-icon-folder" };
  const ext = name.split(".").pop()?.toLowerCase() || "";
  if (["exe", "msi", "appx", "appxbundle", "msix", "lnk"].includes(ext)) return { icon: "application", className: "sr-file-icon sr-icon-app" };
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "heic"].includes(ext)) return { icon: "media", className: "sr-file-icon sr-icon-image" };
  if (["mp4", "mkv", "mov", "avi", "wmv", "flv", "webm"].includes(ext)) return { icon: "video", className: "sr-file-icon sr-icon-video" };
  if (["mp3", "wav", "flac", "m4a", "ogg", "aac"].includes(ext)) return { icon: "music", className: "sr-file-icon sr-icon-audio" };
  if (["zip", "rar", "7z", "tar", "gz", "iso", "cab"].includes(ext)) return { icon: "compressed", className: "sr-file-icon sr-icon-archive" };
  if (["js", "ts", "tsx", "jsx", "py", "rs", "go", "java", "cpp", "cs", "html", "css", "json", "yml", "yaml", "ps1", "bat", "cmd"].includes(ext)) return { icon: "code", className: "sr-file-icon sr-icon-code" };
  if (["xls", "xlsx", "csv"].includes(ext)) return { icon: "th", className: "sr-file-icon sr-icon-sheet" };
  return { icon: "document", className: "sr-file-icon sr-icon-doc" };
}

interface FileIconProps {
  path: string;
  name: string;
  isDir: boolean;
  /** Icon already delivered with the search result, if any. */
  iconData?: string | null;
  /** Stale search rows keep their fallback until their current query lands. */
  loadNativeIcon?: boolean;
  /** Lower values run first inside the shared native-icon queue. */
  priority?: number;
  size?: number;
}

export default function FileIcon({
  path,
  name,
  isDir,
  iconData: initial,
  loadNativeIcon = true,
  priority = 0,
  size = 16,
}: FileIconProps) {
  const iconData = useFileIconData(path, isDir, initial, loadNativeIcon, priority);
  const fallback = getFallbackFileIcon(name, isDir);

  if (iconData) return <img src={iconData} alt="" className="sr-native-icon" style={{ width: size, height: size }} />;
  return <Icon icon={fallback.icon} size={size} className={fallback.className} />;
}
