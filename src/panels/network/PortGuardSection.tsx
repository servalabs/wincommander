// SPDX-License-Identifier: AGPL-3.0-or-later
import { useState } from 'react';
import { Button, InputGroup, Switch, Tag } from '@/components/ui/bp';
import SectionCard from '@/components/shared/SectionCard';
import { WebRTCHeaderButton } from '@/components/network/WebRTCLeakCard';
import usePortGuard from '@/hooks/usePortGuard';
import { parsePortSpec, portDisplay, providerLabel, ruleKey, type PortProtocol } from '@/lib/portGuard';
import { PORT_PRESETS } from '@/lib/portGuardPresets';
import PortGuardFirewall from './PortGuardFirewall';

export default function PortGuardSection(_props: { expanded?: boolean; onExpandedChange?: (next: boolean) => void } = {}) {
  const guard = usePortGuard();
  const [spec, setSpec] = useState(''); const [label, setLabel] = useState('');
  const [protocol, setProtocol] = useState<PortProtocol>('tcp');
  const [formError, setFormError] = useState<string | null>(null);
  const statusText = providerLabel(guard.status, guard.stale);
  async function add() {
    setFormError(null);
    try {
      const ranges = parsePortSpec(spec);
      if (!label.trim() || label.trim().length > 64) throw new Error('Enter a label of 1–64 characters.');
      if (await guard.run('add_network_honeypot_custom_port', { ...ranges[0], portsSpec: spec, protocol, label: label.trim() })) {
        setSpec(''); setLabel('');
      }
    } catch (error) { setFormError(String(error)); }
  }
  return <SectionCard title="Port Guard" icon="shield" headerRight={<WebRTCHeaderButton />} armed={statusText === 'Monitoring'}>
    <p>Watch incoming activity on selected ports, including ports used by your apps. Windows supplies the observations; no test server is needed.</p>
    <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
      <Switch label="Watch enabled" aria-label="Enable Port Guard" checked={guard.status?.desiredEnabled ?? false} disabled={guard.busy || !guard.status}
        onChange={event => { void guard.run((event.target as HTMLInputElement).checked ? 'start_network_honeypot' : 'stop_network_honeypot'); }} />
      <Tag intent={statusText === 'Monitoring' ? 'success' : statusText === 'Off' ? 'none' : 'warning'}>{statusText}</Tag>
      <Button icon="refresh" disabled={guard.busy} onClick={() => { void guard.refresh(); }}>Refresh</Button>
    </div>
    {(guard.error || formError || guard.status?.lastError) && <p role="alert" style={{ color: 'var(--color-danger)' }}>{formError || guard.error || guard.status?.lastError}</p>}
    {guard.status && <div style={{ fontSize: 12, margin: '10px 0', color: 'var(--color-text-secondary)' }}>
      <p>{guard.status.coverageNote}</p>
      <p>Last checked: {new Date(guard.status.observedAt).toLocaleString()} · Observed events: {guard.status.observedEvents} · Queue losses: {guard.status.droppedEvents} · Older rows expired: {guard.status.historyEvicted}</p>
      {guard.status.historyError && <p role="alert">History could not be saved or loaded: {guard.status.historyError}</p>}
      {guard.status.collectionSettingsRetained && <p>Windows event collection stays enabled when watching stops. Existing firewall permissions are unchanged.</p>}
    </div>}
    <form onSubmit={event => { event.preventDefault(); void add(); }} style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 12 }}>
      <InputGroup aria-label="Ports to watch" placeholder="8080 or 9000-9010, 22" value={spec} onChange={event => setSpec(event.target.value)} style={{ minWidth: 200 }} />
      <InputGroup aria-label="Watch label" placeholder="Label" maxLength={64} value={label} onChange={event => setLabel(event.target.value)} />
      <select aria-label="Watch protocol" value={protocol} onChange={event => setProtocol(event.target.value as PortProtocol)}>
        <option value="tcp">TCP</option><option value="udp">UDP</option><option value="both">TCP + UDP</option>
      </select>
      <Button type="submit" icon="plus" intent="primary" loading={guard.busy}>Add watch</Button>
    </form>
    <details style={{ marginTop: 12 }}><summary>Port presets</summary>
      <p>Presets fill the form. Review the ports and protocol before adding.</p>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>{PORT_PRESETS.map(preset => <Button key={preset.name} small disabled={guard.busy}
        onClick={() => { setSpec(preset.ports); setLabel(preset.name); setProtocol(preset.protocol); }}>{preset.name} {preset.ports}</Button>)}</div>
    </details>
    <div style={{ overflowX: 'auto', marginTop: 14 }}><table className="bp5-html-table bp5-html-table-striped" style={{ width: '100%' }}>
      <thead><tr><th>Ports</th><th>Protocol</th><th>Label</th><th>Watch</th><th>Coverage</th><th>Action</th></tr></thead>
      <tbody>{guard.ports.map(rule => <tr key={ruleKey(rule)}>
        <td>{portDisplay(rule)}</td><td>{rule.protocol.toUpperCase()}</td><td>{rule.label}</td><td>
          <Switch checked={rule.enabled} disabled={guard.busy} aria-label={`${rule.enabled ? 'Disable' : 'Enable'} Port Watch for ${rule.label} on ${portDisplay(rule)} ${rule.protocol}`}
            onChange={event => { void guard.run('set_network_honeypot_port_enabled', { port: rule.port, endPort: rule.endPort, protocol: rule.protocol, enabled: (event.target as HTMLInputElement).checked }); }} />
        </td><td>{rule.enabled ? statusText : 'Disabled'}</td><td>
          <Button minimal icon="cross" disabled={guard.busy} aria-label={`Remove ${rule.label} on ${portDisplay(rule)} ${rule.protocol}`}
            onClick={() => { void guard.run('remove_network_honeypot_custom_port', { port: rule.port, endPort: rule.endPort, protocol: rule.protocol }); }}>Remove watch</Button>
        </td>
      </tr>)}</tbody>
    </table></div>
    {!guard.ports.length && !guard.stale && <p>No watches configured. Add a port or range above.</p>}
    <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginTop: 16 }}>
      <strong>Recent inbound observations</strong>
      <Button minimal disabled={guard.busy || !guard.recent.length} onClick={() => { void guard.run('clear_network_honeypot_recent'); }}>Clear history</Button>
    </div>
    <p style={{ fontSize: 12 }}>Allowed means Windows permitted network activity, not that someone logged in. Repeated activity stays in history; notifications are grouped. The latest 256 metadata records are encrypted for your Windows account. The most recent second may be lost if the app crashes.</p>
    <div style={{ maxHeight: 300, overflowY: 'auto' }}>{guard.recent.map(hit => <div key={hit.id} style={{ padding: '8px 0', borderBottom: '1px solid var(--color-border)' }}>
      <strong>{hit.service}</strong> · {hit.protocol.toUpperCase()}/{hit.port} · {hit.outcome} {hit.loopback ? '· local test / loopback' : ''}
      <div>From <code>{hit.peer}</code> to <code>{hit.localAddress}</code> · {new Date(hit.detectedAt).toLocaleString()}</div>
    </div>)}</div>
    {!guard.recent.length && <p>No observations recorded. A quiet list does not establish that the connection path has been tested.</p>}
    <PortGuardFirewall />
  </SectionCard>;
}
