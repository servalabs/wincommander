import type { BackendResponse } from "../../hooks/useBackend";

type ShortcutResult = { status: string; itemsChanged: number; warnings?: string[] };
let shortcutQueue: Promise<void> = Promise.resolve();

export function shouldReconcileCalculatorShortcuts(input: {
  unlocked: boolean;
  decoy: boolean;
  hasRealPin: boolean;
  enabled: boolean;
  hideMode: boolean;
}): boolean {
  return input.unlocked && !input.decoy && input.hasRealPin && input.enabled && !input.hideMode;
}

export async function syncCalculatorShortcuts(
  update: (hidden: boolean) => Promise<BackendResponse<ShortcutResult>>,
  hidden: boolean,
): Promise<string | null> {
  const operation = shortcutQueue.then(async () => {
    try {
      const response = await update(hidden);
      if (!response.success) return response.error || "Could not update Calculator shortcuts.";
      if (!response.data || response.data.status === "error") return "Could not update Calculator shortcuts.";
      return response.data.warnings?.join(" ") || null;
    } catch (error) {
      return String(error);
    }
  });
  shortcutQueue = operation.then(() => undefined);
  return operation;
}
