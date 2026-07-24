// src/panels/privacy/StartupPinConfig.tsx
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppState } from "../../context/AppContext";
import { showSuccess, showError } from "../../utils/toast";

const MODES = [
  { mode: "real",    label: "Real PIN",    desc: "Opens full configuration" },
  { mode: "decoy",   label: "Decoy PIN",   desc: "Shows blank-slate app, nothing wiped" },
  { mode: "destroy", label: "Destroy PIN", desc: "Wipes all WC traces then exits" },
] as const;

type Mode = "real" | "decoy" | "destroy";

function validatePinInput(pin: string): string | null {
  if (pin.length < 4) return "PIN must be at least 4 digits";
  if (!/^[0-9]+$/.test(pin)) return "PIN must contain only digits — the calculator can't display letters";
  if (pin.startsWith("0")) return "PIN can't start with 0 — the calculator drops leading zeros";
  if (/^(\d)\1+$/.test(pin)) return "PIN can't use the same digit repeated";

  const digits = [...pin].map((d) => Number(d));
  const ascending = digits.every((d, i) => i === 0 || d === digits[i - 1] + 1);
  const descending = digits.every((d, i) => i === 0 || d === digits[i - 1] - 1);
  if (ascending || descending) return "PIN can't be a simple ascending or descending sequence";

  return null;
}

export default function StartupPinConfig() {
  const { appSettings, refreshSettings } = useAppState();
  const [inputs, setInputs] = useState<Record<Mode, string>>({ real: "", decoy: "", destroy: "" });

  // Which PINs are currently set, derived live from the stored hashes
  // (the plaintext is never available — only "is a hash present").
  const sp = appSettings?.ideal?.privacy?.startupPin;
  const configured: Record<Mode, boolean> = {
    real: !!sp?.realHash,
    decoy: !!sp?.decoyHash,
    destroy: !!sp?.destroyHash,
  };
  // The calculator gate only engages once a Real PIN exists — a decoy/destroy
  // PIN alone would otherwise lock the user out of the full app.
  const gateArmed = configured.real && sp?.enabled !== false;

  const save = async (mode: Mode) => {
    const pin = inputs[mode].trim();
    // Mirror the Rust gate (startup_auth::validate_pin_enterable): the PIN must
    // be reproducible on the calculator display, or the user locks themselves out.
    const validationError = validatePinInput(pin);
    if (validationError) { showError(validationError); return; }
    const duplicate = MODES.find((m) => m.mode !== mode && inputs[m.mode].trim() === pin);
    if (duplicate) { showError(`PIN must be different from ${duplicate.label}`); return; }
    try {
      await invoke("register_startup_pin", { mode, plaintext: pin });
      setInputs((i) => ({ ...i, [mode]: "" }));
      await refreshSettings();
      showSuccess(`${MODES.find((m) => m.mode === mode)!.label} saved`);
    } catch (e) { showError(String(e)); }
  };

  const clear = async (mode: Mode) => {
    try {
      await invoke("clear_startup_pin", { mode });
      await refreshSettings();
      showSuccess("PIN cleared");
    } catch (e) { showError(String(e)); }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <p style={{ margin: 0, fontSize: 12, color: "var(--color-text-muted)" }}>
        WinCommander opens as a working calculator. Type a PIN and press{" "}
        <kbd style={{ fontFamily: "var(--font-mono)", fontSize: 11 }}>=</kbd> twice to unlock.
        Only a hand-typed PIN counts — a calculation that equals it will not. Plaintext is never stored, only a one-way hash.
      </p>
      <div
        style={{
          fontSize: 12, fontWeight: 600, padding: "6px 10px",
          borderRadius: "var(--radius-md, 4px)",
          border: "1px solid var(--color-border)",
          background: gateArmed
            ? "color-mix(in srgb, var(--color-success, #4caf50) 12%, transparent)"
            : "color-mix(in srgb, var(--color-warning, #e0a800) 12%, transparent)",
          color: gateArmed ? "var(--color-success, #4caf50)" : "var(--color-warning, #e0a800)",
        }}
      >
        {gateArmed
          ? "Calculator lock is ARMED — the app opens as a calculator until a PIN is entered."
          : configured.real
            ? "Calculator lock is OFF — your saved PINs are kept. Turn the lock on above to require the Real PIN at startup."
            : "Calculator lock is OFF — set a Real PIN to enable it. A Decoy or Destroy PIN alone does nothing until a Real PIN exists."}
      </div>
      {MODES.map(({ mode, label, desc }) => (
        <div key={mode} style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 13, fontWeight: 600 }}>{label}</div>
            <div style={{ fontSize: 11, color: "var(--color-text-muted)" }}>{desc}</div>
          </div>
          <input
            type="password"
            placeholder={configured[mode] ? "●●●● set" : "PIN…"}
            value={inputs[mode]}
            inputMode="numeric"
            maxLength={12}
            onChange={(e) => {
              const value = e.target.value.replace(/\D/g, "").slice(0, 12);
              setInputs((i) => ({ ...i, [mode]: value }));
            }}
            onKeyDown={(e) => { if (e.key === "Enter") save(mode); }}
            style={{
              width: 110, padding: "5px 8px", fontSize: 13,
              background: "var(--color-bg-elevated)",
              border: "1px solid var(--color-border)",
              borderRadius: "var(--radius-md, 4px)",
              color: "var(--color-text-primary)",
              outline: "none",
            }}
          />
          <button
            onClick={() => save(mode)}
            style={{
              padding: "5px 10px", fontSize: 12, fontWeight: 600,
              background: "var(--color-accent)", color: "white",
              border: "none", borderRadius: "var(--radius-lg, 6px)", cursor: "pointer",
            }}
          >
            {configured[mode] ? "Update" : "Set"}
          </button>
          {configured[mode] && (
            <button
              onClick={() => clear(mode)}
              style={{
                padding: "5px 10px", fontSize: 12,
                background: "transparent", color: "var(--color-text-muted)",
                border: "1px solid var(--color-border)",
                borderRadius: "var(--radius-lg, 6px)", cursor: "pointer",
              }}
            >
              Clear
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
