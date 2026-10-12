// SPDX-License-Identifier: AGPL-3.0-or-later
import { useState } from 'react';
import { Button, InputGroup, Switch } from '@/components/ui/bp';
import usePortGuardFirewall from '@/hooks/usePortGuardFirewall';
import { firewallEnabled, parsePortSpec } from '@/lib/portGuard';
import { InfoPopover } from './InfoTip';

export default function PortGuardFirewall() {
  const firewall = usePortGuardFirewall();
  const { rules, error, busy, ping, run } = firewall;
  const [ports, setPorts] = useState(''); const [label, setLabel] = useState('');
  const [protocol, setProtocol] = useState('TCP');
  const [direction, setDirection] = useState<'Inbound' | 'Outbound' | 'Both'>('Inbound');
  async function add() {
    parsePortSpec(ports);
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(label.trim())) throw new Error('Use 1–64 letters, numbers, underscores or hyphens for the firewall label.');
    await firewall.add(label.trim(), ports, protocol, direction);
    setPorts(''); setLabel('');
  }
  return <details style={{ marginTop: 16 }}>
    <summary>Firewall port blocks</summary>
    <div className="port-guard-heading" style={{ marginTop: 8 }}>
      <span>Port rules, not whole-app blocks</span>
      <InfoPopover iconOnly title="What a firewall port block does">
        <p>Blocking changes which connections Windows permits. Removing a watch does not remove these rules. Watching never creates an allow rule.</p>
        <p>Inbound blocks match a port on this PC. Outbound blocks match the destination port on another device. Both creates one rule for each direction.</p>
        <p>Blocking TCP 7070 stops the default AnyDesk direct route, not its outbound relay routes. Stopping the whole app requires a separate program-specific firewall policy. Do not block shared web ports 80/443 globally to stop one app.</p>
        <p>Outbound blocks can interrupt browsing, downloads or remote access. Rule enabled means the Windows rule is enabled; verify the actual connection path separately.</p>
      </InfoPopover>
      <Button minimal icon="refresh" disabled={busy} onClick={() => { void run(async () => {}); }}>Refresh rules</Button>
    </div>
    {error && <p role="alert">{error}</p>}
    <Switch checked={ping} disabled={busy} label="Block pings" onChange={event => {
      const enabled = (event.target as HTMLInputElement).checked;
      void run(() => firewall.changePing(enabled));
    }} />
    <div className="port-guard-form">
      <InputGroup aria-label="Firewall ports" placeholder="Ports / ranges" value={ports} onChange={event => setPorts(event.target.value)} />
      <InputGroup aria-label="Firewall label" placeholder="Rule label" maxLength={64} value={label} onChange={event => setLabel(event.target.value)} />
      <select aria-label="Firewall protocol" value={protocol} onChange={event => setProtocol(event.target.value)}><option>TCP</option><option>UDP</option></select>
      <select aria-label="Firewall direction" value={direction} onChange={event => setDirection(event.target.value as typeof direction)}>
        <option value="Inbound">Inbound · local port</option><option value="Outbound">Outbound · remote port</option><option value="Both">Both directions</option>
      </select>
      <Button disabled={busy} intent="danger" onClick={() => { void run(add); }}>Block port</Button>
    </div>
    <div className="port-guard-table-scroll"><table className="bp5-html-table bp5-html-table-striped port-guard-table" aria-label="Firewall port blocks">
      <colgroup><col style={{ width: '27%' }} /><col style={{ width: '14%' }} /><col style={{ width: '12%' }} /><col style={{ width: '16%' }} /><col style={{ width: '13%' }} /><col style={{ width: '18%' }} /></colgroup>
      <thead><tr><th>Rule</th><th>Ports</th><th>Protocol</th><th>Direction / scope</th><th>Rule state</th><th>Action</th></tr></thead>
      <tbody>{rules.map(rule => <tr key={rule.Name}><td>{rule.Name}</td><td>{rule.Port}</td><td>{rule.Protocol}</td><td>{rule.Direction}<div>{rule.Direction === 'Both' ? 'Local + remote ports' : rule.Direction === 'Outbound' ? 'Remote port' : 'Local port'}</div></td><td>
        {rule.Verified !== true ? 'Needs review' : firewallEnabled(rule.Enabled) ? 'Enabled' : 'Disabled'}
        {!!rule.Issues?.length && <details><summary>Details</summary>{rule.Issues.map(issue => <div key={issue}>{issue}</div>)}</details>}
      </td><td>
        <Button disabled={busy} aria-label={`Remove firewall rule ${rule.Name}`} onClick={() => { void run(() => firewall.remove(rule.Name)); }}>Remove block</Button>
      </td></tr>)}</tbody>
    </table></div>
    {!rules.length && !error && <p>No WinCommander protocol blocks.</p>}
  </details>;
}
