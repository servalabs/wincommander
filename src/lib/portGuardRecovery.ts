export interface PortGuardRecoveryStatus {
  schemaVersion?: number;
  desiredEnabled?: boolean;
  running: boolean;
  health?: string;
}

export function shouldRecoverPortGuard(status: PortGuardRecoveryStatus): boolean {
  // Older sidecars expose a listener flag, not persisted passive-monitor intent.
  return status.schemaVersion === 2 && status.desiredEnabled === true && !status.running;
}
