import { describe, expect, test } from 'bun:test';
import { changeUsbPreference, compareUsbTimelineEvents, isCurrentUsbSession, matchUsbVolume, usbEventState, usbMonitorPresentation, usbPolicyReceiptVerified, usbPolicyTimelineRows, usbReadOnlyPolicyReceiptVerified, usbVolumeAccessState, usbWindowsDeviceState } from './usbTimeline';

const volume = { driveLetter: 'E:', instanceId: 'USB\\VID_1234&PID_5678\\TEST' };
const entry = { category: 'Storage', attached: true, instanceId: volume.instanceId, driveLetter: null };

describe('USB identity and verified event display', () => {
  test('running monitor remains visible and stoppable when its saved preference is off', () => {
    expect(usbMonitorPresentation(false, true, false)).toEqual({ label: 'Monitoring', checked: true, mismatch: true });
    expect(usbMonitorPresentation(true, false, false)).toEqual({ label: 'Not running', checked: true, mismatch: true });
    expect(usbMonitorPresentation(false, false, false)).toEqual({ label: 'Off', checked: false, mismatch: false });
    expect(usbMonitorPresentation(true, true, true).label).toBe('Needs attention');
  });

  test('policy decisions never turn requested isolation or ignored scope into a verified allow/block', () => {
    const record = { time: '2026-10-08T12:00:00Z', deviceKey: 'test', friendlyName: 'Test USB', action: 'quarantine', enforced: false, detail: 'Decision recorded' };
    const rows = usbPolicyTimelineRows([
      { ...record, enforced: true, detail: 'Windows readback' }, record,
      { ...record, action: 'ignore' }, { ...record, action: 'alert' },
      { ...record, time: 'invalid' },
    ]);
    expect(rows.map((row) => row.state)).toEqual(['Isolation verified', 'Isolation requested / unverified', 'Policy: no automatic action', 'Policy: alert only']);
    expect(new Set(rows.map((row) => row.id)).size).toBe(4);
    expect(rows[0].id).not.toBe(rows[1].id);
    expect([...rows].sort(compareUsbTimelineEvents)).toEqual(rows);
    expect(usbPolicyTimelineRows([record, record]).map((row) => row.id)[0]).not.toBe(usbPolicyTimelineRows([record, record])[1].id);
  });

  test('preference save failures remain errors and still refresh actual USB runtime', async () => {
    const calls: string[] = [];
    const failedSave = new Error('settings unavailable');
    const result = await changeUsbPreference(
      async () => { calls.push('apply'); },
      async () => { calls.push('save'); throw failedSave; },
      async () => { calls.push('refresh'); },
    ).then(() => null, (error: unknown) => error);
    expect(result).toBe(failedSave);
    expect(calls).toEqual(['apply', 'save', 'refresh']);
  });

  test('failed Windows operation does not save the requested preference but refreshes actual state', async () => {
    const calls: string[] = [];
    const result = await changeUsbPreference(
      async () => { calls.push('apply'); throw new Error('Windows denied'); },
      async () => { calls.push('save'); },
      async () => { calls.push('refresh'); },
    ).then(() => null, (error: unknown) => error);
    expect(result instanceof Error && result.message).toBe('Windows denied');
    expect(calls).toEqual(['apply', 'refresh']);
  });

  test('same-second policy observations retain their recorded order, newest first', () => {
    const block = { at: 100, sortOrder: 100001 };
    const allow = { at: 100, sortOrder: 100002 };
    const earlier = { at: 99 };
    expect([block, earlier, allow].sort(compareUsbTimelineEvents)).toEqual([allow, block, earlier]);
    expect(compareUsbTimelineEvents({ at: 100 }, { at: 100 })).toBe(0);
  });

  test('reconnecting a known device does not mark previous open sessions connected', () => {
    const keys = new Set(['device-a']);
    const prior = { deviceKey: 'device-a', attachedAt: 10, detachedAt: null };
    const current = { ...prior, attachedAt: 100 };
    expect(isCurrentUsbSession(prior, keys, 100)).toBe(false);
    expect(isCurrentUsbSession(current, keys, 100)).toBe(true);
    expect(isCurrentUsbSession(current, keys, null)).toBe(false);
    expect(isCurrentUsbSession(current, new Set(), 100)).toBe(false);
    expect(isCurrentUsbSession({ ...current, detachedAt: 120 }, keys, 100)).toBe(false);
    expect(isCurrentUsbSession({ ...current, endedUnobservedAt: 120 }, keys, 100)).toBe(false);
  });

  test('only associates a connected physical identity with its own volume', () => {
    expect(matchUsbVolume(entry, [volume])).toBe(volume);
    expect(matchUsbVolume({ ...entry, instanceId: entry.instanceId.toLowerCase() }, [volume])).toBe(volume);
    expect(matchUsbVolume({ ...entry, instanceId: 'OTHER' }, [volume])).toBeUndefined();
    expect(matchUsbVolume({ ...entry, attached: false }, [volume])).toBeUndefined();
    expect(matchUsbVolume({ ...entry, instanceId: '' }, [volume])).toBeUndefined();
    expect(matchUsbVolume({ ...entry, category: 'Keyboard / HID' }, [volume])).toBeUndefined();
  });

  test('a reused drive letter cannot authorize an unrelated volume', () => {
    expect(matchUsbVolume({ ...entry, instanceId: 'OTHER', driveLetter: 'E:' }, [volume])).toBeUndefined();
    expect(matchUsbVolume({ ...entry, driveLetter: 'F:' }, [volume])).toBeUndefined();
  });

  test('multiple partitions require an explicit matching volume letter', () => {
    const otherPartition = { ...volume, driveLetter: 'F:' };
    expect(matchUsbVolume(entry, [volume, otherPartition])).toBeUndefined();
    expect(matchUsbVolume({ ...entry, driveLetter: 'F:' }, [volume, otherPartition])).toBe(otherPartition);
  });

  test('a command acknowledgement is not Windows verification', () => {
    for (const receipt of [undefined, null, {}, { ok: true }, { verified: false }, { verified: 'true' }]) {
      expect(usbPolicyReceiptVerified(receipt)).toBe(false);
    }
    expect(usbPolicyReceiptVerified({ verified: true })).toBe(true);
    const event = { id: 'one', deviceKey: 'test', kind: 'block_applied', at: 10 };
    expect(usbEventState(event)).toBe('Block requested');
    expect(usbEventState({ ...event, verified: true })).toBe('Block verified');
    expect(usbEventState({ ...event, kind: 'block_failed' })).toBe('Block failed');
    expect(usbEventState({ ...event, kind: 'mounted' })).toBe('Mount observed');
    expect(usbEventState({ ...event, kind: 'unmounted' })).toBe('Unmount observed');
  });

  test('read-only receipt proves the requested whole physical USB state', () => {
    const receipt = {
      verified: true,
      readOnly: true,
      scope: 'physicalUsbDisk',
      affectedDriveLetters: ['E:', 'F:'],
    };
    expect(usbReadOnlyPolicyReceiptVerified(receipt, true)).toBe(true);
    expect(usbReadOnlyPolicyReceiptVerified(receipt, false)).toBe(false);
    expect(usbReadOnlyPolicyReceiptVerified({ ...receipt, scope: 'volume' }, true)).toBe(false);
    expect(usbReadOnlyPolicyReceiptVerified({ ...receipt, affectedDriveLetters: [] }, true)).toBe(false);
    expect(usbReadOnlyPolicyReceiptVerified({ ...receipt, verified: false }, true)).toBe(false);
  });

  test('only a Windows PnP readback marks a device allowed or blocked', () => {
    expect(usbWindowsDeviceState({ problemCode: 0 })).toBe('allowed');
    expect(usbWindowsDeviceState({ pnpStatus: 'OK' })).toBe('allowed');
    expect(usbWindowsDeviceState({ problemCode: 22, pnpStatus: 'Error' })).toBe('blocked');
    expect(usbWindowsDeviceState({ problemCode: 10, pnpStatus: 'Error' })).toBe('unknown');
    expect(usbWindowsDeviceState({})).toBe('unknown');
  });

  test('read-only is shown only when the volume readback says so', () => {
    expect(usbVolumeAccessState({ readOnly: true })).toBe('read-only');
    expect(usbVolumeAccessState({ readOnly: false })).toBe('writable');
    expect(usbVolumeAccessState({})).toBe('unknown');
    expect(usbVolumeAccessState(undefined)).toBe('unknown');
  });
});
