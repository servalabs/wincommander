// SPDX-License-Identifier: AGPL-3.0-or-later
import { useState } from 'react';
import { Button, InputGroup, Switch } from '@/components/ui/bp';
import usePortGuardFirewall from '@/hooks/usePortGuardFirewall';
import { firewallEnabled, parsePortSpec } from '@/lib/portGuard';

export default function PortGuardFirewall() {
  const firewall = usePortGuardFirewall();
  const { rules, error, busy, ping, run } = firewall;
  const [ports, setPorts] = useState(''); const [label, setLabel] = useState('');
  const [protocol, setProtocol] = useState('TCP');
  const [direction, setDirection] = useState<'Inbound' | 'Outbound' | 'Both'>('Inbound');
  async function add() {
    parsePortSpec(ports);
    if (!label.trim()) throw new Error('Enter a firewall rule label.');
    await firewall.add(label.trim(), ports, protocol, direction);
    setPorts(''); setLabel('');
  }
  return <details style={{ marginTop: 16 }}>
    <summary>Firewall blocks (separate from watching)</summary>
    <p>Blocking changes which connections Windows permits. Removing a watch does not remove these rules.</p>
    {error && <p role="alert">{error}</p>}
    <Switch checked={ping} disabled={busy} label="Block pings" onChange={event => {
      const enabled = (event.target as HTMLInputElement).checked;
      void run(() => firewall.changePing(enabled));
    }} />
    <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
      <InputGroup aria-label="Firewall ports" placeholder="Ports / ranges" value={ports} onChange={event => setPorts(event.target.value)} />
      <InputGroup aria-label="Firewall label" placeholder="Rule label" value={label} onChange={event => setLabel(event.target.value)} />
      <select aria-label="Firewall protocol" value={protocol} onChange={event => setProtocol(event.target.value)}><option>TCP</option><option>UDP</option></select>
      <select aria-label="Firewall direction" value={direction} onChange={event => setDirection(event.target.value as typeof direction)}>
        <option>Inbound</option><option>Outbound</option><option>Both</option>
      </select>
      <Button disabled={busy} intent="danger" onClick={() => { void run(add); }}>Add block</Button>
    </div>
    <p>Outbound blocks can interrupt browsing, downloads or remote access.</p>
    <div style={{ overflowX: 'auto' }}><table className="bp5-html-table bp5-html-table-striped" style={{ width: '100%' }}>
      <thead><tr><th>Rule</th><th>Ports</th><th>Protocol</th><th>Direction</th><th>State</th><th>Action</th></tr></thead>
      <tbody>{rules.map(rule => <tr key={rule.Name}><td>{rule.Name}</td><td>{rule.Port}</td><td>{rule.Protocol}</td><td>{rule.Direction}</td><td>{firewallEnabled(rule.Enabled) ? 'Enabled' : 'Disabled'}</td><td>
        <Button disabled={busy} aria-label={`Remove firewall rule ${rule.Name}`} onClick={() => { void run(() => firewall.remove(rule.Name)); }}>Remove block</Button>
      </td></tr>)}</tbody>
    </table></div>
    {!rules.length && !error && <p>No WinCommander protocol blocks.</p>}
  </details>;
}
