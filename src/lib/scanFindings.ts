export interface ScanFinding {
  id: string;
  category: 'privacy' | 'performance' | 'annoyance' | 'engines' | 'updates';
  label: string;
  impact: string;
  severity: 'critical' | 'warning' | 'info';
  /** If true, this fix is safe for all users and won't break anything */
  safeDefault?: boolean;
  /** True when this finding means ideal.* differs from current.*. */
  drift?: boolean;
  /** Desired checked state to re-apply when fixing drift. */
  targetChecked?: boolean;
}

export interface ScanReport {
  findings: ScanFinding[];
  cpuUsage: number;
  ramUsage: number;
  diskReclaimableGb: number;
}
