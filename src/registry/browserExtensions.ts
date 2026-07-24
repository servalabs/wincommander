// src/registry/browserExtensions.ts
//
// Per-extension toggle metadata for the browser-hardening bundle. Mirrors
// $script:ChromiumExtensionsBase / $script:FirefoxExtensions in
// scripts/modules/tweaks/security.ps1 — keep the slugs in sync with those
// lists' ToggleKey fields. uBlock Origin isn't listed here: it's always
// force-installed and isn't individually toggleable.

import type { IconName } from "@/components/ui/bp";
import { browserExtensionIcons } from "@/assets/browserExtensionIcons";

export interface BrowserExtensionToggle {
  /** Stable id persisted in settings.ideal.privacy.browserExtensions. Must
   *  match a ToggleKey in security.ps1's extension lists. */
  slug: string;
  /** Display name shown in the toggle list. */
  name: string;
  /** Short description of what the extension does. */
  description: string;
  /** Recognizable in-app icon shown beside the extension toggle. */
  icon: IconName;
  /** Publisher extension icon, stored locally so the privacy UI stays offline. */
  iconImage: string;
}

export const BROWSER_EXTENSION_TOGGLES: BrowserExtensionToggle[] = [
  { slug: "privacy-badger", name: "Privacy Badger", description: "Blocks invisible trackers", icon: "shield", iconImage: browserExtensionIcons["privacy-badger"] },
  { slug: "sponsorblock", name: "SponsorBlock", description: "Skips sponsor segments in YouTube videos", icon: "play", iconImage: browserExtensionIcons.sponsorblock },
  { slug: "volume-boost", name: "Volume Booster", description: "Boosts audio past a tab's default max volume", icon: "music", iconImage: browserExtensionIcons["volume-boost"] },
  { slug: "return-youtube-dislike", name: "Return YouTube Dislike", description: "Restores dislike counts on YouTube", icon: "undo", iconImage: browserExtensionIcons["return-youtube-dislike"] },
  { slug: "clearurls", name: "ClearURLs", description: "Strips tracking parameters from links", icon: "clean", iconImage: browserExtensionIcons.clearurls },
  { slug: "search-by-image", name: "Search by Image", description: "Reverse image search from the right-click menu", icon: "search-around", iconImage: browserExtensionIcons["search-by-image"] },
];

const BROWSER_EXTENSION_DELIMITER = "::";

/**
 * A browser-specific preference is stored in the existing flat settings map,
 * avoiding a breaking settings-schema migration. Browser names originate from
 * the backend's fixed detection map, not user-provided labels.
 */
export function browserExtensionSettingKey(browserName: string, slug: string): string {
  return `${browserName}${BROWSER_EXTENSION_DELIMITER}${slug}`;
}

/**
 * Browser-specific values take precedence. The legacy global slug remains a
 * fallback so current installations preserve their extension choices until a
 * user explicitly changes that browser's toggle.
 */
export function isBrowserExtensionEnabled(
  settings: Record<string, boolean> | undefined,
  browserName: string,
  slug: string,
): boolean {
  const browserValue = settings?.[browserExtensionSettingKey(browserName, slug)];
  if (browserValue !== undefined) return browserValue;
  return settings?.[slug] !== false;
}
