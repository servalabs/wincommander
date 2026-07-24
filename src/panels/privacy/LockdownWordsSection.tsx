// src/panels/privacy/LockdownWordsSection.tsx
//
// Lockdown Words — Compact section for safeguards grids.
//
// Stays minimal until expanded. Stored as SHA-256 only.

import { Icon, Button, Dialog, DialogBody, DialogFooter, InputGroup, FormGroup } from "@/components/ui/bp";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { showError, showSuccess } from "../../utils/toast";
import type { CoercionPhraseEntry } from "../../types/settings";
import { PanicKeywordIntro } from "./MonitorIntros";

interface Props {
  isAdvanced: boolean;
  enabled: boolean;
  phrases: CoercionPhraseEntry[];
  onPatch: (patch: { enabled?: boolean; phrases?: CoercionPhraseEntry[] }) => void;
  /** Controlled expand for accordion behaviour in monitoring/safeguards grids. */
  expanded?: boolean;
  onExpandedChange?: (next: boolean) => void;
  /** Render without the outer card chrome (border/bg/pad) so it can sit as a
   *  block inside a shared "Triggers" card. */
  bare?: boolean;
}

export default function LockdownWordsSection({
  isAdvanced,
  enabled,
  phrases,
  onPatch,
  expanded: expandedProp,
  onExpandedChange,
  bare = false,
}: Props) {
  const [running, setRunning] = useState(false);
  const [expandedLocal, setExpandedLocal] = useState(false);
  const isControlled = expandedProp !== undefined && onExpandedChange !== undefined;
  const expanded = isControlled ? expandedProp! : expandedLocal;
  const setExpanded = (updater: boolean | ((v: boolean) => boolean)) => {
    const next = typeof updater === 'function' ? updater(expanded) : updater;
    if (isControlled) onExpandedChange!(next);
    else setExpandedLocal(next);
  };
  const [showIntro, setShowIntro] = useState(false);
  const [adding, setAdding] = useState(false);
  const [newLabel, setNewLabel] = useState("");
  const [newPhrase, setNewPhrase] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const r = await invoke<boolean>("lockdown_words_status");
        if (!cancelled) setRunning(r);
      } catch { /* ignore */ }
    };
    refresh();
    const id = setInterval(refresh, 5_000);
    return () => { cancelled = true; clearInterval(id); };
  }, []);

  const onAdd = async () => {
    if (newPhrase.length < 6) {
      showError("Phrase must be at least 6 characters.");
      return;
    }
    setSubmitting(true);
    try {
      const entry = await invoke<CoercionPhraseEntry>("register_lockdown_words", {
        label: newLabel,
        plaintext: newPhrase,
      });
      onPatch({ phrases: [...phrases, entry] });
      setNewLabel("");
      setNewPhrase("");
      setAdding(false);
      showSuccess("Lockdown word registered.");
    } catch (err) {
      showError(`${err}`);
    } finally {
      setSubmitting(false);
    }
  };

  const onRemove = (hash: string) => {
    onPatch({ phrases: phrases.filter((p) => p.hash !== hash) });
  };

  const handleRegisterEnter = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Enter" || event.shiftKey || event.defaultPrevented) return;
    event.preventDefault();
    void onAdd();
  };

  return (
    <>
    <div
      className={bare ? "lockdown-trigger-block" : "rounded-lg border p-5 transition-colors"}
      style={bare ? undefined : {
        background: enabled ? 'var(--shield-bg-running)' : 'var(--shield-bg-idle)',
        borderColor: enabled ? 'rgba(0, 160, 255, 0.25)' : 'var(--color-border)',
      }}
    >
      <div className="lockdown-trigger-head">
        <Icon icon="chat" size={14} className="lockdown-trigger-icon" />
        <span className="lockdown-trigger-title">
          {isAdvanced ? "Lockdown Words (typed)" : "Lockdown Words"}
        </span>
        {enabled && running && (
          <span className="lockdown-trigger-status">Listening</span>
        )}
        <button
          type="button"
          role="switch"
          aria-checked={enabled}
          className={`lockdown-trigger-toggle ${enabled ? 'is-on' : ''}`}
          onClick={() => onPatch({ enabled: !enabled })}
        />
      </div>
      <p className="lockdown-trigger-desc">
        Trigger a silent lockdown by typing a secret phrase anywhere.{" "}
        <button
          type="button"
          onClick={() => setShowIntro(true)}
          style={{ background: 'none', border: 'none', padding: 0, color: 'var(--color-accent)', cursor: 'pointer', font: 'inherit', textDecoration: 'underline' }}
        >
          How it works?
        </button>
      </p>

      {enabled && (
        <div className="pt-2 mt-1 border-t border-[var(--shield-inner-border)]">
          <button
            type="button"
            className="flex items-center justify-between w-full cursor-pointer hover:opacity-80 transition-opacity"
            onClick={() => setExpanded(v => !v)}
          >
            <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
              {phrases.length} registered phrases
            </span>
            <Icon
              icon={expanded ? "chevron-up" : "chevron-down"}
              size={12}
              color="var(--shield-text-muted)"
            />
          </button>

          {expanded && (
            <div className="mt-4 flex flex-col gap-3">
              <div className="flex items-center justify-between">
                <span className="text-[10px] font-medium uppercase tracking-widest text-[var(--shield-text-muted)]">
                  Phrases
                </span>
                <Button small minimal icon="plus" onClick={() => setAdding(true)}>
                  Add phrase
                </Button>
              </div>

              {phrases.length === 0 ? (
                <p className="text-[11px] text-[var(--color-warning)] italic">
                  No phrases registered. Trigger is dormant.
                </p>
              ) : (
                <div className="flex flex-col gap-1">
                  {phrases.map((p) => (
                    <div
                      key={p.hash}
                      className="flex items-center justify-between gap-2 px-3 py-1.5 rounded bg-[var(--color-bg-secondary)] border border-[var(--shield-inner-border)]"
                    >
                      <span className="flex items-center gap-2 min-w-0">
                        <Icon icon="lock" size={11} color="var(--shield-text-muted)" />
                        <span className="text-[11px] text-[var(--shield-text-subtle)] font-medium truncate">
                          {p.label}
                        </span>
                        <span className="text-[10px] text-[var(--shield-text-muted)] font-mono">
                          · {p.length} chars
                        </span>
                      </span>
                      <Button
                        small
                        minimal
                        icon="cross"
                        onClick={() => onRemove(p.hash)}
                      />
                    </div>
                  ))}
                </div>
              )}

              <Button
                small
                minimal
                icon="bug"
                onClick={async () => {
                  try {
                    await invoke("test_fire_lockdown_words");
                    showSuccess("Test event fired.");
                  } catch (err) {
                    showError(`Test failed: ${err}`);
                  }
                }}
              >
                Test trigger (no destruction)
              </Button>
            </div>
          )}
        </div>
      )}

      <Dialog
        isOpen={adding}
        title="Register lockdown word"
        onClose={() => setAdding(false)}
        canEscapeKeyClose
      >
        <DialogBody>
          <FormGroup label="Label" labelInfo="(visible to you only)">
            <InputGroup
              value={newLabel}
              onChange={(e) => setNewLabel(e.currentTarget.value)}
              placeholder="e.g. Daily standup"
              onKeyDown={handleRegisterEnter}
            />
          </FormGroup>
          <FormGroup
            label="Phrase"
            labelInfo="(min 6 chars)"
            helperText="Stored as SHA-256 only. Plaintext is discarded immediately."
          >
            <InputGroup
              value={newPhrase}
              onChange={(e) => setNewPhrase(e.currentTarget.value)}
              placeholder="e.g. she sells seashells"
              onKeyDown={handleRegisterEnter}
            />
          </FormGroup>
        </DialogBody>
        <DialogFooter>
          <Button onClick={() => setAdding(false)} disabled={submitting}>Cancel</Button>
          <Button intent="primary" onClick={onAdd} loading={submitting}>Register</Button>
        </DialogFooter>
      </Dialog>
    </div>
    <PanicKeywordIntro isOpen={showIntro} onClose={() => setShowIntro(false)} />
    </>
  );
}
