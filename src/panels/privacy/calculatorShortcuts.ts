import type { BackendResponse } from "../../hooks/useBackend";

type ShortcutResult = { status: string; itemsChanged: number; warnings?: string[] };

export async function syncCalculatorShortcuts(
  update: (hidden: boolean) => Promise<BackendResponse<ShortcutResult>>,
  hidden: boolean,
): Promise<string | null> {
  try {
    const response = await update(hidden);
    if (!response.success) return response.error || "Could not update Calculator shortcuts.";
    if (!response.data || response.data.status === "error") return "Could not update Calculator shortcuts.";
    return response.data.warnings?.join(" ") || null;
  } catch (error) {
    return String(error);
  }
}
