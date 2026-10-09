// SPDX-License-Identifier: AGPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import useBackend from './useBackend';
import type { FirewallBlock } from '@/lib/portGuard';

export default function usePortGuardFirewall() {
  const backend = useBackend(); const backendRef = useRef(backend); backendRef.current = backend;
  const [rules, setRules] = useState<FirewallBlock[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false); const [ping, setPing] = useState(false);
  const active = useRef(false);
  const refresh = useCallback(async () => {
    const result = await backendRef.current.getProtocolBlocks();
    if (!result.success || !result.data) throw new Error(result.error || 'Firewall rules could not be read.');
    const raw = result.data.blocks;
    const status = await invoke<{ blocked: boolean }>('get_ping_block_status');
    if (active.current) { setRules(Array.isArray(raw) ? raw : raw ? [raw] : []); setPing(status.blocked); }
  }, []);
  useEffect(() => {
    active.current = true;
    void refresh().catch(reason => { if (active.current) setError(String(reason)); });
    return () => { active.current = false; };
  }, [refresh]);
  async function run(action: () => Promise<void>) {
    setBusy(true); setError(null);
    try { await action(); await refresh(); }
    catch (reason) { if (active.current) setError(String(reason)); }
    finally { if (active.current) setBusy(false); }
  }
  async function add(label: string, ports: string, protocol: string, direction: 'Inbound' | 'Outbound' | 'Both') {
    const result = await backendRef.current.blockProtocol(label, ports, protocol, direction);
    if (!result.success) throw new Error(result.error || 'Firewall block failed.');
  }
  async function remove(name: string) {
    const result = await backendRef.current.unblockProtocol(name);
    if (!result.success) throw new Error(result.error || 'Firewall removal failed.');
  }
  async function changePing(enabled: boolean) { await invoke('set_ping_block', { enabled }); }
  return { rules, error, busy, ping, run, add, remove, changePing };
}
