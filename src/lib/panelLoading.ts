const CHUNK_LOAD_ERROR_PATTERNS = [
  /failed to fetch dynamically imported module/i,
  /error loading dynamically imported module/i,
  /failed to load module script/i,
  /loading chunk \S+ failed/i,
  /chunkloaderror/i,
  /importing a module script failed/i,
];

const VITE_HMR_RUNTIME_ERROR_PATTERNS = [
  /__vite__updateStyle is not defined/i,
  /__vite__removeStyle is not defined/i,
  /__vite__css is not defined/i,
  // KT: browsers cache failed ESM links, so recreating React.lazy cannot
  // recover after an HMR export change; the guarded page reload can.
  /the requested module ['"]\/src\/[^'"]+['"] does not provide an export named/i,
];

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  try {
    return JSON.stringify(error);
  } catch {
    return String(error);
  }
}

export function isChunkLoadError(error: unknown): boolean {
  const message = errorMessage(error);
  return CHUNK_LOAD_ERROR_PATTERNS.some((pattern) => pattern.test(message));
}

export function isViteHmrRuntimeError(error: unknown): boolean {
  const message = errorMessage(error);
  return VITE_HMR_RUNTIME_ERROR_PATTERNS.some((pattern) => pattern.test(message));
}

/** Errors that require a guarded full reload because retrying React.lazy is insufficient. */
export function isStalePanelLoadError(error: unknown): boolean {
  return isChunkLoadError(error) || isViteHmrRuntimeError(error);
}

/**
 * Retry only transient chunk-loading failures. Module evaluation and render
 * errors are deterministic and must reach the panel boundary immediately.
 */
export function importPanelWithRetry<T>(
  importFn: () => Promise<T>,
  retryDelayMs = 350,
): () => Promise<T> {
  return async () => {
    try {
      return await importFn();
    } catch (firstError) {
      if (!isChunkLoadError(firstError)) throw firstError;
      await new Promise<void>((resolve) => globalThis.setTimeout(resolve, retryDelayMs));
      return importFn();
    }
  };
}
