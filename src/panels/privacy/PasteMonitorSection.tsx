// src/panels/privacy/PasteMonitorSection.tsx
//
// F-1 Clipboard guard — Privacy panel section card.
//
// Progressive disclosure: the default view is a single ON/OFF + status
// pill, identical to other always-on watcher cards in the panel.
// Clicking "Configure" expands a sub-panel with:
//
//   - 6 category checkboxes (Cloud APIs, AI APIs, developer tools,
//     Payments & Comms, Keys & Crypto, Personal Data) so the user can
//     mute groups they don't care about without disabling the whole
//     watcher.
//   - Snooze buttons (15 min / 1 hour / cancel) for temporary mute
//     during legitimate credential handling.
//   - "Caught N this session" feedback (last 10 detections, in-memory
//     ring buffer on the Rust side — no clipboard content stored).
//
// Categories are sourced from `appSettings.ideal.privacy.clipboard
// .pasteMonitorCategories` and patched back via `patchAppSettings`.
// The runtime authority that the Rust watcher reads from is synced by
// the global `usePasteMonitor` hook in App.tsx; this component only
// touches settings + Tauri commands for snooze / recent.

import { Switch, Icon, Button, Tag, CheckboxControl } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import { PasteMonitorIntro } from "./MonitorIntros";
import {
  resolveCategories,
  DEFAULT_PASTE_MONITOR_CRYPTO_SWAP_ENABLED,
  DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ENABLED,
  DEFAULT_PASTE_MONITOR_AUTO_CLEAR_SECONDS,
  DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ON_LOCK,
  type PasteMonitorRustCategories,
} from "../../hooks/usePasteMonitor";
import useEntitlements from "../../hooks/useEntitlements";
import type { PasteMonitorCategories } from "../../types/settings";
import SectionCard from "../../components/shared/SectionCard";
import { useAppConfirm } from "../../components/shared/AppConfirmDialog";

interface DetectionRow {
  pattern: string;
  /** "warning" or "danger" — danger entries get a red accent in the log. */
  severity?: string;
  detected_at: string;
}

interface CategoryDef {
  key: keyof PasteMonitorRustCategories;
  title: string;
  description: string;
}

// Order matters — drives the rendering order in the configure panel.
// Malicious commands listed FIRST: it's the highest-impact category
// (defends against active social-engineering attacks at the moment of
// pasting) and the one users should leave on no matter what.
const CATEGORIES: CategoryDef[] = [
  { key: "maliciousCommand", title: "Suspicious Commands", description: "ClickFix / pastejacking — encoded PowerShell, mshta web payloads, iex-irm, curl-pipe-shell" },
  { key: "unicode",          title: "Unicode Anomalies",   description: "Homoglyph URLs (Cyrillic 'а' in paypal.com), bidi override spoofs, zero-width chars hidden in code" },
  { key: "cloudApi",         title: "Cloud APIs",          description: "AWS, Google, SendGrid, Mailgun, Twilio, DB URLs" },
  { key: "aiApi",            title: "AI APIs",             description: "OpenAI, Anthropic" },
  { key: "devTools",         title: "Source Control",      description: "GitHub PAT, NPM tokens" },
  { key: "paymentComms",     title: "Payments & Chat",     description: "Stripe, Slack, Discord" },
  { key: "keysAndCrypto",    title: "Keys & Crypto",       description: "PEM, OpenSSH, JWT, Bitcoin WIF" },
  { key: "personalData",     title: "Personal Data",       description: "Credit card numbers (Luhn-verified)" },
];

