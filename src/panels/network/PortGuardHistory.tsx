// SPDX-License-Identifier: AGPL-3.0-or-later
import { Button } from '@/components/ui/bp';
import type { PortGuardHit } from '@/lib/portGuard';
import { groupPortGuardObservations } from '@/lib/portGuardHistory';
import { InfoPopover } from './InfoTip';

export default function PortGuardHistory({ hits, busy, onClear }: { hits: PortGuardHit[]; busy: boolean; onClear: () => void }) {
  const groups = groupPortGuardObservations(hits);
  return <section aria-label="Recent inbound observations" style={{ marginTop: 16 }}>
    <div className="port-guard-heading">
      <strong>Recent inbound observations</strong>
      <InfoPopover iconOnly title="About repeated network observations">
        <p>One connection attempt can generate several Windows observations, including automatic TCP retries. These are not necessarily separate people or logins.</p>
        <p>Matching endpoints, protocol and result within 30 seconds are grouped. Expand the count to see each recorded timestamp. Different source ports, network paths and allowed/blocked results stay separate.</p>
        <p>Allowed means Windows permitted network activity, not that someone logged in. Blocked means Windows dropped that traffic, not that every route used by the app is blocked.</p>
        <p>The latest 256 metadata records remain encrypted for your Windows account. Grouping only changes this display. Notifications are grouped separately; the most recent second may be lost if the app crashes.</p>
      </InfoPopover>
      <Button minimal disabled={busy || !hits.length} onClick={onClear}>Clear history</Button>
    </div>
    {groups.length > 0 ? <div className="port-guard-table-scroll" style={{ maxHeight: 360 }}>
      <table className="bp5-html-table bp5-html-table-striped port-guard-table" aria-label="Grouped inbound observations">
        <colgroup><col style={{ width: '18%' }} /><col style={{ width: '13%' }} /><col style={{ width: '22%' }} /><col style={{ width: '19%' }} /><col style={{ width: '12%' }} /><col style={{ width: '16%' }} /></colgroup>
        <thead><tr><th>Label</th><th>Port / protocol</th><th>From</th><th>To this PC</th><th>Result</th><th>Observations</th></tr></thead>
        <tbody>{groups.map(({ latest: hit, observations }) => <tr key={hit.id}>
          <td>{hit.service}</td><td>{hit.port}<br />{hit.protocol.toUpperCase()}</td>
          <td><code>{hit.peer}</code></td><td><code>{hit.localAddress}</code>{hit.loopback && <div>Local / loopback</div>}</td>
          <td>{hit.outcome}</td><td>
            <details><summary>{observations.length} {observations.length === 1 ? 'observation' : 'observations'}</summary>
              {observations.map(row => <div key={row.id}><time dateTime={row.detectedAt}>{new Date(row.detectedAt).toLocaleString()}</time></div>)}
            </details>
            <time dateTime={hit.detectedAt}>{new Date(hit.detectedAt).toLocaleTimeString()}</time>
          </td>
        </tr>)}</tbody>
      </table>
    </div> : <p>No observations recorded.</p>}
  </section>;
}
