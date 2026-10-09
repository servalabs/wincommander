import { describe, expect, test } from 'bun:test';
import { applyWifiApproval, wifiStatusLabel, withWifiGuardOperation, type WifiGuardStatus } from './wifiGuard';

function status(overrides: Partial<WifiGuardStatus> = {}): WifiGuardStatus {
  return { schemaVersion: 2, instanceId: 'one', baselineRevision: 1, running: true, collectorHealthy: true, health: 'ready', observedAt: new Date().toISOString(), pollIntervalSecs: 10, interfaces: [{ interfaceId: 'a', name: 'Wi-Fi', state: 'connected', ssid: 'Example', ssidHex: '4578616d706c65', bssid: '02:00:00:00:00:01', authAlgorithm: 7, cipherAlgorithm: 4, authStrength: 3, signal: 70, trust: 'approved' }], ...overrides };
}

describe('Wi-Fi coverage truth', () => {
  test('an applied approval with a lost reply is rolled back before reporting failure', async () => {
    let approved = false;
    const failure = await applyWifiApproval({ trust: async () => { approved = true; throw new Error('lost reply'); }, persist: async () => { throw new Error('must not persist'); }, restore: async () => { approved = false; }, stop: async () => undefined }).then(() => null, (error) => String(error));
    expect(failure).toContain('saved trust settings were restored');
    expect(approved).toBe(false);
  });
  test('a committed settings write with a lost reply is accepted after authoritative verification', async () => {
    let saved = false; let stopped = false;
    await applyWifiApproval({ trust: async () => undefined, persist: async () => { saved = true; throw new Error('lost write reply'); }, restore: async () => saved ? 'saved' : 'restored', stop: async () => { stopped = true; } });
    expect(saved).toBe(true); expect(stopped).toBe(false);
  });
  test('failed rollback stops the observer and does not claim restored trust', async () => {
    let stopped = false;
    const failure = await applyWifiApproval({ trust: async () => undefined, persist: async () => { throw new Error('disk full'); }, restore: async () => { throw new Error('unavailable'); }, stop: async () => { stopped = true; } }).then(() => null, (error) => String(error));
    expect(failure).toContain('observer was stopped');
    expect(stopped).toBe(true);
  });
  test('running without hardware, service or permissions never says guarding', () => {
    for (const health of ['noHardware', 'serviceStopped', 'permissionDenied', 'providerError', 'unknownSecurity']) {
      expect(wifiStatusLabel(status({ health }))).not.toContain('Guarding');
    }
  });
  test('expired readings and empty adapters never say guarding', () => {
    expect(wifiStatusLabel(status({ observedAt: '2000-01-01T00:00:00Z' }))).toBe('Reading is out of date');
    expect(wifiStatusLabel(status({ interfaces: [] }))).toBe('Waiting for a Wi-Fi connection');
  });
  test('approval alone is not a running observer', () => {
    expect(wifiStatusLabel(status({ running: false }))).toBe('Guard is off');
    expect(wifiStatusLabel(status({ collectorHealthy: false }))).not.toContain('Guarding');
    expect(wifiStatusLabel(status())).toBe('Guarding approved Wi-Fi');
  });
  test('learned identities need review', () => {
    const value = status(); value.interfaces[0].trust = 'learned';
    expect(wifiStatusLabel(value)).toBe('Current connection needs your review');
  });
  test('old sidecars cannot produce a healthy claim', () => { expect(wifiStatusLabel(status({ schemaVersion: 0 }))).toContain('update required'); });
  test('mutations are ordered and a failed operation does not block stop', async () => {
    const result: string[] = []; let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const first = withWifiGuardOperation(async () => { result.push('configure'); await gate; throw new Error('failed'); });
    const second = withWifiGuardOperation(async () => { result.push('stop'); });
    await Promise.resolve(); expect(result).toEqual(['configure']); release();
    await first.catch(() => undefined); await second; expect(result).toEqual(['configure', 'stop']);
  });
});
