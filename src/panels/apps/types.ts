export type DebloatSource = "store" | "program" | "windows";

export interface DebloatItem {
  id: string;
  label: string;
  source: DebloatSource;
  category: string;
  sizeKB?: number;
  /** Original local package/program icon, resolved by the backend. */
  iconData?: string | null;
  recommended: boolean;
  riskNote?: string;
  remove: () => Promise<{ success: boolean; error?: string }>;
}
