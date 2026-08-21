export function resolveAvailableTab<T extends string>(
  available: readonly T[],
  current: T | undefined,
  preferred?: T,
): T | undefined {
  if (current && available.includes(current)) return current;
  if (preferred && available.includes(preferred)) return preferred;
  return available[0];
}
