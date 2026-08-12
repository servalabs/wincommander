// src/panels/advisor/index.tsx
//
// AI Security Advisor panel (spec 13 / #10).
//
// A fully-offline local-LLM advisor. Five task buttons run a model
// (via Ollama at 127.0.0.1:11434, in the Pro sidecar) over a bounded
// context Free assembles from the user's OWN machine. Nothing leaves the
// box. Every result carries a persistent "verify before acting"
// disclaimer; the result view is read-only and never executes model
// output.
//
// Business logic lives in useAdvisor(); this component is presentation +
// wiring. Wrapped in DependencyGate (panel `requiresDependency`) keyed to
// the `localLlm` dependency, so the engine install/start flow is
// automatic.

import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Button, Spinner, Tooltip, Icon } from "@/components/ui/bp";
import { panelVariants, panelTransition } from "@/components/shared/motion";
import useAdvisor, {
  ADVISOR_MODELS,
  ADVISOR_MODEL_SIZES,
  ADVISOR_MODEL_HINTS,
  DEFAULT_ADVISOR_MODEL,
  isModelReady,
  type AdvisorModel,
} from "../../hooks/useAdvisor";
import type { LlmTask } from "../../hooks/useBackend";
import { useSettingsQuery, usePatchSettings } from "../../hooks/queries/useSettingsQuery";
import SafeMarkdown from "./SafeMarkdown";
import "./index.css";

interface TaskDef {
  id: LlmTask;
  label: string;
  icon: "shield" | "build" | "history" | "search-around" | "globe-network";
  tooltip: string;
}

const TASKS: TaskDef[] = [
  {
    id: "explain-risks",
    label: "Explain risks",
    icon: "shield",
    tooltip: "Plain-language explanation of risky settings in your current configuration.",
  },
  {
    id: "suggest-hardening",
    label: "Suggest hardening",
    icon: "build",
    tooltip: "Highest-impact changes to harden this machine, in priority order.",
  },
  {
    id: "summarize-logs",
    label: "Summarize logs",
    icon: "history",
    tooltip: "A short summary of recent activity from this app's rolling log.",
  },
  {
    id: "detect-anomalies",
    label: "Detect anomalies",
    icon: "search-around",
    tooltip: "Flags anything unusual in your drift report, recent security-monitor events, and network location signals.",
  },
  {
    id: "explain-connection",
    label: "Explain connections",
    icon: "globe-network",
    tooltip: "What your active network connections are likely doing.",
  },
];

type StatusKind = "checking" | "ready" | "model-missing" | "server-down" | "engine-missing";

function statusKind(
  status: ReturnType<typeof useAdvisor>["status"],
  model: string,
): StatusKind {
  if (!status) return "engine-missing";
  if (!status.installed) return "engine-missing";
  if (!status.running) return "server-down";
  if (!isModelReady(status, model)) return "model-missing";
  return "ready";
}

