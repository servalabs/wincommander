import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { shouldRecoverPortGuard, type PortGuardRecoveryStatus } from '../lib/portGuardRecovery';
import { recordDiagnostic, newDiagnosticOperationId } from '../lib/diagnostics';

export default function usePortGuardMonitor(hasPaid: boolean) {
  const previouslyPaid = useRef(false);
  useEffect(() => {
    const wasPaid = previouslyPaid.current;
    previouslyPaid.current = hasPaid;
    if (!hasPaid && !wasPaid) return;
    let cancelled = false;
    let pending = false;
    let failureReported = false;
    let stopConfirmed = false;
    const reconcile = async () => {
      if (pending || cancelled || stopConfirmed) return;
      pending = true;
      try {
        if (!hasPaid) {
          // Cleanup remains allowed after expiry; do not issue a paid status
          // query or silently abandon a failed stop request.
          await invoke('stop_network_honeypot');
          stopConfirmed = true;
          return;
        }
        const status = await invoke<PortGuardRecoveryStatus>('network_honeypot_status');
        if (!cancelled && shouldRecoverPortGuard(status)) {
          await invoke('reconcile_network_honeypot');
        }
        failureReported = false;
      } catch {
        if (!cancelled && !failureReported) recordDiagnostic({
          operationId: newDiagnosticOperationId('port_guard'), feature: 'port_guard',
          action: 'start', stage: 'sidecar', lifecycle: 'applied', outcome: 'degraded',
          errorCode: 'NETWORK.PORT_GUARD.RECOVERY_UNAVAILABLE', severity: 'warn',
          retryability: 'automatic', suggestedNextAction: 'retry', privacyClass: 'restricted',
        });
        failureReported = true;
      } finally {
        pending = false;
      }
    };
    void reconcile();
    const timer = window.setInterval(() => void reconcile(), 30_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [hasPaid]);
}
