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

// ── beginOperation ───────────────────────────────────────────────────────────
//
// Same task row as runOperation, but held OPEN so more steps can be added to it
// after it started. runOperation fixes its step list at creation, so work the
// user asks for mid-flight (queueing a second app onto a running install) could
// only ever open a competing second row — or, if it reused the title,
// silently overwrite the first row's steps by index via addOperationTask's
// exact-label dedup. This is the one surface where the step list genuinely is
// not known up front.
//
//   const op = beginOperation("Installing apps", { accent: 'blue' });
//   await op.add(firstBatch);   // more batches may be added while this awaits
//   op.finish();

export interface GrowingOperation {
    /** Append these steps to the live row and run them in parallel. */
    add: (steps: OperationStep[]) => Promise<{ anyError: boolean }>;
    /** Close the row: completed, or failed if any step so far errored. */
    finish: () => { anyError: boolean };
}

export function beginOperation(
    title: string,
    opts?: { accent?: 'red' | 'blue' | 'neutral' },
): GrowingOperation {
    const accent = opts?.accent ?? 'neutral';
    // null covers BOTH "TaskStatusProvider not mounted yet" and
    // addOperationTask's SUPPRESSED_TASK_ID — in either case the steps still
    // run, they just do not report into a row.
    const id = _getOperationHandlers().addOperationTask?.(title, [], accent) ?? null;
    let anyError = false;

    const add = async (steps: OperationStep[]): Promise<{ anyError: boolean }> => {
        if (steps.length === 0) return { anyError };
        // Re-read the handlers per call: TaskStatusProvider re-registers them
        // whenever its callbacks change identity, so a set captured at
        // beginOperation time can go stale mid-operation.
        const h = _getOperationHandlers();
        const base = id !== null && h.appendOperationSteps
            ? h.appendOperationSteps(id, steps.map(s => s.label))
            : -1;
        await Promise.allSettled(
            steps.map(async (step, i) => {
                if (base >= 0) h.updateOperationStep?.(id!, base + i, 'running');
                try {
                    await step.fn();
                    if (base >= 0) h.updateOperationStep?.(id!, base + i, 'done');
                } catch {
                    anyError = true;
                    if (base >= 0) h.updateOperationStep?.(id!, base + i, 'error');
                }
            })
        );
        return { anyError };
    };

    const finish = (): { anyError: boolean } => {
        const h = _getOperationHandlers();
        if (id !== null) {
            if (anyError) {
                h.failTask?.(id, "Completed with errors");
            } else {
                playSound('complete');
                h.completeTask?.(id);
            }
        }
        return { anyError };
    };

    return { add, finish };
}

// ── OperationOverlay ─────────────────────────────────────────────────────────
// No longer renders its own UI — TaskStatusBar handles everything.
// Kept as a no-op export so App.tsx does not need changes.

export function OperationOverlay() {
    return null;
}
