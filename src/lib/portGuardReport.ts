// SPDX-License-Identifier: AGPL-3.0-or-later
import type { PortGuardHit, PortGuardStatus } from './portGuard';

function cell(value: string | number | null | undefined): string {
  const text = String(value ?? '').replace(/[\r\n\t]+/g, ' ');
  return /^\s*[=+\-@]/.test(text) ? `'${text}` : text;
}

/** A bounded export of retained observations, not a complete network log. */
export function formatPortGuardReport(hits: PortGuardHit[], status: PortGuardStatus | null, stale: boolean, error: string | null, exportedAt: Date): string {
  const lines = [
    'WinCommander Port Guard observation history',
    `Exported (UTC): ${exportedAt.toISOString()}`,
    'Only retained observations for watched inbound ports are included. This is not a complete network history or packet capture.',
    'An observation is not necessarily a separate person, login or connection; retries can produce multiple observations.',
    `Retained rows: ${hits.length}; status: ${stale ? 'stale - last successful read' : status ? 'last successful read' : 'unavailable'}.`,
    `Last status time: ${cell(status?.observedAt) || 'unavailable'}; watching: ${status ? (status.running ? 'running' : 'not running') : 'unknown'}.`,
    `Coverage: ${cell(status?.coverage) || 'unknown'}; queue losses: ${status?.droppedEvents ?? 'unknown'}; older rows expired: ${status?.historyEvicted ?? 'unknown'}.`,
    `Coverage note: ${cell(status?.coverageNote) || 'unavailable'}.`,
    `History issue: ${status ? cell(status.historyError) || 'none reported' : 'unavailable'}; read issue: ${cell(error) || 'none reported'}.`,
    'Time (as recorded)\tLabel\tPort\tProtocol\tFrom\tTo this PC\tResult\tLoopback',
  ];
  for (const hit of hits) {
    lines.push([hit.detectedAt, hit.service, hit.port, hit.protocol, hit.peer, hit.localAddress, hit.outcome, hit.loopback ? 'yes' : 'no'].map(cell).join('\t'));
  }
  return lines.join('\n');
}
