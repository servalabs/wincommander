// Remote Support: one click opens a 59-minute SSH session through a temporary
// tunnel and shows the address as a QR code. Pro owns the sshd and tunnel work.
import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button, Switch } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';

interface SupportState {
  running: boolean;
  url?: string;
  user?: string;
  secondsLeft?: number;
  qrSvg?: string;
  sshCommand?: string;
}

function mmss(total: number): string {
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export default function RemoteSupportSection() {
  const [state, setState] = useState<SupportState>({ running: false });
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [left, setLeft] = useState(0);

  const refresh = useCallback(async () => {
    try {
      const s = await invoke<SupportState>('remote_support_status');
      setState(s);
      setLeft(s.secondsLeft ?? 0);
    } catch {
      /* Free tier or sidecar unavailable: stay idle */
    }
  }, []);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    if (!state.running) return undefined;
    const id = window.setInterval(() => {
      setLeft((v) => {
        if (v <= 1) { void refresh(); return 0; }
        return v - 1;
      });
    }, 1000);
    return () => window.clearInterval(id);
  }, [state.running, refresh]);

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<SupportState>('start_remote_support');
      setState(s);
      setLeft(59 * 60);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke('stop_remote_support');
      setState({ running: false });
      setConsent(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const qrSrc = state.qrSvg ? `data:image/svg+xml;utf8,${encodeURIComponent(state.qrSvg)}` : null;

  return (
    <SectionCard title="Remote Support" icon="lifesaver">
      {!state.running ? (
        <div className="flex flex-col gap-3">
          <p className="text-[12px] text-[var(--color-text-secondary)]">
            Lets a support technician sign in over SSH for up to 59 minutes. This turns on the
            Windows SSH server, opens a temporary public tunnel, and shows a QR code for the
            address. Only the account you are signed in with can log in, with its Windows
            password. Everything is switched off and restored when you stop or the time ends.
          </p>
          <Switch
            checked={consent}
            label="I allow remote access to this PC for up to 59 minutes"
            onChange={(e) => setConsent((e.target as HTMLInputElement).checked)}
          />
          <div>
            <Button intent="primary" text="Start remote support" disabled={!consent || busy} loading={busy} onClick={() => void start()} />
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          {qrSrc && <img src={qrSrc} alt="QR code of the remote support address" width={220} height={220} style={{ background: '#fff', borderRadius: 8 }} />}
          <code className="text-[12px] break-all select-all">{state.url}</code>
          {state.user && <div className="text-[12px] text-[var(--color-text-secondary)]">Sign in as <strong>{state.user}</strong> with this PC&apos;s Windows password.</div>}
          <div className="text-[12px]">Time left: <strong>{mmss(left)}</strong></div>
          <div className="flex gap-2">
            <Button text="Copy address" onClick={() => void navigator.clipboard?.writeText(state.url ?? '')} />
            <Button intent="danger" text="Stop now" disabled={busy} loading={busy} onClick={() => void stop()} />
          </div>
        </div>
      )}
      {error && <div role="alert" className="text-[12px] text-[var(--color-danger)] mt-2">{error}</div>}
    </SectionCard>
  );
}
