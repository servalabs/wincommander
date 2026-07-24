// src/context/OperationContext.tsx
//
// Universal Operation System — multi-step task runner.
// Backed by TaskStatusContext so all operations appear in the unified TaskStatusBar card.
//
// USAGE (unchanged from before):
//   import { runOperation } from "../context/OperationContext";
//
//   await runOperation("PURGING SYSTEM", [
//     { label: "Erase clipboard",    fn: clearClipboard   },
//     { label: "Clear USB history", fn: clearUsbHistory  },
//   ], { mode: 'parallel', accent: 'red' });

import { _getOperationHandlers } from './TaskStatusContext';
import { playSound } from '../utils/sound';

// ── Types (public API — unchanged) ───────────────────────────────────────────

export type StepStatus = 'idle' | 'running' | 'done' | 'error';

export interface OperationStep {
    label: string;
    fn: () => Promise<any>;
}

export interface RunOperationOptions {
    doneTitle?: string;
    mode?: 'parallel' | 'sequential';
    failFast?: boolean;
    accent?: 'red' | 'blue' | 'neutral';
    autoDismissMs?: number;
}

// ── runOperation ─────────────────────────────────────────────────────────────

export function runOperation(
    title: string,
    steps: OperationStep[],
    opts?: RunOperationOptions
): Promise<{ anyError: boolean }> {
    const mode       = opts?.mode        ?? 'parallel';
    const failFast   = opts?.failFast    ?? true;
    const accent     = opts?.accent      ?? 'neutral';

    return new Promise<{ anyError: boolean }>((resolve) => {
        const execute = async () => {
            const h = _getOperationHandlers();

            // If TaskStatusProvider hasn't mounted yet (very early calls), run silently
            if (!h.addOperationTask || !h.updateOperationStep || !h.completeTask || !h.failTask) {
                let anyError = false;
                try {
                    if (mode === 'parallel') {
                        const results = await Promise.allSettled(steps.map(s => s.fn()));
                        anyError = results.some(r => r.status === 'rejected');
                    } else {
                        for (const step of steps) {
                            try { await step.fn(); } catch { anyError = true; if (failFast) break; }
                        }
                    }
                } catch { anyError = true; }
                resolve({ anyError });
                return;
            }

            const id = h.addOperationTask(title, steps.map(s => s.label), accent);
            let anyError = false;

            // KT: null means the op was suppressed (superset already running).
            // Run steps silently — do NOT touch task state so the superset's
            // completeTask/failTask lifecycle remains intact.
            const suppressed = id === null;

            if (mode === 'parallel') {
                await Promise.allSettled(
                    steps.map(async (step, i) => {
                        if (!suppressed) h.updateOperationStep!(id!, i, 'running');
                        try {
                            await step.fn();
                            if (!suppressed) h.updateOperationStep!(id!, i, 'done');
                        } catch {
                            anyError = true;
                            if (!suppressed) h.updateOperationStep!(id!, i, 'error');
                        }
                    })
                );
            } else {
                for (let i = 0; i < steps.length; i++) {
                    if (!suppressed) h.updateOperationStep!(id!, i, 'running');
                    try {
                        await steps[i].fn();
                        if (!suppressed) h.updateOperationStep!(id!, i, 'done');
                    } catch {
                        anyError = true;
                        if (!suppressed) h.updateOperationStep!(id!, i, 'error');
                        if (failFast) break;
                    }
                }
            }

            if (!suppressed) {
                if (anyError) {
                    h.failTask!(id!, "Completed with errors");
                } else {
                    playSound('complete');
                    h.completeTask!(id!);
                }
            }

            resolve({ anyError });
        };

        execute();
    });
}

// ── OperationOverlay ─────────────────────────────────────────────────────────
// No longer renders its own UI — TaskStatusBar handles everything.
// Kept as a no-op export so App.tsx does not need changes.

export function OperationOverlay() {
    return null;
}