export default function AdvisorPanel() {
  const advisor = useAdvisor();
  const { data: settings } = useSettingsQuery();
  const patchSettings = usePatchSettings();

  const model = (settings?.app?.advisor?.model as AdvisorModel) ?? DEFAULT_ADVISOR_MODEL;
  const [initialProbePending, setInitialProbePending] = useState(true);
  const [activeTask, setActiveTask] = useState<LlmTask | null>(null);
  const [copied, setCopied] = useState(false);

  // Probe the server once on mount.
  useEffect(() => {
    void advisor.refreshStatus().finally(() => setInitialProbePending(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const kind = useMemo(
    () => initialProbePending && !advisor.status ? "checking" : statusKind(advisor.status, model),
    [advisor.status, initialProbePending, model],
  );
  const ready = kind === "ready";

  const onModelChange = useCallback(
    (next: AdvisorModel) => {
      if (next === model) return;
      void patchSettings.mutateAsync({ app: { advisor: { model: next } } });
    },
    [patchSettings, model],
  );

  const onPull = useCallback(() => {
    void advisor.pullModel(model);
  }, [advisor, model]);

  const onRunTask = useCallback(
    (task: LlmTask) => {
      setActiveTask(task);
      setCopied(false);
      void advisor.analyze(task, model);
    },
    [advisor, model],
  );

  const onCopy = useCallback(() => {
    if (!advisor.result?.markdown) return;
    void navigator.clipboard.writeText(advisor.result.markdown).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    });
  }, [advisor.result]);

  const statusLabel = useMemo(() => {
    switch (kind) {
      case "checking":
        return "Checking local AI engine…";
      case "ready":
        return `Advisor ready — ${model}`;
      case "model-missing":
        return `Model not installed — ${model}`;
      case "server-down":
        return "AI server stopped — Start";
      case "engine-missing":
      default:
        return "AI engine not detected";
    }
  }, [kind, model]);

  const pullPercent = useMemo(() => {
    const p = advisor.pullProgress;
    if (!p || !p.total || p.total <= 0) return null;
    return Math.min(100, Math.round(((p.completed ?? 0) / p.total) * 100));
  }, [advisor.pullProgress]);

  return (
    <div className="advisor-panel">
      <header className="advisor-header">
        <div className="advisor-title">
          <Icon icon="predictive-analysis" size={18} />
          <span>AI Security Advisor</span>
        </div>
        <div className={`advisor-status-pill advisor-status-pill--${kind} font-mono`}>
          <span className="advisor-status-dot" aria-hidden />
          {statusLabel}
        </div>
      </header>

      <p className="advisor-subtitle">
        Runs a local AI model entirely on this device. No prompt, log line, or IP ever
        leaves your machine.
      </p>

      {/* ── Model picker + first-run/download controls ── */}
      <div className="advisor-controls">
        <div className="advisor-controls-head">
          <span className="advisor-model-label">Model</span>
          <Button
            className="advisor-refresh-btn"
            icon="refresh"
            minimal
            small
            loading={advisor.statusLoading}
            onClick={() => void advisor.refreshStatus()}
          >
            Re-check
          </Button>
        </div>

        <div
          className="advisor-model-grid"
          role="radiogroup"
          aria-label="Advisor model"
        >
          {ADVISOR_MODELS.map((m) => {
            const selected = m === model;
            const installed = isModelReady(advisor.status, m);
            const recommended = m === DEFAULT_ADVISOR_MODEL;
            return (
              <button
                key={m}
                type="button"
                role="radio"
                aria-checked={selected}
                className={`advisor-model-chip${selected ? " is-selected" : ""}`}
                disabled={advisor.pulling || advisor.analyzing}
                onClick={() => onModelChange(m)}
              >
                <span className="advisor-model-chip-top">
                  <span className="advisor-model-chip-name font-mono">{m}</span>
                  {recommended && (
                    <span className="advisor-model-badge">Recommended</span>
                  )}
                </span>
                <span className="advisor-model-chip-meta">
                  <span className="advisor-model-chip-size font-mono">
                    {ADVISOR_MODEL_SIZES[m]}
                  </span>
                  <span className="advisor-model-chip-hint">
                    {ADVISOR_MODEL_HINTS[m]}
                  </span>
                </span>
                <span
                  className={`advisor-model-chip-state${installed ? " is-installed" : ""}`}
                >
                  <Icon icon={installed ? "tick-circle" : "download"} size={11} />
                  {installed ? "Installed" : "Not downloaded"}
                </span>
              </button>
            );
          })}
        </div>

        {kind === "model-missing" && (
          <div className="advisor-controls-actions">
            <Button
              className="advisor-pull-btn"
              intent="primary"
              icon={advisor.pulling ? undefined : "download"}
              loading={advisor.pulling}
              disabled={advisor.analyzing}
              onClick={onPull}
            >
              {advisor.pulling
                ? "Downloading…"
                : `Download ${model} (${ADVISOR_MODEL_SIZES[model]})`}
            </Button>
          </div>
        )}
      </div>

      {/* ── Pull progress ── */}
      {advisor.pulling && (
        <div className="advisor-pull-progress">
          <div
            className="advisor-pull-bar"
            role="progressbar"
            aria-label={`Downloading ${model}`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={pullPercent ?? undefined}
          >
            <div
              className="advisor-pull-fill"
              data-indeterminate={pullPercent === null}
              style={pullPercent !== null ? { width: `${pullPercent}%` } : undefined}
            />
          </div>
          <span className="advisor-pull-status font-mono">
            {advisor.pullProgress?.status ?? "starting"}
            {pullPercent !== null ? ` — ${pullPercent}%` : ""}
          </span>
        </div>
      )}

      {/* ── Task buttons ── */}
      <div className="advisor-tasks">
        {TASKS.map((t) => (
          <Tooltip key={t.id} content={t.tooltip} compact>
            <Button
              className="advisor-task-btn"
              icon={t.icon}
              disabled={!ready || advisor.analyzing}
              loading={advisor.analyzing && activeTask === t.id}
              onClick={() => onRunTask(t.id)}
            >
              {t.label}
            </Button>
          </Tooltip>
        ))}
      </div>

      {/* ── Error ── */}
      {advisor.error && (
        <div className="advisor-error" role="alert">
          <Icon icon="warning-sign" size={14} />
          <span>{advisor.error}</span>
        </div>
      )}

      {/* ── Result view ── */}
      <div className="advisor-result">
        {/* AnimatePresence cross-fades between loading, result, and empty states.
            Each child needs a stable key so framer exits the old before entering the new. */}
        <AnimatePresence mode="wait">
          {advisor.analyzing ? (
            <motion.div
              key="loading"
              className="advisor-result-loading"
              variants={panelVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={panelTransition}
            >
              <Spinner size={20} />
              <span className="font-mono">Thinking locally…</span>
            </motion.div>
          ) : advisor.result?.markdown ? (
            <motion.div
              key={`result-${advisor.result.task}`}
              variants={panelVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={panelTransition}
            >
              <div className="advisor-result-toolbar">
                <span className="advisor-result-meta font-mono">
                  {advisor.result.task}
                  {advisor.result.tokens != null ? ` · ${advisor.result.tokens} tok` : ""}
                  {advisor.result.elapsedMs != null
                    ? ` · ${(advisor.result.elapsedMs / 1000).toFixed(1)}s`
                    : ""}
                </span>
                <Button minimal small icon={copied ? "tick" : "duplicate"} onClick={onCopy}>
                  {copied ? "Copied" : "Copy"}
                </Button>
              </div>
              <SafeMarkdown source={advisor.result.markdown} />
            </motion.div>
          ) : !advisor.error ? (
            <motion.div
              key="empty"
              className="advisor-result-empty"
              variants={panelVariants}
              initial="initial"
              animate="animate"
              exit="exit"
              transition={panelTransition}
            >
              {ready
                ? "Pick a task above to get a plain-language read on your machine."
                : kind === "checking"
                  ? "Checking the local AI engine and installed models…"
                : "Install the engine and download a model to enable the advisor."}
            </motion.div>
          ) : null}
        </AnimatePresence>

        {/* Persistent advisory disclaimer — always visible under the result area. */}
        <p className="advisor-disclaimer">
          <Icon icon="info-sign" size={12} />
          Advisory — generated by a local model on this device. May be incomplete or
          wrong. Verify before acting.
        </p>
      </div>
    </div>
  );
}
