// Keep the startup logo as a fingerprinted asset instead of embedding roughly
// 116 KB of base64 text into every JavaScript entry that needs the brand.
import logoUrl from "../../assets/products/wincommander/logo.png?url";

export const LOGO_URL = logoUrl;
