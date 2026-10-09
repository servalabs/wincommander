import type { WifiGuardBaselineEntry } from '../types/settings';

export function wifiBaselineIdentity(entries: WifiGuardBaselineEntry[]): string {
  return JSON.stringify(entries.map((entry) => JSON.stringify([
    entry.ssidHex ?? null, entry.ssid, [...entry.bssids].map((value) => value.toLowerCase()).sort(),
    entry.bestAuthStrength, entry.provenance ?? 'learned', entry.authAlgorithm ?? null, entry.cipherAlgorithm ?? null,
  ])).sort());
}

export interface WifiGuardAssociation {
  interfaceId: string;
  name: string;
  state: string;
  ssid: string;
  ssidHex: string;
  bssid: string;
  authAlgorithm: number | null;
  cipherAlgorithm: number | null;
  authStrength: number | null;
  signal: number;
  trust: string;
  errorCode?: number | null;
}

export interface WifiGuardStatus {
  schemaVersion: number;
  instanceId: string;
  baselineRevision: number;
  running: boolean;
  collectorHealthy: boolean;
  health: string;
  observedAt: string;
  errorCode?: number | null;
  interfaces: WifiGuardAssociation[];
  pollIntervalSecs: number;
}

export interface WifiGuardHit {
  id: string;
  interfaceId: string;
  ssid: string;
  bssid: string;
  reason: string;
  detectedAt: string;
}

export function wifiStatusLabel(status: WifiGuardStatus | null, now = Date.now()): string {
  if (!status) return 'Checking Wi-Fi';
  if (status.schemaVersion !== 2) return 'Wi-Fi Guard update required';
  const observed = Date.parse(status.observedAt);
  if (!Number.isFinite(observed) || now - observed > Math.max(status.pollIntervalSecs * 2, 15) * 1000) return 'Reading is out of date';
  const states: Record<string, string> = {
    noHardware: 'No Wi-Fi adapter detected', serviceStopped: 'Windows Wi-Fi service is stopped',
    serviceUnavailable: 'Windows Wi-Fi service is unavailable', disconnected: 'Waiting for a Wi-Fi connection',
    reconnecting: 'Wi-Fi is reconnecting', permissionDenied: 'Windows permission is needed',
    providerError: 'Windows could not read Wi-Fi', providerBusy: 'Waiting for Windows',
    stale: 'Reading is out of date', starting: 'Starting Wi-Fi observation', unsupported: 'Wi-Fi observation is unsupported',
    unknownSecurity: 'Wi-Fi security could not be verified', needsReview: 'Current connection needs your review',
  };
  if (status.health !== 'ready') return states[status.health] ?? 'Wi-Fi observation is unavailable';
  const connected = status.interfaces.filter((row) => row.state === 'connected');
  if (!connected.length) return 'Waiting for a Wi-Fi connection';
  if (connected.some((row) => row.trust !== 'approved')) return 'Current connection needs your review';
  return status.running && status.collectorHealthy ? 'Guarding approved Wi-Fi' : 'Guard is off';
}

export function wifiReasonLabel(reason: string): string {
  return ({ newBssid: 'Unapproved access point', authDowngrade: 'Weaker Wi-Fi security', securityChanged: 'Wi-Fi security changed', learned: 'Previously learned connection needs approval', untrusted: 'Connection has not been approved' } as Record<string, string>)[reason] ?? 'Connection needs review';
}

export function canTrustAssociation(row: WifiGuardAssociation): boolean {
  return row.state === 'connected' && row.trust !== 'unknownSecurity' && row.authStrength !== null && row.authAlgorithm !== null && row.cipherAlgorithm !== null && !!row.ssidHex && !!row.bssid;
}

export function wifiSecurityLabel(row: WifiGuardAssociation): string {
  const auth: Record<number, string> = { 1: 'Open', 2: 'WEP shared key', 3: 'WPA Enterprise', 4: 'WPA Personal', 5: 'WPA', 6: 'WPA2 Enterprise', 7: 'WPA2 Personal', 8: 'WPA3 Enterprise 192-bit', 9: 'WPA3 Personal', 10: 'Enhanced Open', 11: 'WPA3 Enterprise' };
  const cipher: Record<number, string> = { 0: 'no encryption', 1: 'WEP', 2: 'TKIP', 4: 'AES-CCMP', 5: 'WEP', 8: 'AES-GCMP', 9: 'AES-GCMP-256', 10: 'AES-CCMP-256', 257: 'WEP' };
  return `${auth[row.authAlgorithm ?? -1] ?? 'Unknown authentication'} · ${cipher[row.cipherAlgorithm ?? -1] ?? 'unknown encryption'}`;
}

let operationTail: Promise<unknown> = Promise.resolve();
let recoveryBlocked = false;
export const isWifiGuardRecoveryBlocked = () => recoveryBlocked;
export const setWifiGuardRecoveryBlocked = (blocked: boolean) => { recoveryBlocked = blocked; };
// UI mutations and startup reconciliation share one queue, so a late configure
// cannot undo a newer stop or trust operation.
export function withWifiGuardOperation<T>(operation: () => Promise<T>): Promise<T> {
  const result = operationTail.then(operation, operation);
  operationTail = result.catch(() => undefined);
  return result;
}

export async function applyWifiApproval(steps: {
  trust: () => Promise<unknown>;
  persist: () => Promise<void>;
  restore: () => Promise<unknown>;
  stop: () => Promise<unknown>;
}): Promise<void> {
  recoveryBlocked = true;
  try { await steps.trust(); await steps.persist(); recoveryBlocked = false; }
  catch (failure) {
    // Dispatch failure can be an applied request with a lost reply.
    try {
      const outcome = await steps.restore();
      recoveryBlocked = false;
      if (outcome === 'saved') return;
    }
    catch {
      try { await steps.stop(); }
      catch { throw new Error(`Approval and recovery could not be verified. Refresh before relying on this guard. ${String(failure)}`); }
      throw new Error(`Approval could not be saved; the observer was stopped because restoring trust failed. ${String(failure)}`);
    }
    throw new Error(`Approval was not confirmed. The saved trust settings were restored. ${String(failure)}`);
  }
}
