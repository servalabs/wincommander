export interface CommandPaletteVisibility {
  alwaysHiddenPanels: readonly string[];
  lockedPanels: readonly string[];
  alwaysHiddenActions: readonly string[];
  borrowedHidden: readonly string[];
  borrowedActive: boolean;
}

export function isCommandPalettePanelVisible(
  panelId: string,
  visibility: CommandPaletteVisibility,
): boolean {
  if (visibility.alwaysHiddenPanels.includes(panelId)) return false;
  return !visibility.borrowedActive || !visibility.lockedPanels.includes(panelId);
}

export function isCommandPaletteActionVisible(
  actionId: string,
  visibility: CommandPaletteVisibility,
): boolean {
  if (visibility.alwaysHiddenActions.includes(actionId)) return false;
  return !visibility.borrowedActive || !visibility.borrowedHidden.includes(`action:${actionId}`);
}

export function isCommandPaletteFeatureVisible(
  featureId: string,
  visibility: CommandPaletteVisibility,
): boolean {
  return !visibility.borrowedActive || !visibility.borrowedHidden.includes(featureId);
}
