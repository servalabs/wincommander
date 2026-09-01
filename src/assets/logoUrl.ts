// The splash is the first frame of the desktop app. Embed the existing product
// logo so WebView never needs a second request before it can paint that frame.
import logoUrl from "../../assets/products/wincommander/logo.png?inline";

export const LOGO_URL = logoUrl;
