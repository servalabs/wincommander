import { Component, ErrorInfo, ReactNode } from "react";
import { CompatButton as Button } from "@/components/ui/compat-button";
import { EmptyState as NonIdealState } from "@/components/ui/empty-state";
import { logRecord } from "../lib/logger";
import { isStalePanelLoadError } from "../lib/panelLoading";

const AUTO_RECOVERY_KEY = "wc:panel-auto-recovery";
const AUTO_RECOVERY_WINDOW_MS = 60_000;

interface Props {
  children: ReactNode;
  /** Active panel id — when it changes, a contained error auto-clears. */
  panelId?: string;
  /** Recreate the failed panel module/component before clearing the fallback. */
  onRetry?: () => void;
}

interface State {
  hasError: boolean;
  error: Error | null;
  componentStack: string;
  referenceId: string;
  copied: boolean;
}

/**
 * PanelErrorBoundary — contains a crash to the active panel instead of
 * white-screening the whole app (the GlobalErrorBoundary is the last resort).
 * Recovers automatically when the user navigates to a different panel.
 */
export default class PanelErrorBoundary extends Component<Props, State> {
  state: State = {
    hasError: false,
    error: null,
    componentStack: "",
    referenceId: "",
    copied: false,
  };

  static getDerivedStateFromError(error: Error): State {
    return {
      hasError: true,
      error,
      componentStack: "",
      referenceId: `PANEL-${Date.now().toString(36).toUpperCase()}`,
      copied: false,
    };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const referenceId = this.state.referenceId || `PANEL-${Date.now().toString(36).toUpperCase()}`;
    logRecord(
      "error",
      `[PanelErrorBoundary${this.props.panelId ? ":" + this.props.panelId : ""}] [${referenceId}] ${error.message}\n${error.stack}\n${info.componentStack}`,
    );
    this.setState({ componentStack: info.componentStack ?? "", referenceId });
    // KT: a rejected dynamic import is cached by the browser module map, so
    // recreating React.lazy cannot recover after Vite restarts or briefly
    // fails to transform a module. A guarded document reload gets a fresh map.
    if (isStalePanelLoadError(error)) {
      this.recoverStaleViteRuntime();
    }
  }

  componentDidUpdate(prev: Props) {
    if (prev.panelId !== this.props.panelId && this.state.hasError) {
      this.setState({
        hasError: false,
        error: null,
        componentStack: "",
        referenceId: "",
        copied: false,
      });
    }
  }

  diagnosticText = () => {
    const { error, componentStack, referenceId } = this.state;
    return [
      `WinCommander panel error ${referenceId || "(unassigned)"}`,
      `Panel: ${this.props.panelId ?? "unknown"}`,
      `${error?.name ?? "Error"}: ${error?.message ?? "Unknown panel error"}`,
      error?.stack ?? "",
      componentStack,
    ].filter(Boolean).join("\n");
  };

  recoverStaleViteRuntime = () => {
    try {
      const previousRaw = sessionStorage.getItem(AUTO_RECOVERY_KEY);
      const previous = previousRaw
        ? JSON.parse(previousRaw) as { timestamp?: number }
        : undefined;
      const alreadyRetried = typeof previous?.timestamp === "number"
        && Date.now() - previous.timestamp < AUTO_RECOVERY_WINDOW_MS;
      if (alreadyRetried) return;
      sessionStorage.setItem(
        AUTO_RECOVERY_KEY,
        JSON.stringify({ timestamp: Date.now() }),
      );
      window.setTimeout(() => window.location.reload(), 100);
    } catch {
      // Never auto-reload without a durable loop guard.
    }
  };

  handleRetry = () => {
    this.props.onRetry?.();
    this.setState({
      hasError: false,
      error: null,
      componentStack: "",
      referenceId: "",
      copied: false,
    });
  };

  handleCopy = () => {
    navigator.clipboard.writeText(this.diagnosticText())
      .then(() => this.setState({ copied: true }))
      .catch(() => {});
  };

  handleOpenErrorCenter = () => {
    // Navigate first, then fire the sub-tab deep link once Secret Settings
    // has mounted — mirrors the dashboard's navigate-panel + delayed
    // open-* event pattern (src/panels/dashboard/index.tsx's disk-cleanup
    // deep link). Firing the sub-event before the panel exists is a no-op:
    // Secret Settings only starts listening for it in its own mount effect.
    window.dispatchEvent(new CustomEvent("navigate-panel", { detail: "secret" }));
    window.setTimeout(() => window.dispatchEvent(new CustomEvent("open-secret-error-center")), 300);
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-full w-full items-center justify-center p-8">
          <NonIdealState
            icon="error"
            title="This panel hit a snag"
            description={
              <div className="flex flex-col items-center gap-3 text-center">
                <p className="max-w-md text-[13px] text-[var(--color-text-muted)]">
                  This panel ran into an error and was contained — the rest of WinCommander is unaffected.
                  Retry it below, or open Error Center for the complete diagnostic.
                </p>
                <div className="max-w-lg rounded-md border border-[var(--color-border)] bg-[var(--color-bg-secondary)] px-3 py-2 text-left">
                  <p className="font-mono text-[11px] text-[var(--color-text-muted)]">
                    {this.props.panelId ?? "unknown"} · {this.state.referenceId}
                  </p>
                  <p className="mt-1 break-words font-mono text-xs text-[var(--color-danger)]">
                    {this.state.error?.message ?? "Unknown panel error"}
                  </p>
                </div>
                <div className="flex flex-wrap justify-center gap-2">
                  <Button icon="refresh" text="Retry panel" intent="primary" onClick={this.handleRetry} />
                  <Button icon="duplicate" text={this.state.copied ? "Copied" : "Copy details"} onClick={this.handleCopy} />
                  <Button icon="document-open" text="Open Error Center" onClick={this.handleOpenErrorCenter} />
                  <Button icon="reset" text="Restart WinCommander" onClick={() => window.location.reload()} />
                </div>
              </div>
            }
          />
        </div>
      );
    }
    return this.props.children;
  }
}
