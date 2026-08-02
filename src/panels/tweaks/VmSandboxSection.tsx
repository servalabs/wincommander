// src/panels/tweaks/VmSandboxSection.tsx
//
// "Disposable Isolation" — create / destroy throwaway execution environments:
// Hyper-V VMs (persistent, full) and Windows Sandbox (ephemeral; discards all
// state on close). The system-altering lifecycle runs in the Pro sidecar (PAID)
// behind the vm_* / sandbox_* feature_ids; this is the thin UI over them.
//
// Backends are DETECTED, not assumed — if neither Hyper-V nor Windows Sandbox is
// enabled, the section explains how to enable them (both need a reboot, which the
// user performs). Destroying a VM is a two-click confirm (it deletes the disks).

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Spinner, Tag } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';

interface VmCapabilities {
  hyperv: boolean;
  sandbox: boolean;
  hypervFeature: string;
}

type IsolationFeature = "hyperv" | "sandbox";

interface EnableFeatureResult {
  ok: boolean;
  alreadyEnabled: boolean;
  restartRequired: boolean;
  state: string;
  message: string;
}

interface VmInfo {
  Name: string;
  State: string;
  MemoryMB: number;
}

export default function VmSandboxSection() {
  const [caps, setCaps] = useState<VmCapabilities | null>(null);
  const [vms, setVms] = useState<VmInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [confirmDestroy, setConfirmDestroy] = useState<string | null>(null);

  const [name, setName] = useState('');
  const [memoryMb, setMemoryMb] = useState(2048);
  const [vhdSizeGb, setVhdSizeGb] = useState(40);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const c = await invoke<VmCapabilities>('vm_capabilities');
      setCaps(c);
      if (c.hyperv) {
        const r = await invoke<VmInfo[] | VmInfo>('vm_list');
        setVms(Array.isArray(r) ? r : [r]);
      } else {
        setVms([]);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = useCallback(
    async (fn: () => Promise<unknown>) => {
      setBusy(true);
      setError(null);
      setMessage(null);
      try {
        await fn();
        await refresh();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [refresh],
  );

  const createVm = () =>
    run(async () => {
      await invoke('vm_create', { args: { name, memoryMb, vhdSizeGb } });
      setName('');
    });
  const startVm = (n: string) => run(() => invoke('vm_start', { args: { name: n } }));
  const stopVm = (n: string) => run(() => invoke('vm_stop', { args: { name: n, force: false } }));
  const destroyVm = (n: string) =>
    run(async () => {
      await invoke('vm_destroy', { args: { name: n, deleteDisks: true } });
      setConfirmDestroy(null);
    });
  const launchSandbox = () =>
    run(() => invoke('sandbox_launch', { args: { networking: false, readOnly: true } }));
  const closeSandbox = () => run(() => invoke('sandbox_close'));
  const enableFeature = (feature: IsolationFeature) =>
    run(async () => {
      const result = await invoke<EnableFeatureResult>('vm_enable_feature', { args: { feature } });
      setMessage(result.message);
    });

  const headerRight = caps ? (
    <Tag minimal intent={caps.hyperv || caps.sandbox ? 'success' : undefined} className="font-mono">
      {!caps.hyperv && !caps.sandbox
        ? 'UNAVAILABLE'
        : [caps.hyperv ? 'HYPER-V' : null, caps.sandbox ? 'SANDBOX' : null]
            .filter(Boolean)
            .join(' · ')}
    </Tag>
  ) : null;

  return (
    <SectionCard title="Disposable Isolation" icon="cube" headerRight={headerRight}>
      <div className="flex flex-col gap-3">
        <div className="text-sm opacity-80">
          Create and destroy throwaway execution environments — Hyper-V virtual machines
          (persistent) and Windows Sandbox (ephemeral; discards everything on close). Use them to
          open untrusted files or isolate risky work.
        </div>

        <div className="flex items-center gap-2">
          <Button icon="refresh" minimal small onClick={() => void refresh()} disabled={loading || busy}>
            Refresh
          </Button>
          {(loading || busy) && <Spinner size={14} />}
        </div>

        {error && <div className="font-mono text-sm text-red-400" role="alert">{error}</div>}
        {!error && message && <div className="font-mono text-sm text-green-400" role="status">{message}</div>}

        {caps && (!caps.hyperv || !caps.sandbox) && (
          <div className="flex flex-col gap-2 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-bg-elevated)] p-3 text-sm">
            <div className="opacity-80">
              Enable only the isolation method you need. Windows applies the feature first; restart
              the device before it can become available.
            </div>
            <div className="flex flex-wrap gap-2">
              {!caps.hyperv && (
                <Button icon="cube" small onClick={() => enableFeature('hyperv')} disabled={busy}>
                  Enable Hyper-V
                </Button>
              )}
              {!caps.sandbox && (
                <Button icon="application" small onClick={() => enableFeature('sandbox')} disabled={busy}>
                  Enable Windows Sandbox
                </Button>
              )}
            </div>
            <div className="font-mono text-xs opacity-70">RESTART REQUIRED AFTER ENABLEMENT</div>
          </div>
        )}

        {caps?.sandbox && (
          <div className="flex items-center gap-2">
            <span className="font-mono text-xs opacity-70">WINDOWS SANDBOX</span>
            <Button icon="play" small onClick={launchSandbox} disabled={busy}>
              Launch
            </Button>
            <Button icon="cross" minimal small onClick={closeSandbox} disabled={busy}>
              Close
            </Button>
          </div>
        )}

        {caps?.hyperv && (
          <div className="flex flex-col gap-2">
            <span className="font-mono text-xs opacity-70">HYPER-V VIRTUAL MACHINES</span>

            <div className="flex flex-wrap items-end gap-2">
              <label className="flex flex-col text-xs">
                Name
                <input
                  className="bp6-input"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Test VM"
                />
              </label>
              <label className="flex flex-col text-xs">
                Memory (MB)
                <input
                  className="bp6-input"
                  type="number"
                  value={memoryMb}
                  onChange={(e) => setMemoryMb(Number(e.target.value))}
                />
              </label>
              <label className="flex flex-col text-xs">
                Disk (GB)
                <input
                  className="bp6-input"
                  type="number"
                  value={vhdSizeGb}
                  onChange={(e) => setVhdSizeGb(Number(e.target.value))}
                />
              </label>
              <Button icon="add" small onClick={createVm} disabled={busy || name.trim() === ''}>
                Create VM
              </Button>
            </div>

            {vms.length === 0 && <div className="text-sm opacity-70">No virtual machines.</div>}
            {vms.map((vm) => (
              <div key={vm.Name} className="flex items-center gap-2 border-t border-white/10 pt-2">
                <span className="flex-1 font-mono text-sm">{vm.Name}</span>
                <Tag minimal className="font-mono">
                  {vm.State}
                </Tag>
                <Button minimal small icon="play" onClick={() => startVm(vm.Name)} disabled={busy}>
                  Start
                </Button>
                <Button minimal small icon="stop" onClick={() => stopVm(vm.Name)} disabled={busy}>
                  Stop
                </Button>
                {confirmDestroy === vm.Name ? (
                  <Button small icon="trash" onClick={() => destroyVm(vm.Name)} disabled={busy}>
                    Confirm destroy (deletes disks)
                  </Button>
                ) : (
                  <Button minimal small icon="trash" onClick={() => setConfirmDestroy(vm.Name)} disabled={busy}>
                    Destroy
                  </Button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </SectionCard>
  );
}
