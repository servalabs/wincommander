import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AuthMode } from "../../context/AuthModeContext";
import { appIcons } from "@/assets";
import "./CalculatorGate.css";

// ── Types ─────────────────────────────────────────────────────────

type Op = "+" | "-" | "×" | "÷" | null;
type MemoryOp = "MC" | "MR" | "M+" | "M-" | "MS";

interface CalcState {
  display:            string;
  expression:         string;
  operand:            number | null;
  op:                 Op;
  waitingForOperand:  boolean;
  justEvaled:         boolean;
  error:              string | null;
  // PIN-gate hardening: `typed` is true only while the display is a value the
  // user keyed in digit-by-digit — any operator, function, recall, or computed
  // result clears it, so a sum that happens to equal a PIN can never unlock.
  typed:              boolean;
}

interface HistoryEntry {
  expression: string;
  result:     string;
}

const INITIAL: CalcState = {
  display: "0", expression: "", operand: null,
  op: null, waitingForOperand: false, justEvaled: false, error: null,
  typed: false,
};

// ── Arithmetic ───────────────────────────────────────────────────

function compute(a: number, b: number, op: Op): number | "ERR" {
  switch (op) {
    case "+": return a + b;
    case "-": return a - b;
    case "×": return a * b;
    case "÷": return b === 0 ? "ERR" : a / b;
    default:  return b;
  }
}

function fmt(n: number): string {
  if (!isFinite(n)) return "Overflow";
  return parseFloat(n.toPrecision(12)).toString();
}

// Indian number formatting: last group of 3, then groups of 2.
// e.g. 1234567 → "12,34,567"  |  1234567.89 → "12,34,567.89"
// Non-numeric strings (errors) are returned as-is.
function indianFormat(raw: string): string {
  if (!/^-?[\d.]+$/.test(raw)) return raw;
  const neg = raw.startsWith('-');
  const abs = neg ? raw.slice(1) : raw;
  const dotIdx = abs.indexOf('.');
  const intPart = dotIdx >= 0 ? abs.slice(0, dotIdx) : abs;
  const decPart = dotIdx >= 0 ? abs.slice(dotIdx) : ''; // includes the '.'

  let formatted = intPart;
  if (intPart.length > 3) {
    const groups: string[] = [intPart.slice(-3)];
    let head = intPart.slice(0, -3);
    while (head.length > 2) { groups.unshift(head.slice(-2)); head = head.slice(0, -2); }
    if (head.length) groups.unshift(head);
    formatted = groups.join(',');
  }
  return (neg ? '-' : '') + formatted + decPart;
}

// Render the formatted display string, wrapping the decimal separator in a
// span so it remains clearly visible when long values are squeezed.
function renderResultDisplay(formatted: string): React.ReactNode {
  const di = formatted.indexOf('.');
  if (di === -1) return formatted;
  return (
    <>
      {formatted.slice(0, di)}
      <span className="calc-result-dot">.</span>
      {formatted.slice(di + 1)}
    </>
  );
}

// ── Component ────────────────────────────────────────────────────

interface Props {
  onAuth: (mode: AuthMode) => void;
}

