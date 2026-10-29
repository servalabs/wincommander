function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value && typeof value === "object") {
    const object = value as Record<string, unknown>;
    return `{${Object.keys(object).sort().map(key => `${JSON.stringify(key)}:${canonicalJson(object[key])}`).join(",")}}`;
  }
  return JSON.stringify(value);
}

export async function verifySavedInventory(result: unknown, observed: unknown): Promise<boolean> {
  if (!result || typeof result !== "object" || Array.isArray(result)) return false;
  const receipt = (result as Record<string, unknown>).__wcInventoryReceipt;
  if (!receipt || typeof receipt !== "object" || Array.isArray(receipt)) return false;
  const { state, sha256 } = receipt as Record<string, unknown>;
  if (state !== "confirmed" || typeof sha256 !== "string" || !/^[a-f0-9]{64}$/.test(sha256)
    || !observed || typeof observed !== "object" || Array.isArray(observed)) return false;
  try {
    const bytes = new TextEncoder().encode(canonicalJson(observed));
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    return Array.from(digest, byte => byte.toString(16).padStart(2, "0")).join("") === sha256;
  } catch {
    return false;
  }
}


export function joinInventoryScan(slot: { current: Promise<boolean> | null }, start: () => Promise<boolean>): Promise<boolean> {
  if (slot.current) return slot.current;
  const task = Promise.resolve().then(start).finally(() => {
    if (slot.current === task) slot.current = null;
  });
  slot.current = task;
  return task;
}
