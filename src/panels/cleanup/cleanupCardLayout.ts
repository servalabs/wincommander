export const CLEAN_CARDS_PER_GRID_SLOT = 4;

/** Keep completed categories in their scan order while fitting four into the
 * footprint of one regular cleanup card. */
export function packCleanCards<T>(cards: readonly T[]): T[][] {
  const packs: T[][] = [];
  for (let index = 0; index < cards.length; index += CLEAN_CARDS_PER_GRID_SLOT) {
    packs.push(cards.slice(index, index + CLEAN_CARDS_PER_GRID_SLOT));
  }
  return packs;
}
