// SPDX-License-Identifier: AGPL-3.0-or-later
import { useState } from 'react';
import { Button, InputGroup, Switch, Tag } from '@/components/ui/bp';
import SectionCard from '@/components/shared/SectionCard';
import { WebRTCHeaderButton } from '@/components/network/WebRTCLeakCard';
import usePortGuard from '@/hooks/usePortGuard';
import { parsePortSpec, portDisplay, providerLabel, ruleKey, type PortProtocol } from '@/lib/portGuard';
import { PORT_PRESET_CATEGORIES } from '@/lib/portGuardPresets';
import PortGuardFirewall from './PortGuardFirewall';
import PortGuardHistory from './PortGuardHistory';
import { InfoPopover } from './InfoTip';
import './PortGuard.css';

export default function PortGuardSection(_props: { expanded?: boolean; onExpandedChange?: (next: boolean) => void } = {}) {
  const guard = usePortGuard();
  const [spec, setSpec] = useState(''); const [label, setLabel] = useState('');
  const [protocol, setProtocol] = useState<PortProtocol>('tcp');
  const [formError, setFormError] = useState<string | null>(null);
  const [presetNote, setPresetNote] = useState<string | null>(null);
  const statusText = providerLabel(guard.status, guard.stale);
  async function add() {
    setFormError(null);
    try {
      const ranges = parsePortSpec(spec);
      if (!label.trim() || label.trim().length > 64) throw new Error('Enter a label of 1–64 characters.');
      if (await guard.run('add_network_honeypot_custom_port', { ...ranges[0], portsSpec: spec, protocol, label: label.trim() })) {
        setSpec(''); setLabel(''); setPresetNote(null);
      }
    } catch (error) { setFormError(String(error)); }
  }
  return <SectionCard title="Port Guard" className="port-guard-card" icon="shield" headerRight={<WebRTCHeaderButton />} armed={!!guard.status?.running && !guard.stale && !guard.status?.lastError}>
    <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
      <Switch label="Watch enabled" aria-label="Enable Port Guard" checked={guard.status?.desiredEnabled ?? false} disabled={guard.busy || !guard.status}
        onChange={event => { void guard.run((event.target as HTMLInputElement).checked ? 'start_network_honeypot' : 'stop_network_honeypot'); }} />
      <Tag intent={statusText === 'Monitoring' ? 'success' : statusText === 'Off' ? 'none' : 'warning'}>{statusText}</Tag>
      <InfoPopover iconOnly title="Port Guard coverage and diagnostics">
        <p>Limited coverage means the monitor is running, but Windows does not report every packet or app session. It is not a claim that all access is blocked.</p>
        <p>Port Guard passively watches selected ports, including ports used by your apps. It is not a decoy server: it does not bind a port, answer connections or change firewall permissions.</p>
        {guard.status && <>
          <p>{guard.status.coverageNote}</p>
          <p>Last checked: {new Date(guard.status.observedAt).toLocaleString()} · Observed events: {guard.status.observedEvents} · Queue losses: {guard.status.droppedEvents} · Older rows expired: {guard.status.historyEvicted}</p>
          {guard.status.collectionSettingsRetained && <p>Windows event collection stays enabled when watching stops. Existing firewall permissions are unchanged.</p>}
        </>}
      </InfoPopover>
      <Button icon="refresh" disabled={guard.busy} onClick={() => { void guard.refresh(); }}>Refresh</Button>
    </div>
    {(guard.error || formError || guard.status?.lastError) && <p role="alert" style={{ color: 'var(--color-danger)' }}>{formError || guard.error || guard.status?.lastError}</p>}
    {guard.status?.historyError && <p role="alert">History could not be saved or loaded: {guard.status.historyError}</p>}
    {!!guard.status?.droppedEvents && <p role="alert">Windows event queue lost {guard.status.droppedEvents} observations. See diagnostics.</p>}
    <form onSubmit={event => { event.preventDefault(); void add(); }} className="port-guard-form">
      <InputGroup aria-label="Ports to watch" placeholder="8080 or 9000-9010, 22" value={spec} onChange={event => { setSpec(event.target.value); setPresetNote(null); }} style={{ minWidth: 200 }} />
      <InputGroup aria-label="Watch label" placeholder="Label" maxLength={64} value={label} onChange={event => { setLabel(event.target.value); setPresetNote(null); }} />
      <select aria-label="Watch protocol" value={protocol} onChange={event => { setProtocol(event.target.value as PortProtocol); setPresetNote(null); }}>
        <option value="tcp">TCP</option><option value="udp">UDP</option><option value="both">TCP + UDP</option>
      </select>
      <Button type="submit" icon="plus" intent="primary" loading={guard.busy}>Add watch</Button>
    </form>
    {presetNote && <div className="port-guard-heading" style={{ marginTop: 8 }}><span style={{ fontSize: 12 }}>Selected: {label}</span><InfoPopover iconOnly title="Selected preset guidance"><p>{presetNote}</p></InfoPopover></div>}
    <section aria-label="Port presets" style={{ marginTop: 16, padding: 12, border: '1px solid var(--color-border)', borderRadius: 6 }}>
      <div className="port-guard-heading"><strong style={{ fontSize: 12 }}>Port presets — click to prefill</strong>
        <InfoPopover iconOnly title="Choosing a port preset">
          <p>Choose a port on this PC, then select Add watch. Defaults can be changed in each app. A watch records new incoming attempts; it does not open ports, change firewall permissions or detect every app session.</p>
          <p>These presets watch incoming activity on this PC. Outgoing relay sessions, such as AnyDesk connecting to its servers, may never reach your watched local port. For FTP passive data, enter the server's configured local range.</p>
        </InfoPopover>
      </div>
      {PORT_PRESET_CATEGORIES.map(category => <div key={category.name} style={{ marginTop: 10 }}>
        <div style={{ fontSize: 11, fontWeight: 700, textTransform: 'uppercase', color: 'var(--color-text-secondary)', marginBottom: 5 }}>{category.name}</div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>{category.presets.map(preset => <Button key={preset.name} small className="port-guard-preset" disabled={guard.busy}
          title={preset.note}
          onClick={() => { setSpec(preset.ports); setLabel(preset.name); setProtocol(preset.protocol); setPresetNote(preset.note ?? 'Check that your app uses this local port, then make a fresh connection after enabling the watch.'); }}>{preset.name} <span style={{ opacity: 0.7 }}>{preset.ports}</span></Button>)}</div>
      </div>)}
    </section>
    <div className="port-guard-table-scroll"><table className="bp5-html-table bp5-html-table-striped port-guard-table" aria-label="Watched ports">
      <colgroup><col style={{ width: '12%' }} /><col style={{ width: '12%' }} /><col style={{ width: '27%' }} /><col style={{ width: '11%' }} /><col style={{ width: '21%' }} /><col style={{ width: '17%' }} /></colgroup>
      <thead><tr><th>Ports</th><th>Protocol</th><th>Label</th><th>Watch</th><th>Coverage</th><th>Action</th></tr></thead>
      <tbody>{guard.ports.map(rule => <tr key={ruleKey(rule)}>
        <td>{portDisplay(rule)}</td><td>{rule.protocol.toUpperCase()}</td><td>{rule.label}
          {rule.label === 'AnyDesk relay' && <InfoPopover iconOnly title="Legacy AnyDesk relay watch"><p>This incoming watch does not identify AnyDesk relay sessions. Use the actual direct port (default TCP 7070).</p></InfoPopover>}
        </td><td>
          <Switch checked={rule.enabled} disabled={guard.busy} aria-label={`${rule.enabled ? 'Disable' : 'Enable'} Port Watch for ${rule.label} on ${portDisplay(rule)} ${rule.protocol}`}
            onChange={event => { void guard.run('set_network_honeypot_port_enabled', { port: rule.port, endPort: rule.endPort, protocol: rule.protocol, enabled: (event.target as HTMLInputElement).checked }); }} />
        </td><td>{rule.enabled ? statusText : 'Disabled'}</td><td>
          <Button minimal icon="cross" disabled={guard.busy} aria-label={`Remove ${rule.label} on ${portDisplay(rule)} ${rule.protocol}`}
            onClick={() => { void guard.run('remove_network_honeypot_custom_port', { port: rule.port, endPort: rule.endPort, protocol: rule.protocol }); }}>Remove watch</Button>
        </td>
      </tr>)}</tbody>
    </table></div>
    {!guard.ports.length && !guard.stale && <p>No watches configured. Add a port or range above.</p>}
    <PortGuardHistory hits={guard.recent} busy={guard.busy} onClear={() => { void guard.run('clear_network_honeypot_recent'); }} />
    <PortGuardFirewall />
  </SectionCard>;
}
