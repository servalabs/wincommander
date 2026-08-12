// src/context/TaskStatusContext.tsx
//
// Universal Task Status System for WinCommander.
// Single floating card for ALL long-running tasks via runOperation().

import { createContext, useContext, useState, useCallback, useRef, useEffect, ReactNode } from "react";

// Sentinel returned by addOperationTask when the requested op is suppressed
// because a superset task (e.g. Fix Everything) is already running.
// Callers that receive SUPPRESSED_TASK_ID must NOT call completeTask/failTask.
export const SUPPRESSED_TASK_ID = null as null;

// ── Types ────────────────────────────────────────────────────────────────────

export type TaskStatus = "running" | "completed" | "failed";
export type StepStatus = 'idle' | 'running' | 'done' | 'error';

export interface TaskStep {
  label: string;
  status: StepStatus;
}

export interface TaskItem {
  id: string;
  label: string;
  status: TaskStatus;
  message?: string;          // Sub-label
  completedAt?: number;      // Timestamp for auto-dismiss
  steps: TaskStep[];
  accent?: 'red' | 'blue' | 'neutral';
}

interface TaskStatusContextValue {
  tasks: TaskItem[];
  completeTask: (id: string, message?: string) => void;
  failTask: (id: string, message?: string) => void;
  dismissTask: (id: string) => void;
  clearCompleted: () => void;
  // Operation tasks (multi-step).
  // Returns the task id, or null (SUPPRESSED_TASK_ID) when the op is swallowed
  // because a superset task is already running — callers must not touch task state in that case.
  addOperationTask: (label: string, steps: string[], accent?: 'red' | 'blue' | 'neutral') => string | null;
  updateOperationStep: (id: string, stepIndex: number, status: StepStatus) => void;
  // Grows a still-running task's step list. Returns the index the first new
  // step landed at, or -1 if the task is gone or no longer running — needed so
  // work discovered AFTER a task started (a second app queued onto a running
  // install) reports into the SAME row instead of opening a second one.
  appendOperationSteps: (id: string, labels: string[]) => number;
}

// ── Context ──────────────────────────────────────────────────────────────────

const TaskStatusContext = createContext<TaskStatusContextValue | null>(null);

let taskCounter = 0;
const makeId = () => `task-${Date.now()}-${++taskCounter}`;

// ── Provider ─────────────────────────────────────────────────────────────────

