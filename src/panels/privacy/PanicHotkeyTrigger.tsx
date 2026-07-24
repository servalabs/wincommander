// src/panels/privacy/PanicHotkeyTrigger.tsx
//
// The panic-hotkey lockdown trigger, as its own compact card so all THREE
// lockdown triggers (panic hotkey, lockdown/coercion words, dead-man check-in)
// sit together under the Lockdown card's "Triggers" group — instead of the
// hotkey being buried inside the erase-configuration block.
//
// Owns the same behaviour the row had inside LockdownConfigSection: capture
// the next non-modifier keypress, persist it, and re-bind the global shortcut
// via the `update_panic_hotkey` Tauri command without an app restart.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Icon } from "../../components/ui/icon";

interface Props {
  hotkey: string;
  onSave: (combo: string) => void;
  /** Render without its own card chrome so it sits inside a shared card. */
  bare?: boolean;
}

export default function PanicHotkeyTrigger({ hotkey, onSave, bare = false }: Props) {
  const [recording, setRecording] = useState(false);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLInputElement>) => {
      e.preventDefault();
      e.stopPropagation();
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");
      const key = e.key === " " ? "Space" : e.key.length === 1 ? e.key.toUpperCase() : e.key;
      parts.push(key);
      const combo = parts.join("+");
      setRecording(false);
      onSave(combo);
      invoke("update_panic_hotkey", { hotkey: combo }).catch(() => {
        /* best-effort re-bind; settings are still saved */
      });
    },
    [onSave],
  );

  // Keep the global shortcut bound to the persisted value across sessions.
  useEffect(() => {
    if (hotkey) invoke("update_panic_hotkey", { hotkey }).catch(() => { });
  }, [hotkey]);

  return (
    <div
      className={bare ? "lockdown-trigger-block" : "rounded-lg border p-4"}
      style={bare ? undefined : { background: "var(--shield-bg-idle)", borderColor: "var(--color-border)" }}
    >
      <div className="lockdown-trigger-head">
        <Icon icon="key" size={14} className="lockdown-trigger-icon" />
        <span className="lockdown-trigger-title">Panic Hotkey</span>
        {recording ? (
          <input
            autoFocus
            readOnly
            placeholder="Press keys…"
            onKeyDown={handleKeyDown}
            onBlur={() => setRecording(false)}
            className="lockdown-trigger-input text-center"
            style={{ width: 120, borderColor: "var(--color-accent)" }}
          />
        ) : (
          <button
            type="button"
            onClick={() => setRecording(true)}
            title="Click to change"
            className="lockdown-trigger-input"
            style={{ cursor: "pointer", fontWeight: 600 }}
          >
            {hotkey || "Ctrl+Shift+Q"}
          </button>
        )}
      </div>
      <p className="lockdown-trigger-desc">
        Global shortcut that arms a 4-second lockdown countdown.
      </p>
    </div>
  );
}