export default function CalculatorGate({ onAuth }: Props) {
  const [s, setS]               = useState<CalcState>(INITIAL);
  const [memory, setMemory]     = useState<number | null>(null);
  const [history, setHistory]   = useState<HistoryEntry[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const containerRef            = useRef<HTMLDivElement>(null);
  const appWindow               = getCurrentWindow();
  // Latches once a PIN check has resolved to a real/decoy/destroy outcome so
  // repeated presses while the native window transition completes are ignored.
  const authResolvedRef         = useRef(false);

  // ── PIN check on equals ──────────────────────────────────────────
  const checkPin = useCallback(async (displayValue: string) => {
    if (authResolvedRef.current) return;
    try {
      const result = await invoke<string>("verify_startup_pin", { pin: displayValue });
      if (result === "wrong" || authResolvedRef.current) return;
      authResolvedRef.current = true;
      if (result === "real" || result === "open") {
        await invoke("exit_calculator_mode", { title: "WinCommander" });
        onAuth("real");
      } else if (result === "decoy") {
        await invoke("exit_calculator_mode", { title: "WinCommander" });
        onAuth("decoy");
      } else if (result === "destroy") {
        await invoke("lockdown", { deactivateLicenseFirst: false, shutdownSystem: false });
      }
    } catch {
      // swallow — never surface auth errors visually
    }
  }, [onAuth]);

  // ── Reducer ──────────────────────────────────────────────────────

  type Action =
    | { type: "digit";      value: string }
    | { type: "dot" }
    | { type: "op";         value: Op }
    | { type: "equals" }
    | { type: "clear" }
    | { type: "clearEntry" }
    | { type: "backspace" }
    | { type: "negate" }
    | { type: "percent" }
    | { type: "reciprocal" }
    | { type: "square" }
    | { type: "sqrt" };

  const dispatch = useCallback((action: Action) => {
    setS((prev) => {
      let next = { ...prev };

      switch (action.type) {
        case "digit": {
          const d = action.value;
          if (next.error) { next = { ...INITIAL, display: d === "0" ? "0" : d, typed: true }; break; }
          if (next.justEvaled || next.waitingForOperand) {
            next = { ...next, display: d === "0" ? "0" : d, justEvaled: false, waitingForOperand: false, typed: true };
            break;
          }
          if (next.display === "0") next.display = d;
          else if (next.display.length < 15) next.display += d;
          next.typed = true;
          break;
        }
        case "dot": {
          if (next.error) break;
          // A decimal point can't appear in a PIN — disqualify from the gate.
          next.typed = false;
          if (next.justEvaled || next.waitingForOperand) {
            next = { ...next, display: "0.", justEvaled: false, waitingForOperand: false, typed: false };
            break;
          }
          if (!next.display.includes(".")) next.display += ".";
          break;
        }
        case "op": {
          if (next.error) break;
          next.typed = false;
          const cur = parseFloat(next.display);
          if (next.waitingForOperand) {
            next.op         = action.value;
            next.expression = `${fmt(next.operand!)} ${action.value}`;
          } else if (next.operand !== null && next.op && !next.justEvaled) {
            const r = compute(next.operand, cur, next.op);
            if (r === "ERR") {
              next = { ...INITIAL, display: "Cannot divide by zero", error: "ERR" };
              break;
            }
            next.operand    = r;
            next.display    = fmt(r);
            next.op         = action.value;
            next.expression = `${fmt(r)} ${action.value}`;
          } else {
            next.operand    = cur;
            next.op         = action.value;
            next.expression = `${fmt(cur)} ${action.value}`;
          }
          next.justEvaled        = false;
          next.waitingForOperand = true;
          break;
        }
        case "equals": {
          if (next.error) { next = INITIAL; break; }
          const cur = parseFloat(next.display);
          // Track whether this = press involved an arithmetic operation.
          // Only a direct "PIN =" (no pending op) should trigger auth —
          // prevents computed coincidences like 500+500=1000 from unlocking.
          const hadOp = !!(next.op && next.operand !== null);
          if (hadOp) {
            const expr = `${fmt(next.operand!)} ${next.op} ${fmt(cur)} =`;
            const r = compute(next.operand!, cur, next.op!);
            if (r === "ERR") {
              next = { ...INITIAL, display: "Cannot divide by zero", error: "ERR" };
              break;
            }
            next.expression = expr;
            next.display    = fmt(r);
            // A computed result is no longer a hand-typed PIN candidate.
            next.typed      = false;
          } else {
            next.expression = `${fmt(cur)} =`;
            // Gate trigger: only a value keyed digit-by-digit can authenticate.
            // A computed result (including one equal to a PIN) is never valid.
            if (next.typed && next.display !== "0") {
              setTimeout(() => checkPin(next.display), 0);
            }
          }
          next.op                = null;
          next.operand           = null;
          next.justEvaled        = true;
          next.waitingForOperand = false;
          break;
        }
        case "clear":      { next = INITIAL; break; }
        case "clearEntry": {
          if (next.error) { next = INITIAL; break; }
          next.display           = "0";
          next.justEvaled        = false;
          next.waitingForOperand = false;
          next.typed             = false;
          break;
        }
        case "backspace": {
          if (next.error || next.justEvaled || next.waitingForOperand) { next = INITIAL; break; }
          next.display = next.display.length > 1
            ? next.display.slice(0, -1)
            : "0";
          // Editing a typed PIN keeps it gate-eligible; a lone "0" does not.
          if (next.display === "0") next.typed = false;
          break;
        }
        case "negate": {
          if (next.error) break;
          next.display           = fmt(-parseFloat(next.display));
          next.waitingForOperand = false;
          next.typed             = false;
          break;
        }
        case "percent": {
          if (next.error) break;
          const pct  = parseFloat(next.display);
          const base = next.operand ?? 0;
          next.display           = fmt(base * pct / 100);
          next.justEvaled        = true;
          next.waitingForOperand = false;
          next.typed             = false;
          break;
        }
        case "reciprocal": {
          if (next.error) break;
          const v = parseFloat(next.display);
          if (v === 0) { next = { ...INITIAL, display: "Cannot divide by zero", error: "ERR" }; break; }
          next.display           = fmt(1 / v);
          next.expression        = `1/(${fmt(v)})`;
          next.justEvaled        = true;
          next.waitingForOperand = false;
          next.typed             = false;
          break;
        }
        case "square": {
          if (next.error) break;
          const v = parseFloat(next.display);
          next.display           = fmt(v * v);
          next.expression        = `sqr(${fmt(v)})`;
          next.justEvaled        = true;
          next.waitingForOperand = false;
          next.typed             = false;
          break;
        }
        case "sqrt": {
          if (next.error) break;
          const v = parseFloat(next.display);
          if (v < 0) { next = { ...INITIAL, display: "Invalid input", error: "ERR" }; break; }
          next.display           = fmt(Math.sqrt(v));
          next.expression        = `√(${fmt(v)})`;
          next.justEvaled        = true;
          next.waitingForOperand = false;
          next.typed             = false;
          break;
        }
      }
      return next;
    });
  }, [checkPin]);

  // Record history entry whenever an equals-result lands
  useEffect(() => {
    if (s.justEvaled && s.expression.endsWith("=") && !s.error) {
      setHistory((h) => [{ expression: s.expression, result: s.display }, ...h].slice(0, 50));
    }
  }, [s.justEvaled, s.expression, s.display, s.error]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (showHistory) { if (e.key === "Escape") setShowHistory(false); return; }
      if (e.key >= "0" && e.key <= "9") { dispatch({ type: "digit", value: e.key }); return; }
      if (e.key === ".") { dispatch({ type: "dot" }); return; }
      if (e.key === "Enter" || e.key === "=") { dispatch({ type: "equals" }); return; }
      if (e.key === "Backspace")  { dispatch({ type: "backspace" }); return; }
      if (e.key === "Escape")     { dispatch({ type: "clear" }); return; }
      if (e.key === "Delete")     { dispatch({ type: "clearEntry" }); return; }
      if (e.key === "+")          { dispatch({ type: "op", value: "+" }); return; }
      if (e.key === "-")          { dispatch({ type: "op", value: "-" }); return; }
      if (e.key === "*")          { dispatch({ type: "op", value: "×" }); return; }
      if (e.key === "/") { e.preventDefault(); dispatch({ type: "op", value: "÷" }); return; }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [dispatch, showHistory]);

  const handleMemory = useCallback((op: MemoryOp) => {
    const v = Number.parseFloat(s.display);
    if (!Number.isFinite(v)) return;
    switch (op) {
      case "MR":
        if (memory !== null) {
          // A recalled value is not hand-typed — never gate-eligible.
          setS((prev) => ({ ...prev, display: fmt(memory), justEvaled: true, waitingForOperand: false, error: null, typed: false }));
        }
        break;
      case "MC": setMemory(null); break;
      case "MS": setMemory(v); break;
      case "M+": setMemory((m) => (m ?? 0) + v); break;
      case "M-": setMemory((m) => (m ?? 0) - v); break;
    }
  }, [memory, s.display]);

  const handleMinimize = () => { void appWindow.minimize(); };
  const handleClose    = () => { void appWindow.close(); };

  // set_resizable(false) in enter_calculator_mode removes WS_THICKFRAME on
  // Windows, breaking data-tauri-drag-region. Call startDragging() directly.
  const handleDragMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button === 0) void appWindow.startDragging();
  }, [appWindow]);

  const displayFormatted = indianFormat(s.display);
  const resultClass =
    displayFormatted.length > 12 ? "calc-result calc-result--xs" :
    displayFormatted.length > 8  ? "calc-result calc-result--sm" :
    "calc-result";

  const B = (label: string, cls: string, onClick: () => void, key?: string) => (
    <button key={key ?? label} className={`calc-btn ${cls}`} onClick={onClick}>
      {label}
    </button>
  );

  return (
    <div className="calc-root" ref={containerRef}>
      <div className="calc-titlebar" data-tauri-drag-region onMouseDown={handleDragMouseDown}>
        <img src={appIcons["calc.png"]} alt="" className="calc-titlebar-icon" aria-hidden="true" />
        <div className="calc-window-title">Calculator</div>
        <div className="calc-window-controls" data-tauri-drag-region={false} onMouseDown={(e) => e.stopPropagation()}>
          <button type="button" onClick={handleMinimize} aria-label="Minimize">−</button>
          <button type="button" onClick={handleClose} aria-label="Close">×</button>
        </div>
      </div>

      {/* Header — draggable, interactive buttons excluded */}
      <div className="calc-header" data-tauri-drag-region onMouseDown={handleDragMouseDown}>
        <div className="calc-mode-title">Standard</div>
        <button
          className={`calc-icon-btn calc-history-btn${showHistory ? " active" : ""}`}
          aria-label="History"
          aria-pressed={showHistory}
          data-tauri-drag-region={false}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => setShowHistory((v) => !v)}
        >
          {/* Lucide-style "history" icon: back-arrow arc + clock hands */}
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M3 3v5h5" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>
            <path d="M3.07 13A9 9 0 1 0 6 5.3L3 8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>
            <path d="M12 7v5l4 2" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>
          </svg>
        </button>
      </div>

      {/* History panel — overlays the button grid */}
      {showHistory && (
        <div className="calc-history-panel">
          <div className="calc-history-header">
            <span>History</span>
            <button
              type="button"
              className="calc-history-close"
              aria-label="Close history"
              onClick={() => setShowHistory(false)}
            >
              ×
            </button>
          </div>
          {history.length === 0 ? (
            <div className="calc-history-empty">No history yet</div>
          ) : (
            <ul className="calc-history-list">
              {history.map((entry, i) => (
                <li key={i} className="calc-history-entry"
                  onClick={() => {
                    setS((prev) => ({ ...prev, display: entry.result, justEvaled: true, waitingForOperand: false, error: null, typed: false }));
                    setShowHistory(false);
                  }}
                >
                  <span className="calc-history-expr">{entry.expression}</span>
                  <span className="calc-history-result">{entry.result}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {/* Display */}
      <div className="calc-display">
        <div className="calc-expression">{s.expression || " "}</div>
        <div className={resultClass}>{renderResultDisplay(displayFormatted)}</div>
      </div>

      <div className="calc-memory-row">
        {(["MC", "MR", "M+", "M-", "MS"] as MemoryOp[]).map((op) => (
          <button
            key={op}
            type="button"
            disabled={(op === "MC" || op === "MR") && memory === null}
            onClick={() => handleMemory(op)}
          >
            {op}
          </button>
        ))}
        <button type="button" className="calc-memory-more" disabled={memory === null}>M⌄</button>
      </div>

      {/* Buttons */}
      <div className="calc-buttons">
        {B("%",  "calc-btn--special", () => dispatch({ type: "percent" }))}
        {B("CE", "calc-btn--special", () => dispatch({ type: "clearEntry" }))}
        {B("C",  "calc-btn--special", () => dispatch({ type: "clear" }))}

        {/* Backspace — left-arrow polygon with ✕, sized to fit the button */}
        <button key="⌫" className="calc-btn calc-btn--special" onClick={() => dispatch({ type: "backspace" })}>
          <svg width="20" height="16" viewBox="0 0 20 16" fill="none" aria-hidden="true" style={{ flexShrink: 0 }}>
            <path d="M7.5 1H18C18.55 1 19 1.45 19 2V14C19 14.55 18.55 15 18 15H7.5L1.5 8Z" stroke="currentColor" strokeWidth="1.35" strokeLinejoin="round"/>
            <line x1="10.5" y1="5.5" x2="15.5" y2="10.5" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round"/>
            <line x1="15.5" y1="5.5" x2="10.5" y2="10.5" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round"/>
          </svg>
        </button>

        {/* 1/x — fraction notation */}
        <button key="1/x" className="calc-btn calc-btn--fn" onClick={() => dispatch({ type: "reciprocal" })}>
          <span className="calc-btn-frac" aria-label="1 over x"><sup>1</sup>/<sub>x</sub></span>
        </button>

        {B("x²",  "calc-btn--fn", () => dispatch({ type: "square" }))}

        {/* ²√x — radical with index */}
        <button key="²√x" className="calc-btn calc-btn--fn" onClick={() => dispatch({ type: "sqrt" })}>
          <span className="calc-btn-sqrt" aria-label="square root of x"><sup>2</sup>√x</span>
        </button>

        {B("÷",   "calc-btn--op", () => dispatch({ type: "op", value: "÷" }))}

        {["7","8","9"].map(d => B(d, "", () => dispatch({ type: "digit", value: d })))}
        {B("×", "calc-btn--op", () => dispatch({ type: "op", value: "×" }))}

        {["4","5","6"].map(d => B(d, "", () => dispatch({ type: "digit", value: d })))}
        {B("−", "calc-btn--op", () => dispatch({ type: "op", value: "-" }))}

        {["1","2","3"].map(d => B(d, "", () => dispatch({ type: "digit", value: d })))}
        {B("+", "calc-btn--op", () => dispatch({ type: "op", value: "+" }))}

        <button key="+/-" className="calc-btn" onClick={() => dispatch({ type: "negate" })}>
          <svg width="22" height="22" viewBox="0 0 22 22" fill="none" aria-hidden="true">
            <line x1="3.5" y1="8" x2="9.5" y2="8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
            <line x1="6.5" y1="5" x2="6.5" y2="11" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
            <line x1="13.5" y1="5.5" x2="7.5" y2="16.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round"/>
            <line x1="12.5" y1="14" x2="18.5" y2="14" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
          </svg>
        </button>
        {B("0",   "calc-btn--zero", () => dispatch({ type: "digit", value: "0" }))}
        {B(".",   "calc-btn--dot",  () => dispatch({ type: "dot" }))}
        {B("=",   "calc-btn--eq",   () => dispatch({ type: "equals" }))}
      </div>
    </div>
  );
}
