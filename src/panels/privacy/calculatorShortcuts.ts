import type { BackendResponse } from "../../hooks/useBackend";
import type { SharedShortcutManaged } from "../../hooks/sharedShortcutClaims";

type ShortcutResult = { status: string; itemsChanged: number; warnings?: string[] };
let shortcutQueue: Promise<void> = Promise.resolve();

export function shouldReconcileCalculatorShortcuts(input: {
  unlocked: boolean;
  decoy: boolean;
  hasRealPin: boolean;
  enabled: boolean;
  hideMode: boolean;
}): boolean {
  // Hide mode changes restoration ownership, not whether an armed lock needs
  // its Calculator shortcut. An unarmed lock must never restore at startup.
  return input.unlocked && !input.decoy && input.hasRealPin && input.enabled;
}

export async function syncCalculatorShortcuts(
  update: (hidden: boolean, managed?: SharedShortcutManaged) => Promise<BackendResponse<ShortcutResult>>,
  hidden: boolean,
  managed?: SharedShortcutManaged,
): Promise<string | null> {
  const operation = shortcutQueue.then(async () => {
    try {
      const response = await update(hidden, managed);
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
