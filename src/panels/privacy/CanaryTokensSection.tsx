// src/panels/privacy/CanaryTokensSection.tsx
//
// Canary Tokens — self-hosted HTTP beacon canaries (v1).
// A "canary" is an artifact that, when opened/fetched, requests a unique URL
// on our local HTTP listener, recording a hit + notifying the user.
//
// v1 token types:
//   "docx" — a real .docx (ZIP) whose word/document.xml embeds a remote-image
//             reference pointing at http://127.0.0.1:<port>/beacon/<id>.
//   "url"  — a Windows .url internet-shortcut whose URL= is the beacon URL.
//
// The listener is a local HTTP server in the Pro sidecar; DNS is v2.
// Wire commands: generate_canary, list_canaries, delete_canary,
//   start_canary_listener, stop_canary_listener, canary_listener_status,
//   get_canary_recent, clear_canary_recent.
// Live updates arrive on the "canary-token-fired" Tauri event.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { Button, HTMLSelect, InputGroup, Spinner, Tag } from '@/components/ui/bp';
import type { Intent } from '@/components/ui/bp';
import SectionCard from '../../components/shared/SectionCard';
import { showError, showSuccess } from '../../utils/toast';

// ── Types ────────────────────────────────────────────────────────────────────

interface CanaryToken {
  id: string;
  label: string;
  tokenType: 'docx' | 'url';
  outputPath: string;
  beaconUrl: string;
  createdAt: string;
}

interface CanaryHit {
  tokenId: string;
  label: string;
  remoteAddr: string;
  userAgent: string | null;
  firedAt: string;
}

interface ListenerStatus {
  running: boolean;
  port: number | null;
}

const TOKEN_TYPES: { label: string; value: string }[] = [
  { label: 'Word document (.docx)', value: 'docx' },
  { label: 'URL shortcut (.url)', value: 'url' },
];

const RECENT_CAP = 50;

