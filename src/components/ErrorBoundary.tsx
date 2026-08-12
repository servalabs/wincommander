import { Component, ErrorInfo, ReactNode } from "react";
import { NonIdealState, Button, Icon, Intent } from "@/components/ui/bp";
import { logRecord } from "../lib/logger";
import { invoke } from "@tauri-apps/api/core";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
  likelyCulprit: string | null;
}

export default class GlobalErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null, likelyCulprit: null };
  }

  static getDerivedStateFromError(error: Error): State {
    // Attempt to pinpoint the code location causing the crash
    const stackLines = error.stack?.split("\n") || [];
    const culpritLine = stackLines.find(line =>
      line.includes("/src/") &&
      !line.includes("node_modules") &&
      !line.includes("ErrorBoundary.tsx") &&
      !line.includes("react-dom")
    );

    let likelyCulprit = null;
    if (culpritLine) {
      // Clean up the string to show just the file and line/col
      const match = culpritLine.match(/([a-zA-Z\._-]+\.tsx?:\d+:\d+)/);
      likelyCulprit = match ? match[1] : culpritLine.trim().split(' ').pop();
    }

    return {
      hasError: true,
      error,
      likelyCulprit: likelyCulprit ? String(likelyCulprit) : null
    };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    logRecord('error', `[ErrorBoundary] CRASH: ${error.message}\nStack: ${error.stack}\nComponent Stack: ${errorInfo.componentStack}`);
    // Proactively open the log file to help the user diagnose or share the file
    setTimeout(() => {
      invoke('open_log_file').catch(() => { });
    }, 500);
  }

  handleReset = () => {
    this.setState({ hasError: false, error: null, likelyCulprit: null });
    window.location.reload();
  };

  handleCopy = () => {
    const errorText = `ERROR: ${this.state.error?.message}\nCULPRIT: ${this.state.likelyCulprit || "Unknown"}\nSTACK: ${this.state.error?.stack}`;
    navigator.clipboard.writeText(errorText);
  };

  handleOpenLog = () => {
    invoke('open_log_file').catch(() => { });
  };


  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          height: '100vh',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: 'var(--color-bg-primary)',
          color: 'var(--color-text-primary)',
          padding: '20px'
        }}>
          <NonIdealState
            icon="error"
            title="Application Crash"
            description={
              <div style={{ textAlign: 'center', width: '100%', maxWidth: '650px' }}>
                <p style={{ marginBottom: '20px', opacity: 0.8 }}>
                  A critical error occurred. We've automatically launched the log file for diagnostics. Please share this to wc@servalabs.com
                </p>

                {this.state.likelyCulprit && (
                  <div style={{
                    background: 'var(--color-warning-dim)',
                    border: '1px solid color-mix(in srgb, var(--color-warning) 30%, transparent)',
                    padding: '12px 18px',
                    borderRadius: '8px',
                    marginBottom: '15px',
                    display: 'flex',
                    alignItems: 'center',
                    gap: '12px',
                    textAlign: 'left'
                  }}>
                    <Icon icon="locate" intent={Intent.WARNING} />
                    <div>
                      <div style={{ fontSize: '10px', textTransform: 'uppercase', letterSpacing: '1px', opacity: 0.6 }}>Likely Culprit</div>
                      <div style={{ fontFamily: 'monospace', fontWeight: 'bold', color: 'var(--color-warning)' }}>{this.state.likelyCulprit}</div>
                    </div>
                  </div>
                )}

                <div style={{
                  background: 'var(--color-danger-dim)',
                  padding: '15px',
                  borderRadius: '8px',
                  fontSize: '12px',
                  fontFamily: 'monospace',
                  marginBottom: '25px',
                  maxHeight: '250px',
                  overflowY: 'auto',
                  overflowX: 'hidden',
                  border: '1px solid color-mix(in srgb, var(--color-danger) 40%, transparent)',
                  textAlign: 'left',
                  whiteSpace: 'pre-wrap',
                  overflowWrap: 'break-word'
                }}>
                  <div style={{ fontWeight: 'bold', marginBottom: '10px', color: 'var(--color-danger)' }}>
                    {this.state.error?.name}: {this.state.error?.message}
                  </div>
                  <div style={{ opacity: 0.5, fontSize: '11px', lineHeight: '1.4', overflowWrap: 'break-word' }}>
                    {this.state.error?.stack}
                  </div>
                </div>

                <div style={{ display: 'flex', flexWrap: 'wrap', gap: '10px', justifyContent: 'center' }}>
                  <Button
                    intent="primary"
                    large
                    icon="refresh"
                    text="Restart Application"
                    onClick={this.handleReset}
                  />
                  <Button
                    icon="duplicate"
                    large
                    text="Copy Error"
                    onClick={this.handleCopy}
                  />
                  <Button
                    icon="document-open"
                    large
                    text="Open Log File"
                    onClick={this.handleOpenLog}
                  />
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
