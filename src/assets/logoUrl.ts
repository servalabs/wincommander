// Keep the startup logo as a fingerprinted asset instead of embedding roughly
// 116 KB of base64 text into every JavaScript entry that needs the brand.
import logoUrl from "../../assets/products/wincommander/logo.png?url";

export const LOGO_URL = logoUrl;

/** Start fetching the packaged brand asset before the main React entry loads. */
export function preloadAppLogo(): void {
  if (document.querySelector('link[data-wincommander-logo-preload]')) return;
  const link = document.createElement("link");
  link.rel = "preload";
  link.as = "image";
  link.href = LOGO_URL;
  link.setAttribute("data-wincommander-logo-preload", "");
  document.head.append(link);
}
