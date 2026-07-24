import { invoke } from "@tauri-apps/api/core";

export type LogLevel = 'info' | 'warn' | 'error';

/**
 * Sends a log record to the Rust backend to be written to %APPDATA%/WinCommander/logs/wincommander.log
 */
export async function logRecord(level: LogLevel, message: string) {
  try {
    // Avoid circular logging if invoke itself fails or logs
    await invoke('write_log_record', { level, message });
  } catch (e) {
    // Silently fail to avoid loops or use a native console if absolutely needed
  }
}

const _recentLogs = new Map<string, number>();
function _rateLimited(message: string): boolean {
  // `now` via Date.now() is fine in the browser/webview runtime.
  const now = Date.now();
  const last = _recentLogs.get(message) ?? 0;
  if (now - last < 5000) return true;       // same message within 5s → skip the forward
  _recentLogs.set(message, now);
  if (_recentLogs.size > 200) _recentLogs.clear(); // bound the map
  return false;
}

// KT: when an error boundary catches a render error, React itself writes a
// console.error ("The above error occurred in the <X> component…") BEFORE
// componentDidCatch runs. Our error boundaries (ErrorBoundary.tsx,
// PanelErrorBoundary.tsx) already call logRecord with a richer message
// (error + stack + component stack) from componentDidCatch, so forwarding
// React's own echo too would double-log every single caught render error.
// This prefix is stable across React versions (it's part of React's public
// dev warning copy) — skip only this exact echo, not other console.error calls.
function _isReactErrorBoundaryEcho(message: string): boolean {
  return message.startsWith("The above error occurred in the");
}

/**
 * Universal error reporting: patches console and global window events to ensures
 * all crashes are captured in the local log file.
 */
export function initUniversalLogging() {
  const originalError = console.error;
  const originalWarn = console.warn;
  const originalLog = console.log;

  console.error = (...args: any[]) => {
    const message = args.map(arg => {
        try {
            return typeof arg === 'object' ? JSON.stringify(arg) : String(arg);
        } catch {
            return String(arg);
        }
    }).join(' ');
    // Skip React's own error-boundary echo — componentDidCatch already logs
    // a richer record for the same crash (see _isReactErrorBoundaryEcho).
    if (!_isReactErrorBoundaryEcho(message) && !_rateLimited('E:' + message)) {
      logRecord('error', message);
    }
    originalError(...args);
  };

  console.warn = (...args: any[]) => {
    const message = args.map(arg => {
        try {
            return typeof arg === 'object' ? JSON.stringify(arg) : String(arg);
        } catch {
            return String(arg);
        }
    }).join(' ');
    if (!_rateLimited('W:' + message)) logRecord('warn', message);
    originalWarn(...args);
  };

  // We only log important logs to file to avoid bloating
  console.log = (...args: any[]) => {
    const firstArg = String(args[0]);
    if (firstArg.startsWith('[App]') || firstArg.startsWith('[License]') || firstArg.startsWith('[Wizard]') || firstArg.startsWith('[Flow]')) {
        const message = args.map(arg => {
            try {
                return typeof arg === 'object' ? JSON.stringify(arg) : String(arg);
            } catch {
                return String(arg);
            }
        }).join(' ');
        logRecord('info', message);
    }
    originalLog(...args);
  };

  window.onerror = (message, source, lineno, colno, error) => {
    const msg = `[Window Error] ${message} at ${source}:${lineno}:${colno} - ${error?.stack || 'no stack'}`;
    logRecord('error', msg);
  };

  window.onunhandledrejection = (event) => {
    logRecord('error', `[Unhandled Rejection] ${event.reason}`);
  };
  
  logRecord('info', `[System] Universal Logging initialized. Platform: ${navigator.platform}`);
}
