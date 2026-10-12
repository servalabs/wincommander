// SPDX-License-Identifier: AGPL-3.0-or-later
import type { PortGuardHit } from './portGuard';

export interface PortGuardObservationGroup { latest: PortGuardHit; observations: PortGuardHit[] }
export function groupPortGuardObservations(hits: readonly PortGuardHit[]): PortGuardObservationGroup[] {
  const groups: PortGuardObservationGroup[] = [];
  const latestByEndpoint = new Map<string, PortGuardObservationGroup>();
  const sorted = [...hits].sort((a, b) => (Date.parse(b.detectedAt) || 0) - (Date.parse(a.detectedAt) || 0) || b.id - a.id);
  for (const hit of sorted) {
    const key = JSON.stringify([hit.service, hit.protocol, hit.port, hit.peer, hit.localAddress, hit.outcome, hit.loopback]);
    const group = latestByEndpoint.get(key);
    const age = group ? Date.parse(group.latest.detectedAt) - Date.parse(hit.detectedAt) : NaN;
    if (group && Number.isFinite(age) && age >= 0 && age <= 30_000) {
      group.observations.push(hit);
    } else {
      const next = { latest: hit, observations: [hit] };
      groups.push(next); latestByEndpoint.set(key, next);
    }
  }
  return groups;
}
