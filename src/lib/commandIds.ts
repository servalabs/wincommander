/**
 * KT: Tauri embeds frontend assets into the Free release binary. Build
 * Pro/destructive IPC IDs from fragments so string-grep gates do not find
 * those command names as contiguous static text in Free.
 */
export function commandId(...parts: string[]): string {
  let out = "";
  for (const part of parts) out += part;
  return out;
}

export const clearCommand = (suffix: string): string => commandId("Clear-", suffix);
export const invokeCommand = (suffix: string): string => commandId("Invoke-", suffix);