export function TaskStatusProvider({ children }: { children: ReactNode }) {
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const dismissTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());
  // KT: Synchronous mirror of the tasks array keyed by id. Needed so
  // addOperationTask can read current task state OUTSIDE a setTasks updater,
  // which is not guaranteed to be synchronous under React 19 concurrent rendering.
  const taskMapRef = useRef<Map<string, TaskItem>>(new Map());

  // Keep taskMapRef in sync with the rendered tasks array so addOperationTask
  // can read it synchronously without relying on setTasks updater sequencing.
  useEffect(() => {
    const m = new Map<string, TaskItem>();
    tasks.forEach((t) => m.set(t.id, t));
    taskMapRef.current = m;
  }, [tasks]);

  const scheduleAutoDismiss = useCallback((id: string) => {
    const timer = setTimeout(() => {
      setTasks(prev => prev.filter(t => t.id !== id));
      dismissTimers.current.delete(id);
    }, 2500);
    dismissTimers.current.set(id, timer);
  }, []);

  const completeTask = useCallback((id: string, message?: string) => {
    setTasks(prev => prev.map(t =>
      t.id === id
        ? { ...t, status: "completed" as TaskStatus, message: message ?? "Done", completedAt: Date.now() }
        : t
    ));
    scheduleAutoDismiss(id);
  }, [scheduleAutoDismiss]);

  const failTask = useCallback((id: string, message?: string) => {
    const existing = dismissTimers.current.get(id);
    if (existing) { clearTimeout(existing); dismissTimers.current.delete(id); }

    setTasks(prev => prev.map(t =>
      t.id === id ? { ...t, status: "failed" as TaskStatus, message: message ?? "Failed" } : t
    ));
    const timer = setTimeout(() => {
      setTasks(prev => prev.filter(t => t.id !== id));
      dismissTimers.current.delete(id);
    }, 6000);
    dismissTimers.current.set(id, timer);
  }, []);

  const dismissTask = useCallback((id: string) => {
    const timer = dismissTimers.current.get(id);
    if (timer) { clearTimeout(timer); dismissTimers.current.delete(id); }
    setTasks(prev => prev.filter(t => t.id !== id));
  }, []);

  const clearCompleted = useCallback(() => {
    setTasks(prev => {
      const toRemove = prev.filter(t => t.status === "completed" || t.status === "failed");
      toRemove.forEach(t => {
        const timer = dismissTimers.current.get(t.id);
        if (timer) { clearTimeout(timer); dismissTimers.current.delete(t.id); }
      });
      return prev.filter(t => t.status !== "completed" && t.status !== "failed");
    });
  }, []);

  const addOperationTask = useCallback((label: string, steps: string[], accent?: 'red' | 'blue' | 'neutral'): string | null => {
    // KT: Read current task state synchronously from the ref (not inside
    // setTasks) so the returned id is reliable under React 19 concurrent mode.
    const isAppUpdateLabel = /^Update \d+ Apps?$/.test(label);
    const currentTasks = Array.from(taskMapRef.current.values());

    // Exact-label dedup: same op already running — reuse its entry.
    const exactMatch = currentTasks.find(t => t.status === "running" && t.label === label);
    if (exactMatch) return exactMatch.id;

    // KT: Superset dedup — if Fix Everything is running, the app-update op is
    // already covered. Return null (SUPPRESSED_TASK_ID) so runOperation skips
    // task-state management entirely and just runs the steps silently, preventing
    // completeTask/failTask from prematurely dismissing Fix Everything.
    if (isAppUpdateLabel) {
      const supersetRunning = currentTasks.some(t => t.status === "running" && t.label === "Fix Everything");
      if (supersetRunning) return null;
    }

    const id = makeId();
    setTasks(prev => [...prev, {
      id, label, status: "running",
      steps: steps.map(s => ({ label: s, status: 'idle' as StepStatus })),
      accent: accent ?? 'neutral',
    }]);
    // Mirror into the ref immediately so subsequent synchronous callers see it.
    taskMapRef.current.set(id, {
      id, label, status: "running",
      steps: steps.map(s => ({ label: s, status: 'idle' as StepStatus })),
      accent: accent ?? 'neutral',
    });
    return id;
  }, []);

  const appendOperationSteps = useCallback((id: string, labels: string[]): number => {
    // Same reason addOperationTask reads taskMapRef: the caller needs the base
    // index back synchronously, which a setTasks updater cannot provide.
    const existing = taskMapRef.current.get(id);
    if (!existing || existing.status !== "running") return -1;
    if (labels.length === 0) return existing.steps.length;
    const base = existing.steps.length;
    const added: TaskStep[] = labels.map(label => ({ label, status: 'idle' as StepStatus }));
    taskMapRef.current.set(id, { ...existing, steps: [...existing.steps, ...added] });
    setTasks(prev => prev.map(t => (t.id === id ? { ...t, steps: [...t.steps, ...added] } : t)));
    return base;
  }, []);

  const updateOperationStep = useCallback((id: string, stepIndex: number, status: StepStatus) => {
    setTasks(prev => prev.map(t => {
      if (t.id !== id || !t.steps) return t;
      const newSteps = [...t.steps];
      newSteps[stepIndex] = { ...newSteps[stepIndex], status };
      return { ...t, steps: newSteps };
    }));
  }, []);

  useEffect(() => {
    _registerGlobalHandlers(completeTask, failTask, addOperationTask, updateOperationStep, appendOperationSteps);
    return () => {
      _globalCompleteTask = null;
      _globalFailTask = null;
      _globalAddOperationTask = null;
      _globalUpdateOperationStep = null;
      _globalAppendOperationSteps = null;
    };
  }, [completeTask, failTask, addOperationTask, updateOperationStep, appendOperationSteps]);

  return (
    <TaskStatusContext.Provider value={{ tasks, completeTask, failTask, dismissTask, clearCompleted, addOperationTask, updateOperationStep, appendOperationSteps }}>
      {children}
    </TaskStatusContext.Provider>
  );
}

// ── Hook ─────────────────────────────────────────────────────────────────────

export function useTaskStatus(): TaskStatusContextValue {
  const ctx = useContext(TaskStatusContext);
  if (!ctx) throw new Error("useTaskStatus must be used inside <TaskStatusProvider>");
  return ctx;
}

// ── Global Handlers for OperationContext ────────────────────────────────────

let _globalCompleteTask: ((id: string, message?: string) => void) | null = null;
let _globalFailTask: ((id: string, message?: string) => void) | null = null;
let _globalAddOperationTask: ((label: string, steps: string[], accent?: 'red' | 'blue' | 'neutral') => string | null) | null = null;
let _globalUpdateOperationStep: ((id: string, stepIndex: number, status: StepStatus) => void) | null = null;
let _globalAppendOperationSteps: ((id: string, labels: string[]) => number) | null = null;

export function _registerGlobalHandlers(
  complete: (id: string, message?: string) => void,
  fail: (id: string, message?: string) => void,
  addOperation: (label: string, steps: string[], accent?: 'red' | 'blue' | 'neutral') => string | null,
  updateStep: (id: string, stepIndex: number, status: StepStatus) => void,
  appendSteps: (id: string, labels: string[]) => number,
) {
  _globalCompleteTask = complete;
  _globalFailTask = fail;
  _globalAddOperationTask = addOperation;
  _globalUpdateOperationStep = updateStep;
  _globalAppendOperationSteps = appendSteps;
}

export function _getOperationHandlers() {
  return {
    addOperationTask: _globalAddOperationTask,
    updateOperationStep: _globalUpdateOperationStep,
    appendOperationSteps: _globalAppendOperationSteps,
    completeTask: _globalCompleteTask,
    failTask: _globalFailTask,
  };
}
