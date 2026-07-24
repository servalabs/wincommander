// src/components/TaskStatusBar.tsx
//
// Unified floating task card — one card for ALL tasks (runOperation).
// Bottom-right corner. Matches OperationOverlay visual language.
// Mounted once in App.tsx; no per-panel wiring needed.

import { useTaskStatus, type TaskItem } from "../context/TaskStatusContext";
import { useState, useEffect, useRef } from "react";
import { Icon } from "./ui/icon";
import { ProcessChip } from "./ui/ProcessChip";
import './TaskStatusBar.css';

// ── Accent palette (mirrors OperationContext) ─────────────────────────────────

const ACCENT_COLORS = {
  red: {
    border:  'rgba(220, 38, 38, 0.55)',
    glow:    '0 8px 32px rgba(220, 38, 38, 0.35)',
    bar:     'var(--color-danger)',
    barBg:   'rgba(220, 38, 38, 0.18)',
    title:   'var(--color-danger)',
  },
  blue: {
    border:  'rgba(59, 130, 246, 0.55)',
    glow:    '0 8px 32px rgba(59, 130, 246, 0.25)',
    bar:     'var(--color-info)',
    barBg:   'rgba(59, 130, 246, 0.18)',
    title:   'var(--color-info)',
  },
  neutral: {
    border:  'rgba(100, 116, 139, 0.45)',
    glow:    '0 8px 32px rgba(0,0,0,0.35)',
    bar:     'var(--color-accent)',
    barBg:   'rgba(100, 116, 139, 0.15)',
    title:   'var(--color-text-secondary)',
  },
  done: {
    border:  'rgba(34, 197, 94, 0.55)',
    glow:    '0 8px 32px rgba(34, 197, 94, 0.3)',
    bar:     'var(--color-success)',
    barBg:   'rgba(34, 197, 94, 0.12)',
    title:   'var(--color-success)',
  },
  failed: {
    border:  'rgba(239, 68, 68, 0.55)',
    glow:    '0 8px 32px rgba(239, 68, 68, 0.3)',
    bar:     'var(--color-danger)',
    barBg:   'rgba(239, 68, 68, 0.15)',
    title:   'var(--color-danger)',
  },
};

// ── Derive the dominant accent for the card border/glow ──────────────────────

function cardAccent(tasks: TaskItem[]) {
  if (tasks.length === 0) return ACCENT_COLORS.neutral;
  const anyFailed  = tasks.some(t => t.status === 'failed');
  const allDone    = tasks.every(t => t.status === 'completed' || t.status === 'failed');
  if (anyFailed)   return ACCENT_COLORS.failed;
  if (allDone)     return ACCENT_COLORS.done;
  const hasRed     = tasks.some(t => t.accent === 'red');
  const hasBlue    = tasks.some(t => t.accent === 'blue');
  if (hasRed)      return ACCENT_COLORS.red;
  if (hasBlue)     return ACCENT_COLORS.blue;
  return ACCENT_COLORS.neutral;
}


// ── Main unified card ─────────────────────────────────────────────────────────

export default function TaskStatusBar() {
  const { tasks, clearCompleted } = useTaskStatus();
  const [minimized, setMinimized] = useState(false);
  const prevCountRef = useRef(0);

  // Auto-expand when a new task is added while minimized
  useEffect(() => {
    const runningCount = tasks.filter(t => t.status === 'running').length;
    if (runningCount > prevCountRef.current) {
      setMinimized(false);
    }
    prevCountRef.current = runningCount;
  }, [tasks]);

  if (tasks.length === 0) return null;

  const palette   = cardAccent(tasks);
  const allDone   = tasks.every(t => t.status === 'completed' || t.status === 'failed');
  const anyFailed = tasks.some(t => t.status === 'failed');
  const running   = tasks.filter(t => t.status === 'running').length;

  // Card title: always 'N TASKS' to avoid duplication with the task.label in the row below
  const cardTitle = tasks.length === 1
    ? "1 TASK"
    : `${tasks.length} TASKS`;

  const headerIcon = anyFailed ? 'cross' : allDone ? 'tick' : null;

  return (
    <div
      className="tsb-card"
      style={{
        position: 'fixed',
        bottom: 16,
        right: 16,
        zIndex: 'var(--z-status)',
        width: 270,
        border: `1px solid ${palette.border}`,
        boxShadow: palette.glow,
      }}
    >
      {/* Card header */}
      <div className="tsb-header">
        {/* Status icon or pulse dot */}
        {headerIcon ? (
          <span
            className="tsb-header-icon"
            style={{ color: anyFailed ? 'var(--color-danger)' : 'var(--color-success)' }}
          >
            <Icon icon={headerIcon} size={12} />
          </span>
        ) : (
          <span className="tsb-dot-running tsb-header-dot" />
        )}

        <span className="tsb-header-title" style={{ color: palette.title }}>
          {cardTitle}
        </span>

        {/* Running count badge — hidden when minimized to save space */}
        {running > 0 && !minimized && (
          <span className="tsb-header-count" style={{ color: `${palette.title}99` }}>
            {running} running
          </span>
        )}

        {/* Minimize toggle — always present */}
        <button
          className="tsb-dismiss-btn"
          onClick={() => setMinimized(m => !m)}
          title={minimized ? 'Expand' : 'Minimize'}
        >
          <Icon icon={minimized ? 'chevron-up' : 'chevron-down'} size={12} />
        </button>

        {/* Dismiss all — only when everything is done */}
        {allDone && (
          <button
            className="tsb-dismiss-btn"
            onClick={clearCompleted}
            title="Dismiss"
          >
            <Icon icon="cross" size={12} />
          </button>
        )}
      </div>

      {/* Collapsible body */}
      {!minimized && (
        <>
          {/* Task rows */}
          <div className="tsb-task-list">
            {tasks.map(task => (
              <ProcessChip key={task.id} task={task} showCopyOnError />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
