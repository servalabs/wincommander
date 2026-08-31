// SPDX-License-Identifier: AGPL-3.0-or-later
// Normalises the standalone Windows location grammar accepted by quick search.

export interface SearchStorageLocation {
  /** Absolute Windows path passed to Everything's `-path` flag. */
  path: string;
  /** Exact user text restored when Backspace demotes the storage chip. */
  source: string;
}

export interface KnownSearchFolder {
  label: string;
  path: string;
}

export interface KnownFolderScope {
  /** The typed search terms, with the trailing `in/on <folder>` removed. */
  query: string;
  /** The exact phrase the user wrote, retained for lossless Backspace undo. */
  source: string;
  folder: KnownSearchFolder;
}

function folderName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const segment = /[^\\/]+$/.exec(trimmed)?.[0];
  return segment && !/^[a-z]:$/i.test(segment) ? segment : path;
}

function folderAliases(folder: KnownSearchFolder): string[] {
  const names = [folder.label, folderName(folder.path)];
  // Windows names Downloads/Pictures/etc. are commonly spoken in singular.
  // Keep the original label as the display text but accept that natural form.
  for (const name of [...names]) {
    if (/s$/i.test(name)) names.push(name.slice(0, -1));
  }
  return names.flatMap((name) => [name, `${name} folder`]);
}

function folderForLabel(label: string, folders: readonly KnownSearchFolder[]): KnownSearchFolder | null {
  const needle = label.trim().toLocaleLowerCase();
  let match: KnownSearchFolder | null = null;
  for (const folder of folders) {
    if (!folderAliases(folder).some((alias) => alias.toLocaleLowerCase() === needle)) continue;
    // A duplicate folder name is not enough information to choose a scope.
    if (match && match.path !== folder.path) return null;
    match = folder;
  }
  return match;
}

/**
 * Folders inferred from recently opened files/folders. A file contributes its
 * containing folder, while an opened folder is preserved as-is when it has a
 * directory-like trailing separator. The caller merges these with the Windows
 * known folders before parsing natural language scopes.
 */
export function recentSearchFolders(paths: readonly string[]): KnownSearchFolder[] {
  const unique = new Map<string, KnownSearchFolder>();
  for (const path of paths) {
    const normalized = path.trim().replaceAll("/", "\\");
    if (!/^[a-z]:\\/i.test(normalized)) continue;
    const parent = normalized.endsWith("\\")
      ? normalized.replace(/\\+$/, "")
      : normalized.slice(0, normalized.lastIndexOf("\\"));
    if (!/^[a-z]:\\[^\\]+/i.test(parent)) continue;
    const key = parent.toLocaleLowerCase();
    if (!unique.has(key)) unique.set(key, { label: folderName(parent), path: parent });
  }
  return [...unique.values()];
}

/**
 * Recognise a trailing natural-language scope such as `budget in Downloads`.
 * Labels come from Windows' Known Folder API, rather than assuming that a
 * folder still lives under the user profile (it may have been moved to another
 * drive or redirected to OneDrive). The original `in` / `on` phrasing is kept
 * as chip source text, so undo restores exactly what was typed.
 */
export function parseKnownFolderScope(
  text: string,
  folders: readonly KnownSearchFolder[],
): KnownFolderScope | null {
  const match = /(?:^|\s)((?:in|on)\s+(.+?))\s*$/i.exec(text);
  if (!match) return null;
  const label = match[2].trim();
  const location = parseSearchStorageLocation(label);
  const folder = location
    ? { label: folderName(location.path), path: location.path }
    : folderForLabel(label, folders);
  if (!folder) return null;
  return {
    query: text.slice(0, match.index).trimEnd(),
    source: match[1],
    folder,
  };
}

/**
 * Recognise a complete standalone storage location. `D:` and the reverse
 * shorthand `:D` both mean the D drive; absolute paths retain their full path.
 * The whole input must be a location, so paths with spaces work while a
 * location embedded in a natural-language search stays ordinary query text.
 */
export function parseSearchStorageLocation(text: string): SearchStorageLocation | null {
  const source = text.trim();
  const drive = /^([a-z]):$/i.exec(source)
    ?? /^:([a-z])$/i.exec(source)
    ?? /^([a-z])\s+(?:drive|disk)$/i.exec(source);
  if (drive) return { path: `${drive[1].toUpperCase()}:\\`, source };

  const absolutePath = /^([a-z]):([\\/].*)$/i.exec(source);
  if (!absolutePath) return null;
  return {
    path: `${absolutePath[1].toUpperCase()}:${absolutePath[2].replaceAll("/", "\\")}`,
    source,
  };
}
