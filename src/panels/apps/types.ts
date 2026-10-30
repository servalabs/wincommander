export type DebloatSource = "store" | "program" | "windows";

export interface DebloatItem {
  id: string;
  label: string;
  source: DebloatSource;
  category: string;
  sizeKB?: number;
  publisher?: string;
  version?: string;
  installDate?: string;
  installLocation?: string;
  packageFullName?: string;
  isProvisioned?: boolean;
  removalUnavailable?: string;
  /** Original local package/program icon, resolved by the backend. */
  iconData?: string | null;
  recommended: boolean;
  riskNote?: string;
  remove: () => Promise<{ success: boolean; error?: string }>;
}
