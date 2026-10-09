// SPDX-License-Identifier: AGPL-3.0-or-later
import { useCallback, useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { PortEntry, PortGuardHit, PortGuardStatus } from '@/lib/portGuard';

export default function usePortGuard() {
  const [status, setStatus] = useState<PortGuardStatus | null>(null);
  const [ports, setPorts] = useState<PortEntry[]>([]);
  const [recent, setRecent] = useState<PortGuardHit[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [stale, setStale] = useState(false);
  const mounted = useRef(false); const sequence = useRef(0); const mutating = useRef(false);
  const refresh = useCallback(async () => {
    if (mutating.current) return;
    const request = ++sequence.current;
    try {
      const [next, rules, hits] = await Promise.all([
        invoke<PortGuardStatus>('network_honeypot_status'),
        invoke<PortEntry[]>('get_network_honeypot_ports'),
        invoke<PortGuardHit[]>('get_network_honeypot_recent'),
      ]);
      if (!mounted.current || request !== sequence.current) return;
      if (next.schemaVersion !== 2) throw new Error('Update WinCommander and Pro together to use passive Port Guard.');
      setStatus(next); setPorts(rules); setRecent([...hits].reverse()); setStale(false); setReadError(null);
    } catch (reason) {
      if (mounted.current && request === sequence.current) { setReadError(String(reason)); setStale(true); }
    }
  }, []);
  useEffect(() => {
    const requests = sequence;
    mounted.current = true; void refresh();
    const timer = setInterval(() => { void refresh(); }, 3000);
    let disposed = false; let remove: (() => void) | undefined;
    void listen('network-honeypot-detected', () => { void refresh(); }).then(unlisten => {
      if (disposed) unlisten(); else remove = unlisten;
    }).catch(reason => { if (!disposed) setError(`Live notification updates unavailable: ${String(reason)}`); });
    return () => { disposed = true; mounted.current = false; requests.current++; clearInterval(timer); remove?.(); };
  }, [refresh]);
  const run = useCallback(async (command: string, args?: Record<string, unknown>) => {
    if (mutating.current) return false;
    mutating.current = true; sequence.current++; setBusy(true); setError(null);
    let succeeded = false;
    try { await invoke(command, args); succeeded = true; }
    catch (reason) { if (mounted.current) setError(String(reason)); }
    finally { mutating.current = false; if (mounted.current) { setBusy(false); await refresh(); } }
    return succeeded;
  }, [refresh]);
  return { status, ports, recent, error: readError || error, busy, stale, refresh, run };
}
