// SPDX-License-Identifier: AGPL-3.0-or-later
import type { QuickMountSlot } from "@/types/settings";

const sameSlot = (a: QuickMountSlot, b: QuickMountSlot) =>
  a.filePath === b.filePath && a.driveLetter === b.driveLetter && a.targetType === b.targetType
  && a.mountTarget === b.mountTarget && a.mountPoint === b.mountPoint;

export function changeQuickMountSlot(current: QuickMountSlot[], expected: QuickMountSlot | null, replacement: QuickMountSlot | null): QuickMountSlot[] {
  const matches = expected ? current.flatMap((slot, index) => sameSlot(slot, expected) ? [index] : []) : [];
  if (expected && (matches.length === 0 || (replacement && matches.length !== 1))) throw new Error("The shortcut changed or has duplicate records. Refresh Quick Mount before editing it.");
  const index = matches[0] ?? -1;
  const next = current.filter((_, i) => i !== index);
  if (replacement) {
    const letter = replacement.driveLetter.replace(/:$/, "").toUpperCase();
    if (!/^[D-Z]$/.test(letter)) throw new Error("Select a non-system drive letter from D: to Z:.");
    if (next.some(slot => slot.driveLetter.replace(/:$/, "").toUpperCase() === letter)) {
      throw new Error(`Drive ${letter}: is reserved by another saved Quick Mount entry.`);
    }
    if (index < 0) next.push(replacement);
    else next.splice(index, 0, replacement);
  }
  return next;
}
