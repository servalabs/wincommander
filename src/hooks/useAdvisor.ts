// src/hooks/useAdvisor.ts
//
// useAdvisor — drives the AI Security Advisor panel (spec 13 / #10).
//
// Split of responsibility (per spec):
//   • Free assembles the bounded context (advisor_build_context — FREE,
//     invoked directly like get_drift_report).
//   • Pro runs the local Ollama model (Get-OllamaStatus / Pull-OllamaModel
//     / Llm-Analyze — PAID, routed via run_backend_script → sidecar).
//
// All inference is 100% localhost (Pro → 127.0.0.1:11434). This hook
// never sees prompts or completions leave the machine.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  executeBackendCommand,
  type OllamaStatusResult,
  type LlmAnalyzeResult,
  type LlmTask,
  type NetworkPortsResult,
} from "./useBackend";
import {
  getSnapshot,
  subscribe,
  setPulling,
  setPullProgress,
  initPullListener,
  type PullProgress,
} from "./advisorPullStore";

// Re-export so existing imports from this module keep working.
export type { PullProgress };

/** Pinned model allowlist — MUST mirror Pro's ALLOWED_MODELS. The picker
 *  offers exactly these; anything else is refused by Pro.
 *
 *  NOTE: uses the Qwen3.5 generation — Ollama tags it `qwen3.5:*`
 *  (qwen3.5:0.8b / 2b / 4b / 9b dense). VERIFY the exact tag resolves
 *  (`ollama pull qwen3.5:4b`); if the local registry names the 3.5 line
 *  differently it's a one-line change here + in llm.rs (keep the two lists
 *  in exact sync). llama3.2:3b is kept as a guaranteed fallback. */
export const ADVISOR_MODELS = [
  "qwen3.5:0.8b",
  "qwen3.5:2b",
  "qwen3.5:4b",
  "qwen3.5:9b",
  "llama3.2:3b",
] as const;

export type AdvisorModel = (typeof ADVISOR_MODELS)[number];

/** qwen3.5:4b — best balance of quality vs RAM/speed for the advisor. */
export const DEFAULT_ADVISOR_MODEL: AdvisorModel = "qwen3.5:4b";

/** Approximate download sizes (Q4) for the first-run CTA copy. */
export const ADVISOR_MODEL_SIZES: Record<AdvisorModel, string> = {
  "qwen3.5:0.8b": "~0.6 GB",
  "qwen3.5:2b": "~1.4 GB",
  "qwen3.5:4b": "~2.6 GB",
  "qwen3.5:9b": "~5.6 GB",
  "llama3.2:3b": "~2.0 GB",
};

/** Short RAM/speed hint per model — drives the picker chip subtitle. */
export const ADVISOR_MODEL_HINTS: Record<AdvisorModel, string> = {
  "qwen3.5:0.8b": "Tiny · lowest RAM",
  "qwen3.5:2b": "Low RAM · fast",
  "qwen3.5:4b": "Balanced",
  "qwen3.5:9b": "Best quality · needs headroom",
  "llama3.2:3b": "Llama fallback",
};

export interface UseAdvisor {
  status: OllamaStatusResult | null;
  statusLoading: boolean;
  result: LlmAnalyzeResult | null;
  analyzing: boolean;
  pulling: boolean;
  pullProgress: PullProgress | null;
  error: string | null;
  refreshStatus: () => Promise<void>;
  pullModel: (model: AdvisorModel) => Promise<boolean>;
  analyze: (task: LlmTask, model: AdvisorModel) => Promise<void>;
}

/** True when the configured model is present in the running server. */
export function isModelReady(
  status: OllamaStatusResult | null,
  model: string,
): boolean {
  if (!status?.running) return false;
  return status.models.includes(model);
}

export default function useAdvisor(): UseAdvisor {
  const [status, setStatus] = useState<OllamaStatusResult | null>(null);
  const [statusLoading, setStatusLoading] = useState(false);
  const [result, setResult] = useState<LlmAnalyzeResult | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Subscribe to the module-level pull store so pulling/pullProgress survive
  // panel switches. The Tauri "llm-pull-progress" listener lives in the store
  // (never unmounts), so events keep flowing even when this hook is gone.
  const [pullState, setPullState] = useState(getSnapshot);
  useEffect(() => {
    initPullListener(); // idempotent — registers the Tauri listener once ever
    // Sync immediately in case a download started while this panel was away.
    setPullState(getSnapshot());
    const unsubscribe = subscribe(() => {
      setPullState(getSnapshot());
    });
    return unsubscribe;
  }, []);

  const { pulling, pullProgress } = pullState;

  const refreshStatus = useCallback(async () => {
    setStatusLoading(true);
    setError(null);
    const res = await executeBackendCommand<OllamaStatusResult>("Get-OllamaStatus");
    if (res.success && res.data) setStatus(res.data);
    else setError(res.error ?? "Failed to query the AI server.");
    setStatusLoading(false);
  }, []);

  const pullModel = useCallback(
    async (model: AdvisorModel): Promise<boolean> => {
      setPulling(true);
      setPullProgress(null);
      setError(null);
      try {
        const res = await executeBackendCommand<{ ok: boolean; model: string; error?: string }>(
          "Pull-OllamaModel",
          { model },
        );
        if (!res.success) {
          setError(res.error ?? "Model download failed.");
          return false;
        }
        if (res.data && !res.data.ok) {
          setError(res.data.error ?? "Model download failed.");
          return false;
        }
        await refreshStatus();
        return true;
      } finally {
        setPulling(false);
        setPullProgress(null);
      }
    },
    [refreshStatus],
  );

  const analyze = useCallback(
    async (task: LlmTask, model: AdvisorModel) => {
      setAnalyzing(true);
      setError(null);
      setResult(null);
      try {
        // explain-connection needs the live ports the FE already fetches;
        // pass them to the free context builder (it filters/caps them).
        let ports: NetworkPortsResult | undefined;
        if (task === "explain-connection") {
          const portsRes = await executeBackendCommand<NetworkPortsResult>("Get-NetworkPorts");
          if (portsRes.success) ports = portsRes.data;
        }

        // FREE: assemble the bounded context (invoked directly, not via
        // run_backend_script — it's a plain Tauri command).
        const context = await invoke<unknown>("advisor_build_context", {
          task,
          ports: ports ?? null,
        });

        // PAID: run the model in Pro. context is stringified; model + task
        // are validated server-side.
        const res = await executeBackendCommand<LlmAnalyzeResult>("Llm-Analyze", {
          task,
          context: JSON.stringify(context),
          model,
        });

        if (!res.success) {
          setError(res.error ?? "Analysis failed.");
          return;
        }
        if (res.data && !res.data.ok && res.data.error) {
          setError(res.data.error);
          setResult(res.data);
          return;
        }
        setResult(res.data ?? null);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setAnalyzing(false);
      }
    },
    [],
  );

  return {
    status,
    statusLoading,
    result,
    analyzing,
    pulling,
    pullProgress,
    error,
    refreshStatus,
    pullModel,
    analyze,
  };
}
