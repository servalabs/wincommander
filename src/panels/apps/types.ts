export type DebloatSource = "store" | "program" | "windows";

export interface DebloatItem {
  id: string;
  label: string;
  source: DebloatSource;
  category: string;
  sizeKB?: number;
  recommended: boolean;
  riskNote?: string;
  remove: () => Promise<{ success: boolean; error?: string }>;
}
