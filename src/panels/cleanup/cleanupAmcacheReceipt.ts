/** A queued deletion is not proof that Windows deleted anything. */
export function isConfirmedAmcacheDeferredReceipt(value: unknown): boolean {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const receipt = value as { ok?: unknown; stdout?: unknown };
  if (receipt.ok !== true || typeof receipt.stdout !== "string") return false;
  const match = /^ok: bootScheduled=([1-9]\d*)$/.exec(receipt.stdout.trim());
  return !!match && Number.isSafeInteger(Number(match[1]));
}
