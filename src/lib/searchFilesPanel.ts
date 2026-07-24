export function mergeIndexedRoots(currentRoots: string[], addedRoots: string[]): string[] {
  return Array.from(new Set([...currentRoots, ...addedRoots]));
}

export function removeIndexedRoot(currentRoots: string[], root: string): string[] {
  return currentRoots.filter((candidate) => candidate !== root);
}

export function getIndexDisplayError(lastError: string | null | undefined): string | null {
  if (!lastError) return null;
  if (lastError.startsWith("Extraction error:")) return null;
  return lastError;
}
