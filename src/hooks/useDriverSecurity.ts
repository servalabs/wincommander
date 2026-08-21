// src/hooks/useDriverSecurity.ts
//
// Typed wrapper around the vulnerable-driver (BYOVD) scan Tauri command,
// for NEW call sites outside src/panels/privacy/DriverHealthSection.tsx.
// That file already calls `get_vulnerable_drivers` directly but is a
// grandfathered exception under the A6 IPC-layering guard in
// eslint.config.js (LEGACY_RAW_INVOKE_FILES) — new code must not extend
// that allowlist, so it goes through this hook instead (see
// src/hooks/useArgus.ts for the established typed-wrapper pattern).

import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface VulnerableDriver {
  filename: string;
  path: string;
  state: string;
  reason: string;
  matchedBy: string;
}

export interface VulnerableDriversReport {
  vulnerable: VulnerableDriver[];
  scanned: number;
  ok: boolean;
}

/** On-demand `get_vulnerable_drivers` (BYOVD) scan — call `scan()` to run it. */
export function useVulnerableDriverScan() {
  const [report, setReport] = useState<VulnerableDriversReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scan = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await invoke<VulnerableDriversReport>("get_vulnerable_drivers");
      setReport(r);
      return r;
    } catch (err) {
      setError(String(err));
      return null;
    } finally {
      setLoading(false);
    }
  }, []);

  return { scan, report, loading, error };
}