interface Props {
  isAdvanced: boolean;
  searchQuery: string;
  enabled: boolean;
  categories: PasteMonitorCategories | null | undefined;
  /** Paid: detect clipboard-hijack malware that swaps a copied crypto
   *  address. Omitted/null = use default (on). Optional so existing
   *  callers (intelligence panel) don't need to be updated in lockstep. */
  cryptoSwapEnabled?: boolean | null | undefined;
  /** Paid: auto-clear sensitive clipboard content after a detection. */
  autoClearEnabled?: boolean | null | undefined;
  /** Paid: how many seconds to wait before auto-clearing. */
  autoClearSeconds?: number | null | undefined;
  /** Free: erase the clipboard when the workstation locks. */
  autoClearOnLock?: boolean | null | undefined;
  onPatchClipboard: (patch: Record<string, unknown>) => void;
  /** Controlled expand for accordion behaviour in monitoring/safeguards grids. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
}

export default function PasteMonitorSection({
  isAdvanced,
  searchQuery,
  enabled,
  categories,
  cryptoSwapEnabled,
  autoClearEnabled,
  autoClearSeconds,
  autoClearOnLock,
  onPatchClipboard,
  expanded: expandedProp,
  onExpandedChange,
}: Props) {
  const requestConfirm = useAppConfirm();
  const { canUse } = useEntitlements();
  const isPaid = canUse('paid');
  const cryptoSwap = cryptoSwapEnabled ?? DEFAULT_PASTE_MONITOR_CRYPTO_SWAP_ENABLED;
  const autoClear = autoClearEnabled ?? DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ENABLED;
  const autoClearSecs = autoClearSeconds ?? DEFAULT_PASTE_MONITOR_AUTO_CLEAR_SECONDS;
  const clearOnLock = autoClearOnLock ?? DEFAULT_PASTE_MONITOR_AUTO_CLEAR_ON_LOCK;
  const isControlled = expandedProp !== undefined && onExpandedChange !== undefined;
  const [expandedInternal, setExpandedInternal] = useState(false);
  const expanded = isControlled ? expandedProp : expandedInternal;
  const setExpanded = (next: boolean) => {
    if (isControlled) onExpandedChange(next);
    else setExpandedInternal(next);
  };
  const [showIntro, setShowIntro] = useState(false);
  const [snoozeSecsLeft, setSnoozeSecsLeft] = useState(0);
  const [recent, setRecent] = useState<DetectionRow[]>([]);

  const cats = resolveCategories(categories);
  const enabledCount = Object.values(cats).filter(Boolean).length;
  const isSnoozed = snoozeSecsLeft > 0;

  const refreshSnooze = useCallback(async () => {
    try {
      const secs = await invoke<number>("paste_monitor_snooze_remaining");
      setSnoozeSecsLeft(secs);
    } catch {
      // Watcher not registered yet; leave at 0.
    }
  }, []);

  const refreshRecent = useCallback(async () => {
    try {
      const rows = await invoke<DetectionRow[]>("get_paste_monitor_recent");
      setRecent(rows);
    } catch {
      setRecent([]);
    }
  }, []);

  // Poll snooze + recent only when the user has the section expanded —
  // when collapsed we only need the count, refreshed on expand.
  useEffect(() => {
    if (!expanded) return;
    refreshSnooze();
    refreshRecent();
    const id = setInterval(() => {
      refreshSnooze();
      refreshRecent();
    }, 5000);
    return () => clearInterval(id);
  }, [expanded, refreshSnooze, refreshRecent]);

  // Always keep the "caught N" count fresh-ish even when collapsed, so
  // the badge isn't stale when the panel re-mounts.
  useEffect(() => {
    if (!enabled) {
      setRecent([]);
      return;
    }
    refreshRecent();
    const id = setInterval(refreshRecent, 30_000);
    return () => clearInterval(id);
  }, [enabled, refreshRecent]);

  const onToggleCategory = (key: keyof PasteMonitorRustCategories, val: boolean) => {
    onPatchClipboard({ pasteMonitorCategories: { [key]: val } });
  };

  const onSnooze = async (minutes: number) => {
    try {
      await invoke("snooze_paste_monitor", { minutes });
    } catch (err) {
      console.warn("[PasteMonitor] snooze failed:", err);
    }
    refreshSnooze();
  };

  const onCancelSnooze = async () => {
    try {
      await invoke("cancel_paste_monitor_snooze");
    } catch (err) {
      console.warn("[PasteMonitor] cancel snooze failed:", err);
    }
    refreshSnooze();
  };

  const onClearRecent = async () => {
    const accepted = await requestConfirm({
      title: "Clear recent clipboard detections?",
      description: "This removes the recent detection metadata recorded for this app session. Clipboard content is not stored here.",
      confirmLabel: "Clear detections",
    });
    if (!accepted) return;
    try {
      await invoke("clear_paste_monitor_recent");
    } catch (err) {
      console.warn("[PasteMonitor] clear recent failed:", err);
    }
    refreshRecent();
  };

  if (searchQuery.trim()) return null;

  // Header status tag — three states: snoozed (warning), watching (success),
  // idle (muted). Mirrors the icon + headerRight convention used by the
  // other monitor cards (Canary, USB, Print).
  const headerRight = (
    <Tag
      minimal
      intent={enabled && isSnoozed ? 'warning' : enabled ? 'success' : 'none'}
      className="font-mono"
    >
      {enabled && isSnoozed ? `SNOOZED · ${formatMinSec(snoozeSecsLeft)}` : enabled ? 'WATCHING' : 'OFF'}
    </Tag>
  );

  return (
    <>
      <SectionCard
        title={isAdvanced ? "Clipboard secret guard" : "Credential paste guard"}
        icon="clipboard"
        headerRight={headerRight}
        armed={enabled}
      >
        <div className="flex flex-col">
        <div className="flex items-start justify-between gap-4">
          <div className="flex flex-col gap-1 min-w-0">
            <div className="flex items-center gap-2 flex-wrap">
              <button
                type="button"
                onClick={() => setShowIntro(true)}
                className="text-[10px] px-1.5 py-0.5 rounded border border-[var(--color-accent)]/30 text-[var(--color-accent)] hover:bg-[var(--color-accent)]/10 transition-colors"
              >
                How it works?
              </button>
              {enabled && recent.length > 0 && (
                <span
                  className="text-[10px] px-2 py-0.5 rounded bg-[var(--color-accent)]/10 text-[var(--color-accent)] border border-[var(--color-accent)]/25 flex-shrink-0 cursor-default"
                  title="Recent detections this session"
                >
                  Caught {recent.length}
                </span>
              )}
            </div>
            <p className="text-xs text-[var(--shield-text-subtle)] text-pretty max-w-[320px]">
              {isAdvanced
                ? "Watches clipboard for credential patterns. Toasts on match. Clipboard contents never leave this machine."
                : "Warns you when you copy what looks like a password or secret."}
            </p>
            {enabled && enabledCount < CATEGORIES.length && (
              <p className="text-[11px] text-[var(--shield-text-muted)] mt-1">
                Watching {enabledCount} of {CATEGORIES.length} categories
              </p>
            )}
          </div>
          <Switch
            checked={enabled}
            onChange={(e) => {
              onPatchClipboard({ pasteMonitorEnabled: e.currentTarget.checked });
            }}
          />
        </div>

        {/* Configure (progressive disclosure) — only relevant when ON */}
        {enabled && (
          <div className="mt-4 pt-4 border-t border-[var(--shield-inner-border)]">
            <button
              type="button"
              className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
              onClick={() => setExpanded(!expanded)}
            >
              <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                Configure
              </span>
              <Icon
                icon={expanded ? "chevron-up" : "chevron-down"}
                size={12}
                color="var(--shield-text-muted)"
              />
            </button>

            {expanded && (
              <div className="mt-4 flex flex-col gap-5">
                {/* Categories */}
                <div className="flex flex-col gap-2">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    Watch for
                  </span>
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                    {CATEGORIES.map((c) => (
                      <div
                        key={c.key}
                        onClick={() => onToggleCategory(c.key, !cats[c.key])}
                        className="flex cursor-pointer items-start gap-2.5 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)] px-3 py-2 transition-colors hover:border-[var(--color-accent)]/40"
                        style={{ userSelect: 'none' }}
                      >
                        <CheckboxControl
                          checked={cats[c.key]}
                          onChange={(event) => onToggleCategory(c.key, event.currentTarget.checked)}
                          onClick={(event) => event.stopPropagation()}
                          ariaLabel={`Watch for ${c.title}`}
                          className="mt-0.5"
                        />
                        <div className="flex flex-col gap-0.5 min-w-0">
                          <span className="text-[11px] font-medium text-[var(--shield-text-subtle)] leading-tight">
                            {c.title}
                          </span>
                          <span className="text-[10px] text-[var(--shield-text-muted)] leading-tight">
                            {c.description}
                          </span>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>

                {/* Pro protections — crypto-swap detection + auto-clear */}
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                      Pro protections
                    </span>
                    {!isPaid && (
                      <span className="text-[9px] font-mono font-bold tracking-widest text-[var(--color-text-muted)] border border-[var(--color-border)] rounded px-1.5 py-0.5">
                        🔒 PRO
                      </span>
                    )}
                  </div>

                  {/* Crypto-swap row */}
                  <div
                    className={`flex items-start justify-between gap-3 px-3 py-2 rounded border bg-[var(--color-bg-secondary)] ${isPaid ? 'border-[var(--shield-inner-border)]' : 'border-[var(--color-border)] opacity-60'}`}
                    style={{ userSelect: 'none' }}
                  >
                    <div className="flex flex-col gap-0.5 min-w-0">
                      <span className="text-[11px] font-medium text-[var(--shield-text-subtle)] leading-tight">
                        Crypto address-swap detection
                      </span>
                      <span className="text-[10px] text-[var(--shield-text-muted)] leading-tight">
                        Catches clipboard-hijack malware that silently overwrites a copied wallet address with the attacker's.
                      </span>
                    </div>
                    <Switch
                      checked={isPaid && cryptoSwap}
                      disabled={!isPaid}
                      onChange={(e) => onPatchClipboard({ pasteMonitorCryptoSwapEnabled: e.currentTarget.checked })}
                      style={{ marginBottom: 0 }}
                    />
                  </div>

                  {/* Auto-clear row */}
                  <div
                    className={`flex flex-col gap-2 px-3 py-2 rounded border bg-[var(--color-bg-secondary)] ${isPaid ? 'border-[var(--shield-inner-border)]' : 'border-[var(--color-border)] opacity-60'}`}
                    style={{ userSelect: 'none' }}
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="flex flex-col gap-0.5 min-w-0">
                        <span className="text-[11px] font-medium text-[var(--shield-text-subtle)] leading-tight">
                          Auto-clear sensitive clipboard
                        </span>
                        <span className="text-[10px] text-[var(--shield-text-muted)] leading-tight">
                          Clear the clipboard {autoClearSecs}s after a detection — only if you haven't copied anything else.
                        </span>
                      </div>
                      <Switch
                        checked={isPaid && autoClear}
                        disabled={!isPaid}
                        onChange={(e) => onPatchClipboard({ pasteMonitorAutoClearEnabled: e.currentTarget.checked })}
                        style={{ marginBottom: 0 }}
                      />
                    </div>
                    {isPaid && autoClear && (
                      <div className="flex items-center gap-3 mt-1">
                        <span className="text-[10px] text-[var(--shield-text-muted)] font-mono w-12 shrink-0">
                          {autoClearSecs}s
                        </span>
                        <input
                          type="range"
                          min={5}
                          max={300}
                          step={5}
                          value={autoClearSecs}
                          onChange={(e) => onPatchClipboard({ pasteMonitorAutoClearSeconds: parseInt(e.currentTarget.value, 10) })}
                          className="flex-1 accent-[var(--color-accent)]"
                        />
                        <span className="text-[10px] text-[var(--shield-text-muted)] font-mono shrink-0">
                          5s … 5m
                        </span>
                      </div>
                    )}
                  </div>
                </div>

                {/* Clear on lock (free-tier) */}
                <div className="flex flex-col gap-2">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    Lock screen
                  </span>
                  <div
                    className="flex items-start justify-between gap-3 px-3 py-2 rounded border border-[var(--shield-inner-border)] bg-[var(--color-bg-secondary)]"
                    style={{ userSelect: 'none' }}
                  >
                    <div className="flex flex-col gap-0.5 min-w-0">
                      <span className="text-[11px] font-medium text-[var(--shield-text-subtle)] leading-tight">
                        Clear clipboard when computer locks
                      </span>
                      <span className="text-[10px] text-[var(--shield-text-muted)] leading-tight">
                        Erases the clipboard the moment the screen locks (Win+L). Fires on the unlock→lock transition only.
                      </span>
                    </div>
                    <Switch
                      checked={clearOnLock}
                      onChange={(e) => onPatchClipboard({ pasteMonitorAutoClearOnLock: e.currentTarget.checked })}
                      style={{ marginBottom: 0 }}
                    />
                  </div>
                </div>

                {/* Snooze */}
                <div className="flex flex-col gap-2">
                  <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                    Snooze
                  </span>
                  <div className="flex items-center gap-2 flex-wrap">
                    {isSnoozed ? (
                      <>
                        <span className="text-[11px] text-[var(--color-warning)] font-mono">
                          Muted · {formatMinSec(snoozeSecsLeft)} left
                        </span>
                        <Button small minimal onClick={onCancelSnooze}>
                          Resume now
                        </Button>
                      </>
                    ) : (
                      <>
                        <Button small minimal onClick={() => onSnooze(15)}>
                          15 min
                        </Button>
                        <Button small minimal onClick={() => onSnooze(60)}>
                          1 hour
                        </Button>
                        <span className="text-[10px] text-[var(--shield-text-muted)] ml-1">
                          Mute when you're handling credentials on purpose.
                        </span>
                      </>
                    )}
                  </div>
                </div>

                {/* Recent detections */}
                {recent.length > 0 && (
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                        Recent ({recent.length})
                      </span>
                      <Button small minimal onClick={onClearRecent}>
                        Clear
                      </Button>
                    </div>
                    <div className="flex flex-col gap-1 max-h-[160px] overflow-y-auto">
                      {[...recent].reverse().map((r, i) => {
                        const isDanger = r.severity === "danger";
                        return (
                          <div
                            key={`${r.detected_at}-${i}`}
                            className="flex items-center justify-between px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border"
                            style={{
                              borderColor: isDanger
                                ? 'var(--color-danger, #f87171)'
                                : 'var(--shield-inner-border)',
                            }}
                          >
                            <span className="flex items-center gap-1.5 min-w-0">
                              {isDanger && (
                                <span
                                  className="text-[10px] px-1.5 py-0.5 rounded bg-[var(--color-danger)]/15 text-[var(--color-danger,#f87171)] border border-[var(--color-danger,#f87171)]/30 flex-shrink-0 font-mono"
                                  title="Suspicious command — likely malware"
                                >
                                  !
                                </span>
                              )}
                              <span className="text-[11px] text-[var(--shield-text-subtle)] font-medium truncate">
                                {r.pattern}
                              </span>
                            </span>
                            <span className="text-[10px] text-[var(--shield-text-muted)] font-mono flex-shrink-0">
                              {formatRelative(r.detected_at)}
                            </span>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>
            )}
          </div>
        )}
        </div>
      </SectionCard>
      <PasteMonitorIntro isOpen={showIntro} onClose={() => setShowIntro(false)} />
    </>
  );
}

function formatMinSec(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  if (m === 0) return `${s}s`;
  return `${m}m ${String(s).padStart(2, '0')}s`;
}

function formatRelative(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const diffMs = Date.now() - t;
  const diffSec = Math.max(0, Math.round(diffMs / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const m = Math.floor(diffSec / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}