function formatRelative(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const diffSec = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const m = Math.floor(diffSec / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

// ── Section ──────────────────────────────────────────────────────────────────

export default function CanaryTokensSection() {
  // Listener
  const [listenerStatus, setListenerStatus] = useState<ListenerStatus>({ running: false, port: null });
  const [portInput, setPortInput] = useState('8765');
  const [listenerBusy, setListenerBusy] = useState(false);

  // Token generation
  const [tokenType, setTokenType] = useState<'docx' | 'url'>('docx');
  const [tokenLabel, setTokenLabel] = useState('');
  const [genBusy, setGenBusy] = useState(false);

  // Token list
  const [tokens, setTokens] = useState<CanaryToken[]>([]);
  const [tokensBusy, setTokensBusy] = useState(false);

  // Recent hits
  const [recent, setRecent] = useState<CanaryHit[]>([]);
  const [clearBusy, setClearBusy] = useState(false);

  // General error
  const [error, setError] = useState<string | null>(null);

  // ── Data fetch ─────────────────────────────────────────────────────────────

  const refreshStatus = useCallback(async () => {
    try {
      const s = await invoke<ListenerStatus>('canary_listener_status');
      setListenerStatus(s ?? { running: false, port: null });
    } catch (e) {
      setListenerStatus({ running: false, port: null });
    }
  }, []);

  const refreshTokens = useCallback(async () => {
    setTokensBusy(true);
    try {
      const list = await invoke<CanaryToken[]>('list_canaries');
      setTokens(Array.isArray(list) ? list : []);
    } catch (e) {
      setError(String(e));
    } finally {
      setTokensBusy(false);
    }
  }, []);

  const refreshRecent = useCallback(async () => {
    try {
      const hits = await invoke<CanaryHit[]>('get_canary_recent');
      setRecent(Array.isArray(hits) ? hits : []);
    } catch {
      // Non-fatal — listener may not be running yet.
    }
  }, []);

  // ── Mount ──────────────────────────────────────────────────────────────────

  useEffect(() => {
    void refreshStatus();
    void refreshTokens();
    void refreshRecent();

    // Live hit updates from the Pro sidecar.
    let unlisten: UnlistenFn | null = null;
    (async () => {
      unlisten = await listen<CanaryHit>('canary-token-fired', (e) => {
        setRecent((prev) => [e.payload, ...prev].slice(0, RECENT_CAP));
        void showSuccess(`Canary fired: ${e.payload.label || e.payload.tokenId}`);
      });
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, [refreshStatus, refreshTokens, refreshRecent]);

  // ── Listener controls ──────────────────────────────────────────────────────

  const startListener = useCallback(async () => {
    setListenerBusy(true);
    setError(null);
    try {
      const port = parseInt(portInput, 10);
      if (Number.isNaN(port) || port < 1024 || port > 65535) {
        setError('Port must be 1024–65535.');
        return;
      }
      await invoke('start_canary_listener', { port });
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setListenerBusy(false);
    }
  }, [portInput, refreshStatus]);

  const stopListener = useCallback(async () => {
    setListenerBusy(true);
    setError(null);
    try {
      await invoke('stop_canary_listener');
      await refreshStatus();
    } catch (e) {
      setError(String(e));
    } finally {
      setListenerBusy(false);
    }
  }, [refreshStatus]);

  // ── Generate token ─────────────────────────────────────────────────────────

  const generateToken = useCallback(async () => {
    if (!tokenLabel.trim()) {
      setError('Label is required.');
      return;
    }
    if (!listenerStatus.running) {
      setError('Start the listener before generating a token.');
      return;
    }

    const ext = tokenType === 'docx' ? 'docx' : 'url';
    const outputPath = await save({
      title: 'Save canary token',
      defaultPath: `${tokenLabel.trim().replace(/[^a-z0-9_-]/gi, '_')}.${ext}`,
      filters: [
        {
          name: tokenType === 'docx' ? 'Word Document' : 'URL Shortcut',
          extensions: [ext],
        },
      ],
    });
    if (!outputPath) return; // user cancelled

    setGenBusy(true);
    setError(null);
    try {
      await invoke('generate_canary', {
        tokenType,
        label: tokenLabel.trim(),
        outputPath,
      });
      setTokenLabel('');
      await refreshTokens();
      void showSuccess(`Canary token saved to ${outputPath}`);
    } catch (e) {
      showError(String(e));
      setError(String(e));
    } finally {
      setGenBusy(false);
    }
  }, [tokenType, tokenLabel, listenerStatus.running, refreshTokens]);

  // ── Delete token ───────────────────────────────────────────────────────────

  const deleteToken = useCallback(
    async (id: string) => {
      try {
        await invoke('delete_canary', { tokenId: id });
        setTokens((prev) => prev.filter((t) => t.id !== id));
      } catch (e) {
        showError(String(e));
      }
    },
    [],
  );

  // ── Clear recent ───────────────────────────────────────────────────────────

  const clearRecent = useCallback(async () => {
    setClearBusy(true);
    try {
      await invoke('clear_canary_recent');
      setRecent([]);
    } catch (e) {
      showError(String(e));
    } finally {
      setClearBusy(false);
    }
  }, []);

  // ── Header pill ───────────────────────────────────────────────────────────

  const hitCount = recent.length;
  const pillIntent: Intent = hitCount > 0 ? 'danger' : listenerStatus.running ? 'success' : 'none';
  const headerRight = (
    <Tag minimal intent={pillIntent} className="font-mono">
      {hitCount > 0
        ? `${hitCount} HIT${hitCount === 1 ? '' : 'S'}`
        : listenerStatus.running
          ? `LISTENING :${listenerStatus.port}`
          : 'OFF'}
    </Tag>
  );

  // ── Render ─────────────────────────────────────────────────────────────────

  return (
    <SectionCard title="Canary Tokens" icon="feed" headerRight={headerRight}>
      <div className="flex flex-col gap-4">
        <div className="text-sm opacity-80">
          Plant a traceable artifact — a Word doc or URL shortcut — that beacons
          home when opened. The local HTTP listener records the hit and alerts you.
          v1 supports self-hosted HTTP beacons; DNS canaries are v2.
        </div>

        {/* ── Listener controls ── */}
        <div className="flex flex-col gap-2">
          <div className="text-xs font-semibold uppercase tracking-widest opacity-60">
            HTTP Listener
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            <InputGroup
              type="number"
              value={portInput}
              onChange={(e) => setPortInput(e.target.value)}
              placeholder="Port"
              disabled={listenerStatus.running || listenerBusy}
              style={{ width: 90 }}
              aria-label="Listener port"
            />
            {listenerStatus.running ? (
              <Button
                icon="stop"
                intent="danger"
                small
                disabled={listenerBusy}
                onClick={() => void stopListener()}
              >
                Stop
              </Button>
            ) : (
              <Button
                icon="play"
                intent="success"
                small
                disabled={listenerBusy}
                onClick={() => void startListener()}
              >
                Start
              </Button>
            )}
            {listenerBusy && <Spinner size={14} />}
            <span className="text-xs opacity-60 font-mono">
              {listenerStatus.running
                ? `listening on :${listenerStatus.port}`
                : 'not running'}
            </span>
          </div>
        </div>

        {/* ── Token generation ── */}
        <div className="flex flex-col gap-2">
          <div className="text-xs font-semibold uppercase tracking-widest opacity-60">
            Generate Token
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            <HTMLSelect
              options={TOKEN_TYPES}
              value={tokenType}
              onChange={(e) => setTokenType(e.target.value as 'docx' | 'url')}
              disabled={genBusy}
            />
            <InputGroup
              value={tokenLabel}
              onChange={(e) => setTokenLabel(e.target.value)}
              placeholder="Label (e.g. Budget Q4)"
              disabled={genBusy}
              style={{ minWidth: 180 }}
              aria-label="Token label"
            />
            <Button
              icon="document"
              small
              disabled={genBusy || !tokenLabel.trim()}
              loading={genBusy}
              onClick={() => void generateToken()}
            >
              Save token…
            </Button>
          </div>
        </div>

        {error && <div className="font-mono text-sm text-[var(--color-danger)]">{error}</div>}

        {/* ── Token list ── */}
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div className="text-xs font-semibold uppercase tracking-widest opacity-60">
              Tokens ({tokens.length})
            </div>
            <Button
              icon="refresh"
              minimal
              small
              onClick={() => void refreshTokens()}
              disabled={tokensBusy}
            >
              Refresh
            </Button>
          </div>
          {tokensBusy && <Spinner size={14} />}
          {!tokensBusy && tokens.length === 0 && (
            <div className="text-sm opacity-60">No tokens generated yet.</div>
          )}
          {tokens.length > 0 && (
            <div className="flex flex-col gap-1 max-h-[220px] overflow-y-auto">
              {tokens.map((t) => (
                <div
                  key={t.id}
                  className="flex items-center gap-2 px-3 py-1.5 rounded-[var(--r-lg)] border border-[var(--border)] bg-[var(--surface-2)]"
                >
                  <Tag minimal className="font-mono shrink-0">
                    {t.tokenType}
                  </Tag>
                  <div className="flex flex-col flex-1 min-w-0">
                    <span className="text-[13px] truncate">{t.label}</span>
                    <span
                      className="font-mono text-[10px] opacity-50 truncate"
                      title={t.outputPath}
                    >
                      {t.outputPath}
                    </span>
                  </div>
                  <span className="font-mono text-[10px] opacity-40 shrink-0">
                    {formatRelative(t.createdAt)}
                  </span>
                  <Button
                    icon="trash"
                    minimal
                    small
                    intent="danger"
                    onClick={() => void deleteToken(t.id)}
                    aria-label={`Delete ${t.label}`}
                  />
                </div>
              ))}
            </div>
          )}
        </div>

        {/* ── Recent hits ── */}
        <div className="flex flex-col gap-2">
          <div className="flex items-center justify-between">
            <div className="text-xs font-semibold uppercase tracking-widest opacity-60">
              Recent Hits ({recent.length})
            </div>
            <Button
              icon="trash"
              minimal
              small
              disabled={clearBusy || recent.length === 0}
              onClick={() => void clearRecent()}
            >
              Clear
            </Button>
          </div>
          {recent.length === 0 && (
            <div className="text-sm opacity-60">No hits recorded.</div>
          )}
          {recent.length > 0 && (
            <div className="flex flex-col gap-1 max-h-[220px] overflow-y-auto">
              {recent.map((h, i) => (
                <div
                  key={`${h.tokenId}-${h.firedAt}-${i}`}
                  className="flex items-start gap-2 px-3 py-1.5 rounded-[var(--r-lg)] border border-[var(--color-danger,#f87171)]/30 bg-[var(--color-danger,#f87171)]/5"
                >
                  <Tag minimal intent="danger" className="font-mono shrink-0">
                    HIT
                  </Tag>
                  <div className="flex flex-col flex-1 min-w-0">
                    <span className="text-[13px] truncate">{h.label || h.tokenId}</span>
                    <span className="font-mono text-[10px] opacity-60 truncate">
                      {h.remoteAddr}
                      {h.userAgent ? ` · ${h.userAgent}` : ''}
                    </span>
                  </div>
                  <span className="font-mono text-[10px] opacity-40 shrink-0">
                    {formatRelative(h.firedAt)}
                  </span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </SectionCard>
  );
}
