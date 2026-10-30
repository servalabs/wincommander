/** Convert a backend-supplied Windows path to the ordinary Explorer display form. */
export function toExplorerDisplayPath(path: string | null | undefined): string | null {
  if (!path || !path.trim()) return null;

  let displayPath = path.replace(/\//g, "\\");
  if (/^\\\\\?\\UNC\\/i.test(displayPath)) {
    displayPath = `\\\\${displayPath.slice(8)}`;
  } else if (/^\\\\\?\\[A-Za-z]:\\/i.test(displayPath)) {
    displayPath = displayPath.slice(4);
  } else if (/^\\\\\?\\/i.test(displayPath)) {
    // Keep volume/device namespaces unavailable until there is a verified,
    // Explorer-readable normalization for them.
    return null;
  }

  // '?' cannot appear in an ordinary Windows filename. Treat it, or a lossy
  // Unicode replacement marker, as undecodable instead of displaying a guess.
  if (displayPath.includes("?") || displayPath.includes("\uFFFD")) return null;
  return displayPath;
}
