export function shouldSkipStartupSplash(isDevelopment: boolean, pathname: string): boolean {
  return isDevelopment && pathname.endsWith("/ui-audit.html");
}
