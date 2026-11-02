/** Count consecutive clear-entry actions without revealing the shortcut in the UI. */
export function advanceCalculatorHandoff(count: number, isClearEntry: boolean): {
  count: number;
  shouldOpen: boolean;
} {
  if (!isClearEntry) return { count: 0, shouldOpen: false };
  const next = count + 1;
  return next >= 4 ? { count: 0, shouldOpen: true } : { count: next, shouldOpen: false };
}
