import { useState, useCallback } from "react";
import type { TaskItem, StepStatus } from "../../context/TaskStatusContext";

const STEP_GLYPH: Record<StepStatus, string> = {
  idle: "○",
  running: "◌",
  done: "✓",
  error: "✗",
};

interface ProcessChipProps {
  task: TaskItem;
  showCopyOnError?: boolean;
}

export function ProcessChip({ task, showCopyOnError = false }: ProcessChipProps) {
  const isCompleted = task.status === "completed";
  const isFailed = task.status === "failed";
  const isRunning = task.status === "running";
  const [copied, setCopied] = useState(false);
  const total = task.steps.length;
  const done = task.steps.filter((s) => s.status === "done" || s.status === "error").length;

  const handleCopy = useCallback(() => {
    const lines = [task.label];
    if (task.message) lines.push(task.message);
    navigator.clipboard.writeText(lines.join("\n")).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [task.label, task.message]);

  const borderClass = isFailed
    ? "border-[var(--danger)]"
    : isCompleted
      ? "border-[color-mix(in_srgb,var(--ok)_35%,var(--border))]"
      : "border-[color-mix(in_srgb,var(--accent)_25%,var(--border))]";

  const bgClass = isFailed
    ? "bg-[color-mix(in_srgb,var(--danger)_6%,var(--surface-2))]"
    : isCompleted
      ? "bg-[color-mix(in_srgb,var(--ok)_6%,var(--surface-2))]"
      : "bg-[color-mix(in_srgb,var(--accent)_6%,var(--surface-2))]";

  return (
    <div className={`flex flex-col gap-1.5 rounded-[var(--r)] border ${borderClass} ${bgClass} p-2.5`}>
      <div className="flex items-center gap-2">
        {isRunning && (
          <span
            className="inline-block h-1.5 w-1.5 shrink-0 rounded-full"
            style={{ background: "var(--accent)", boxShadow: "0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent)" }}
          />
        )}
        {isCompleted && <span className="text-[10px] font-bold text-[var(--ok)]">✓</span>}
        {isFailed && <span className="text-[10px] font-bold text-[var(--danger)]">✗</span>}

        <span
          className="flex-1 truncate font-[family-name:var(--font-mono)] text-[10.5px] font-bold uppercase tracking-wider"
          style={{ color: isFailed ? "var(--danger)" : isCompleted ? "var(--ok)" : "var(--text)" }}
        >
          {task.label}
        </span>

        {total > 0 && (
          <span className="shrink-0 font-[family-name:var(--font-mono)] text-[9.5px] text-[var(--text-mute)]">
            {done}/{total}
          </span>
        )}

        <span
          className="shrink-0 rounded px-1.5 py-0.5 font-[family-name:var(--font-mono)] text-[8.5px] font-bold uppercase tracking-wider"
          style={{
            background: isFailed
              ? "color-mix(in srgb, var(--danger) 20%, transparent)"
              : isCompleted
                ? "color-mix(in srgb, var(--ok) 20%, transparent)"
                : "color-mix(in srgb, var(--accent) 20%, transparent)",
            color: isFailed ? "var(--danger)" : isCompleted ? "var(--ok)" : "var(--accent)",
          }}
        >
          {isFailed ? "Error" : isCompleted ? "Done" : "Running"}
        </span>

        {isFailed && showCopyOnError && (
          <button
            className="grid h-5 w-5 shrink-0 place-items-center rounded-[var(--r-sm)] text-[var(--text-mute)] transition-colors hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
            onClick={handleCopy}
            title="Copy error to clipboard"
          >
            <span className="text-[11px]">{copied ? "✓" : "⧉"}</span>
          </button>
        )}
      </div>

      {task.message && (
        <div className="truncate text-[10.5px] text-[var(--text-mute)]">{task.message}</div>
      )}

      {task.steps.length > 0 && (
        <div className="flex flex-col gap-0.5 pl-3">
          {task.steps.map((s, i) => (
            <div
              key={i}
              className="flex items-center gap-1.5 font-[family-name:var(--font-mono)] text-[9.5px]"
              style={{
                opacity: s.status === "idle" ? 0.4 : s.status === "done" ? 0.7 : 1,
                color:
                  s.status === "error"
                    ? "var(--danger)"
                    : s.status === "done"
                      ? "var(--ok)"
                      : s.status === "running"
                        ? "var(--warn)"
                        : "var(--text-mute)",
              }}
            >
              <span className="w-3 shrink-0 text-center">{STEP_GLYPH[s.status]}</span>
              <span className="truncate">{s.label}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
