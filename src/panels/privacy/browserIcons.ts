const BROWSER_ICON_SLUG: Record<string, string> = {
  "Google Chrome": "chrome",
  "Microsoft Edge": "edge",
  "Firefox": "firefox",
  "Brave": "brave",
  "Opera": "opera",
  "Opera GX": "opera-gx",
  "Vivaldi": "vivaldi",
  "LibreWolf": "librewolf",
  "Floorp": "floorp",
};

export function resolveBrowserIconSlug(name: string): string | undefined {
  return BROWSER_ICON_SLUG[name];
}

export function resolveBrowserIconUrl(
  name: string,
  browserLogos: Record<string, string>,
): string | undefined {
  const slug = resolveBrowserIconSlug(name);
  if (!slug) return undefined;
  return browserLogos[`${slug}.svg`];
}
